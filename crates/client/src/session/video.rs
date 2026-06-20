use std::{
  collections::{HashMap, HashSet, VecDeque},
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::{Condvar, Mutex};

use super::{LobbyConnectionWarningKind, VideoStreamError};
use crate::{
  network::{
    protocol::{
      UserId, VideoCodecId,
      data::{ForwardedVideoFrame, VideoControl},
    },
    server::{ReceivedVideoDatagram, ReceivedVideoPacket, Server, ServerError},
  },
  services::{
    profiler,
    video::{DecodedVideoFrame, NativeVideoBackend, VideoDecodeConfig, VideoDecoder, VideoError},
  },
};

pub(super) const MAX_QUEUED_VIDEO_PACKETS: usize = 64;
pub(super) const LARGE_VIDEO_BATCH_LOG_THRESHOLD: usize = 3;
const KEYFRAME_REQUEST_RETRY_INTERVAL: Duration = Duration::from_millis(750);
const SOFTWARE_BACKLOG_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(2);
const SLOW_VIDEO_DECODE_LOG_THRESHOLD: Duration = Duration::from_millis(100);
const SLOW_VIDEO_PRESENT_TIMELINE_THRESHOLD: Duration = Duration::from_millis(30);
const WATCHED_STREAM_PRESENT_JITTER_THRESHOLD: Duration = Duration::from_millis(45);
const WATCHED_STREAM_RECEIVE_JITTER_THRESHOLD: Duration = Duration::from_millis(45);
const WATCHED_STREAM_CADENCE_LOG_INTERVAL: Duration = Duration::from_secs(1);
const WATCHED_STREAM_QUEUE_IDLE_WAIT: Duration = Duration::from_millis(100);
const VIDEO_PRESENT_TIMELINE_SAMPLE_INTERVAL_MS: u64 = 1_000;
static VIDEO_PRESENT_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "windows")]
pub(super) const SHARED_NV12_PLANES_SURFACE_CACHE_LIMIT: usize = 32;
#[cfg(target_os = "windows")]
// The direct watched-stream path can decode bursts faster than the DX12 render
// queue consumes them. Keep enough native surfaces to avoid reusing a texture
// while the GPU may still sample it.
pub(super) const DX12_DECODE_SURFACE_RING_SIZE: usize = 32;
#[cfg(target_os = "windows")]
const ENABLE_DX12_NATIVE_STREAM_DECODE: bool = true;
#[cfg(target_os = "windows")]
const WINDOWS_NVIDIA_VENDOR_ID: u32 = 0x10DE;
#[cfg(target_os = "windows")]
const WINDOWS_AMD_VENDOR_ID: u32 = 0x1002;

#[cfg(target_os = "windows")]
static WINDOWS_DEFAULT_DXGI_ADAPTER_VENDOR_ID: std::sync::LazyLock<Option<u32>> =
  std::sync::LazyLock::new(windows_default_dxgi_adapter_vendor_id);

#[derive(Clone, Debug, Default)]
pub struct VideoReceiverDebugSnapshot {
  pub watched_user_id: Option<UserId>,
  pub queue_limit: usize,
  pub last_batch_queued: usize,
  pub last_batch_dropped: u64,
  pub last_dropped_senders: Vec<(UserId, u64)>,
  pub awaiting_keyframes: Vec<UserId>,
  pub awaiting_decoded_output: Vec<UserId>,
  pub expected_frame_numbers: Vec<(UserId, u32)>,
  pub received_counts: Vec<(UserId, u64)>,
  pub decoded_counts: Vec<(UserId, u64)>,
  pub keyframe_request_ages_ms: Vec<(UserId, u128)>,
}

#[derive(Debug)]
struct QueuedVideoPacket {
  packet: ForwardedVideoFrame,
  received_at: Instant,
  queued_at: Instant,
  transport: VideoPacketTransport,
}

#[derive(Debug)]
struct QueuedVideoDecode {
  packet: QueuedVideoPacket,
  output: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VideoPacketTransport {
  Stream,
  Datagram,
}

#[derive(Debug)]
struct WatchedStreamCadenceStats {
  started_at: Instant,
  samples: u32,
  frame_delta_total: u64,
  skipped_frames: u64,
  present_delta_total: Duration,
  present_delta_max: Duration,
  receive_delta_total: Duration,
  receive_delta_max: Duration,
  recv_to_present_max: Duration,
}

impl QueuedVideoPacket {
  fn from_stream(packet: ForwardedVideoFrame) -> Self {
    let now = Instant::now();
    Self {
      packet,
      received_at: now,
      queued_at: now,
      transport: VideoPacketTransport::Stream,
    }
  }

  fn from_datagram(datagram: ReceivedVideoDatagram) -> Self {
    Self {
      packet: datagram.packet,
      received_at: datagram.received_at,
      queued_at: Instant::now(),
      transport: VideoPacketTransport::Datagram,
    }
  }
}

impl std::ops::Deref for QueuedVideoPacket {
  type Target = ForwardedVideoFrame;

  fn deref(&self) -> &Self::Target {
    &self.packet
  }
}

impl VideoPacketTransport {
  fn label(self) -> &'static str {
    match self {
      Self::Stream => "stream",
      Self::Datagram => "datagram",
    }
  }
}

#[cfg(target_os = "windows")]
pub(super) static DX12_NATIVE_STREAM_DECODE_SUPPORTED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
  if !ENABLE_DX12_NATIVE_STREAM_DECODE {
    return false;
  }

  match *WINDOWS_DEFAULT_DXGI_ADAPTER_VENDOR_ID {
    Some(WINDOWS_NVIDIA_VENDOR_ID) => {
      tracing::debug!(target: "video::decode", "[video:decode] DX12 native stream decode enabled: default DXGI adapter is NVIDIA, NVDEC interop is allowed");
      true
    }
    Some(WINDOWS_AMD_VENDOR_ID) => {
      tracing::debug!(target: "video::decode", "[video:decode] DX12 native stream decode enabled: default DXGI adapter is AMD, AMF shared NV12 planes interop is allowed");
      true
    }
    Some(vendor_id) => {
      tracing::debug!(target: "video::decode",
        "[video:decode] DX12 native stream decode disabled: default DXGI adapter vendor_id=0x{vendor_id:04x} is not NVIDIA or AMD"
      );
      false
    }
    None => {
      tracing::debug!(target: "video::decode", "[video:decode] DX12 native stream decode disabled: failed to resolve default DXGI adapter");
      false
    }
  }
});

pub(super) fn native_video_backend_label(backend: NativeVideoBackend) -> &'static str {
  match backend {
    NativeVideoBackend::NvidiaNvenc => "NVIDIA NVENC",
    NativeVideoBackend::NvidiaNvdec => "NVIDIA NVDEC",
    NativeVideoBackend::AmdAmf => "AMD AMF",
    NativeVideoBackend::WindowsMediaFoundation => "Windows Media Foundation",
    NativeVideoBackend::OpenH264 => "OpenH264",
    NativeVideoBackend::SoftwareDecoder => "Software decoder",
    NativeVideoBackend::AppleVideoToolbox => "Apple VideoToolbox",
  }
}

#[cfg(target_os = "windows")]
fn windows_default_dxgi_adapter_vendor_id() -> Option<u32> {
  use ::windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

  let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.ok()?;
  let adapter = unsafe { factory.EnumAdapters1(0) }.ok()?;
  let desc = unsafe { adapter.GetDesc1() }.ok()?;
  Some(desc.VendorId)
}

pub(super) struct VideoPacketQueue {
  state: Mutex<VideoPacketQueueState>,
  notify: Condvar,
  closed: AtomicBool,
}

struct VideoPacketQueueState {
  packets: VecDeque<QueuedVideoPacket>,
  dropped_senders: HashMap<UserId, u64>,
}

enum VideoPacketPopResult {
  Batch(u64),
  Empty,
  Closed,
}

impl VideoPacketQueue {
  pub(super) fn new() -> Self {
    Self {
      state: Mutex::new(VideoPacketQueueState {
        packets: VecDeque::new(),
        dropped_senders: HashMap::new(),
      }),
      notify: Condvar::new(),
      closed: AtomicBool::new(false),
    }
  }

  fn push(&self, packet: QueuedVideoPacket) {
    if self.closed.load(Ordering::Relaxed) {
      return;
    }
    {
      let mut state = self.state.lock();
      if self.closed.load(Ordering::Relaxed) {
        return;
      }
      if state.packets.len() >= MAX_QUEUED_VIDEO_PACKETS {
        if let Some(dropped) = state.packets.pop_front() {
          *state.dropped_senders.entry(dropped.sender_id).or_insert(0) += 1;
        }
        tracing::debug!(target: "video", "[video] dropped queued stale video packet to preserve latency: max_queue={MAX_QUEUED_VIDEO_PACKETS}");
      }
      state.packets.push_back(packet);
    }
    self.notify.notify_one();
  }

  pub(super) fn push_frame(&self, packet: ForwardedVideoFrame) {
    self.push(QueuedVideoPacket::from_stream(packet));
  }

  pub(super) fn close(&self) {
    self.closed.store(true, Ordering::Relaxed);
    self.notify.notify_all();
  }

  pub(super) fn pop_batch_into(
    &self,
    stop: &AtomicBool,
    batch: &mut Vec<QueuedVideoPacket>,
    dropped_senders: &mut HashMap<UserId, u64>,
    timeout: Duration,
  ) -> VideoPacketPopResult {
    let mut state = self.state.lock();
    while state.packets.is_empty() && !self.closed.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
      if timeout.is_zero() {
        break;
      }
      self.notify.wait_for(&mut state, timeout);
      break;
    }

    if state.packets.is_empty() {
      return if self.closed.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
        VideoPacketPopResult::Closed
      } else {
        VideoPacketPopResult::Empty
      };
    }

    batch.clear();
    batch.extend(state.packets.drain(..));
    dropped_senders.clear();
    dropped_senders.extend(state.dropped_senders.drain());
    let dropped = dropped_senders.values().sum();
    VideoPacketPopResult::Batch(dropped)
  }
}

pub(super) trait VideoReceiverSession: Clone + Send + Sync + 'static {
  fn mark_video_network_activity(&self);
  fn reset_video_packet_queue(&self) -> Arc<VideoPacketQueue>;
  fn handle_video_control_packet(&self, control: VideoControl);
  fn set_video_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String);
  fn set_video_receiver_debug_snapshot(&self, snapshot: VideoReceiverDebugSnapshot);
  fn watching_user_id(&self) -> Option<UserId>;
  fn video_decode_config_for_share(&self, user_id: UserId) -> Option<VideoDecodeConfig>;
  fn set_video_error(&self, user_id: UserId, error: VideoStreamError);
  fn clear_video_error(&self, user_id: UserId);
  fn present_video_frame(&self, frame: DecodedVideoFrame);
  fn take_video_pixel_buffer(&self, user_id: UserId, width: u16, height: u16) -> Option<Vec<u8>>;
  fn has_video_frame(&self, user_id: UserId, width: u16, height: u16) -> bool;
  fn video_frame_image_id(&self, user_id: UserId) -> Option<u64>;

  #[cfg(target_os = "windows")]
  fn shared_nv12_planes_video_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, usize, usize), Arc<lurq::app::dx12_render::Dx12Nv12Surface>>,
    user_id: UserId,
    width: u16,
    height: u16,
    y_shared_handle: usize,
    uv_shared_handle: usize,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>;

  #[cfg(target_os = "windows")]
  fn dx12_video_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, u16, u16), VecDeque<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>>,
    user_id: UserId,
    width: u16,
    height: u16,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>;

  #[cfg(target_os = "windows")]
  fn present_dx12_video_frame(
    &self,
    sender_id: UserId,
    frame_number: u32,
    codec: VideoCodecId,
    width: u16,
    height: u16,
    surface: Arc<lurq::app::dx12_render::Dx12Nv12Surface>,
  );
}

pub(super) fn run_video_receiver<S>(
  session: S,
  server: Arc<Server>,
  runtime: tokio::runtime::Handle,
  stop: Arc<AtomicBool>,
) where
  S: VideoReceiverSession,
{
  tracing::debug!(target: "video", "[video] receiver thread started");
  let queue = session.reset_video_packet_queue();
  let reader_task = {
    let server = server.clone();
    let stop = stop.clone();
    let queue = queue.clone();
    let session = session.clone();
    runtime.spawn(async move {
      let mut last_watched_stream_read = None::<(UserId, u32, Instant)>;
      while !stop.load(Ordering::Relaxed) {
        let read_started_at = Instant::now();
        match server.recv_video().await {
          Ok(ReceivedVideoPacket::Frame(packet)) => {
            let read_finished_at = Instant::now();
            if Some(packet.sender_id) == session.watching_user_id() {
              log_watched_stream_read_jitter(
                &mut last_watched_stream_read,
                VideoPacketTransport::Stream,
                packet.sender_id,
                packet.frame.frame_number,
                packet.frame.keyframe,
                packet.frame.codec,
                packet.frame.encoded.len(),
                read_finished_at.duration_since(read_started_at),
                read_finished_at,
              );
            }
            session.mark_video_network_activity();
            queue.push(QueuedVideoPacket::from_stream(packet));
          }
          Ok(ReceivedVideoPacket::VideoControl(control)) => {
            session.mark_video_network_activity();
            session.handle_video_control_packet(control);
          }
          Err(ServerError::Protocol(error)) => {
            tracing::debug!(target: "video", "[video] ignored malformed video packet: {error}");
            continue;
          }
          Err(error) => {
            let error = error.to_string();
            tracing::debug!(
              target: "video",
              "[video] video stream reader transport error; waiting for keepalive/control to confirm disconnect: {error}"
            );
            session.set_video_connection_warning(
              LobbyConnectionWarningKind::VideoReceiverStopped,
              format!("Video stream receiver stopped: {error}"),
            );
            break;
          }
        }
      }
      queue.close();
    })
  };
  let datagram_reader_task = {
    let server = server.clone();
    let stop = stop.clone();
    let queue = queue.clone();
    let session = session.clone();
    runtime.spawn(async move {
      let mut last_watched_datagram_read = None::<(UserId, u32, Instant)>;
      while !stop.load(Ordering::Relaxed) {
        let read_started_at = Instant::now();
        match server.recv_forwarded_video_datagram_until(stop.as_ref()).await {
          Ok(Some(packet)) => {
            let read_finished_at = Instant::now();
            if Some(packet.packet.sender_id) == session.watching_user_id() {
              log_watched_stream_read_jitter(
                &mut last_watched_datagram_read,
                VideoPacketTransport::Datagram,
                packet.packet.sender_id,
                packet.packet.frame.frame_number,
                packet.packet.frame.keyframe,
                packet.packet.frame.codec,
                packet.packet.frame.encoded.len(),
                read_finished_at.duration_since(read_started_at),
                packet.received_at,
              );
            }
            session.mark_video_network_activity();
            queue.push(QueuedVideoPacket::from_datagram(packet));
          }
          Ok(None) => break,
          Err(ServerError::Protocol(error)) => {
            tracing::debug!(target: "video", "[video] ignored malformed video datagram: {error}");
            continue;
          }
          Err(error) => {
            let error = error.to_string();
            tracing::debug!(
              target: "video",
              "[video] video datagram reader transport error; waiting for keepalive/control to confirm disconnect: {error}"
            );
            session.set_video_connection_warning(
              LobbyConnectionWarningKind::VideoReceiverStopped,
              format!("Video datagram receiver stopped: {error}"),
            );
            break;
          }
        }
      }
    })
  };
  let mut decode_pool = VideoDecodePool::new();
  #[cfg(target_os = "windows")]
  let mut shared_nv12_planes_surfaces =
    HashMap::<(UserId, usize, usize), Arc<lurq::app::dx12_render::Dx12Nv12Surface>>::new();
  #[cfg(target_os = "windows")]
  let mut dx12_decode_surfaces =
    HashMap::<(UserId, u16, u16), VecDeque<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>>::new();
  let mut awaiting_keyframes = session.watching_user_id().into_iter().collect::<HashSet<_>>();
  let mut awaiting_decoded_output = HashSet::<UserId>::new();
  let mut expected_frame_numbers = HashMap::<UserId, u32>::new();
  let mut received_counts = HashMap::<UserId, u64>::new();
  let mut decoded_counts = HashMap::<UserId, u64>::new();
  let mut dropped_senders = HashMap::<UserId, u64>::new();
  let mut last_keyframe_requests = HashMap::<UserId, Instant>::new();
  let mut last_keyframes = HashMap::<UserId, (u32, Instant)>::new();
  let mut last_presented_frames = HashMap::<UserId, (u32, Instant, Instant)>::new();
  let mut watched_cadence_stats = HashMap::<UserId, WatchedStreamCadenceStats>::new();
  let mut last_watched_user = session.watching_user_id();
  let mut batch = Vec::<QueuedVideoPacket>::with_capacity(MAX_QUEUED_VIDEO_PACKETS);
  let mut process_batch = Vec::<QueuedVideoDecode>::with_capacity(MAX_QUEUED_VIDEO_PACKETS);

  while !stop.load(Ordering::Relaxed) {
    let watched_user = session.watching_user_id();
    if watched_user != last_watched_user {
      tracing::debug!(
        target: "video::watch",
        "watched stream target changed previous={:?} next={:?}",
        last_watched_user,
        watched_user
      );
      decode_pool.retain_watched(watched_user);
      awaiting_decoded_output.retain(|user_id| Some(*user_id) == watched_user);
      expected_frame_numbers.retain(|user_id, _| Some(*user_id) == watched_user);
      last_keyframe_requests.retain(|user_id, _| Some(*user_id) == watched_user);
      last_keyframes.retain(|user_id, _| Some(*user_id) == watched_user);
      last_presented_frames.retain(|user_id, _| Some(*user_id) == watched_user);
      watched_cadence_stats.retain(|user_id, _| Some(*user_id) == watched_user);
      #[cfg(target_os = "windows")]
      shared_nv12_planes_surfaces.retain(|(user_id, ..), _| Some(*user_id) == watched_user);
      #[cfg(target_os = "windows")]
      dx12_decode_surfaces.retain(|(user_id, ..), _| Some(*user_id) == watched_user);
      awaiting_keyframes.retain(|user_id| Some(*user_id) == watched_user);
      if let Some(user_id) = watched_user {
        awaiting_keyframes.insert(user_id);
        request_keyframe_for(
          &runtime,
          &server,
          &mut last_keyframe_requests,
          user_id,
          "watch target changed",
        );
        if let Some(config) = session.video_decode_config_for_share(user_id) {
          if let Err(error) = decode_pool.prewarm(user_id, config) {
            let error = error.to_string();
            tracing::debug!(target: "video::decode", "[video:decode] failed to prewarm decoder for user {user_id}: {error}");
            if native_decoder_unavailable_error(&error) {
              session.set_video_error(user_id, native_decoder_unavailable_stream_error(error));
            }
          }
        }
      }
      last_watched_user = watched_user;
      tracing::debug!(target: "video", "[video] watch target changed: {watched_user:?}");
    }

    batch.clear();
    dropped_senders.clear();
    let (dropped_count, batch_popped_at, has_incoming_batch) = {
      let _span = profiler::span("video.receive.pop_batch");
      match queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders, WATCHED_STREAM_QUEUE_IDLE_WAIT) {
        VideoPacketPopResult::Batch(dropped_count) => (dropped_count, Instant::now(), true),
        VideoPacketPopResult::Empty => (0, Instant::now(), false),
        VideoPacketPopResult::Closed => break,
      }
    };
    let _batch_span = profiler::span("video.receive.process_batch");

    if batch.len() >= LARGE_VIDEO_BATCH_LOG_THRESHOLD {
      tracing::debug!(target: "video",
        "[video] draining large video batch: queued={} dropped={}",
        batch.len(),
        dropped_count
      );
    }

    if dropped_count > 0 {
      let affected_users = dropped_senders.keys().copied().collect::<HashSet<_>>();
      if let Some(user_id) = watched_user
        && dropped_senders.contains_key(&user_id)
      {
        let sample_frame = batch
          .iter()
          .find(|packet| packet.sender_id == user_id)
          .map(|packet| &packet.frame);
        if sample_frame.is_none() {
          tracing::debug!(target: "video",
            "[video] watched stream packet dropped but no replacement packet was queued: user={user_id} dropped={}",
            dropped_senders.get(&user_id).copied().unwrap_or_default()
          );
          tracing::debug!(
            target: "video::watch",
            "watched stream backlog dropped user={} dropped={} queued_replacement=false",
            user_id,
            dropped_senders.get(&user_id).copied().unwrap_or_default()
          );
          if awaiting_keyframes.insert(user_id) {
            request_keyframe_for(
              &runtime,
              &server,
              &mut last_keyframe_requests,
              user_id,
              "watched video backlog dropped",
            );
          }
        }
        let sample_config = sample_frame.map(|frame| VideoDecodeConfig {
          codec: frame.codec,
          width: frame.width,
          height: frame.height,
          hardware_decoding: session
            .video_decode_config_for_share(user_id)
            .map(|config| config.hardware_decoding)
            .unwrap_or(true),
        });
        if sample_config
          .as_ref()
          .is_some_and(|config| !decode_pool.has_decoder_failure(user_id, config))
          && sample_config.as_ref().is_none_or(|config| config.hardware_decoding)
          && awaiting_keyframes.insert(user_id)
        {
          request_keyframe_for(
            &runtime,
            &server,
            &mut last_keyframe_requests,
            user_id,
            "watched video backlog dropped",
          );
        }
      }
      tracing::debug!(target: "video",
        "[video] dropping stale video backlog: queued={} dropped={} users={}",
        batch.len(),
        dropped_count,
        affected_users.len()
      );
    }

    let last_batch_queued = batch.len();
    if let Some(user_id) = watched_user {
      if last_batch_queued > 1 || dropped_count > 0 {
        let stream_packets = batch
          .iter()
          .filter(|packet| packet.sender_id == user_id && packet.transport == VideoPacketTransport::Stream)
          .count();
        let datagram_packets = batch
          .iter()
          .filter(|packet| packet.sender_id == user_id && packet.transport == VideoPacketTransport::Datagram)
          .count();
        let first_frame = batch
          .iter()
          .filter(|packet| packet.sender_id == user_id)
          .map(|packet| packet.frame.frame_number)
          .next();
        let last_frame = batch
          .iter()
          .filter(|packet| packet.sender_id == user_id)
          .map(|packet| packet.frame.frame_number)
          .last();
        tracing::debug!(
          target: "video::watch",
          "watched stream batch user={} queued={} stream={} datagram={} dropped={} first_frame={:?} last_frame={:?} expected_frame={:?} awaiting_keyframe={} awaiting_output={}",
          user_id,
          last_batch_queued,
          stream_packets,
          datagram_packets,
          dropped_count,
          first_frame,
          last_frame,
          expected_frame_numbers.get(&user_id),
          awaiting_keyframes.contains(&user_id),
          awaiting_decoded_output.contains(&user_id),
        );
      }
      order_watched_video_batch(&mut batch, user_id, expected_frame_numbers.get(&user_id).copied());
    }

    process_batch.clear();
    for packet in batch.drain(..) {
      process_batch.push(QueuedVideoDecode { packet, output: true });
    }
    if process_batch.is_empty() && !has_incoming_batch {
      session.set_video_receiver_debug_snapshot(video_receiver_debug_snapshot(
        watched_user,
        last_batch_queued,
        dropped_count,
        &dropped_senders,
        &awaiting_keyframes,
        &awaiting_decoded_output,
        &expected_frame_numbers,
        &received_counts,
        &decoded_counts,
        &last_keyframe_requests,
      ));
      continue;
    }

    for queued_decode in process_batch.drain(..) {
      let output = queued_decode.output;
      let packet = queued_decode.packet;
      if Some(packet.sender_id) != watched_user {
        decode_pool.remove_user(packet.sender_id);
        awaiting_keyframes.remove(&packet.sender_id);
        awaiting_decoded_output.remove(&packet.sender_id);
        expected_frame_numbers.remove(&packet.sender_id);
        last_keyframes.remove(&packet.sender_id);
        last_presented_frames.remove(&packet.sender_id);
        continue;
      }

      let packet_config = VideoDecodeConfig {
        codec: packet.frame.codec,
        width: packet.frame.width,
        height: packet.frame.height,
        hardware_decoding: session
          .video_decode_config_for_share(packet.sender_id)
          .map(|config| config.hardware_decoding)
          .unwrap_or(true),
      };
      if decode_pool.decoder_config_mismatch(packet.sender_id, &packet_config) {
        let previous_config = decode_pool.decoder_config(packet.sender_id).cloned();
        tracing::debug!(
          target: "video::watch",
          "watched stream decoder reset user={} reason=config changed previous={:?} next={:?}",
          packet.sender_id,
          previous_config,
          packet_config
        );
        decode_pool.reset_user(packet.sender_id);
        #[cfg(target_os = "windows")]
        shared_nv12_planes_surfaces.retain(|(user_id, ..), _| *user_id != packet.sender_id);
        #[cfg(target_os = "windows")]
        dx12_decode_surfaces.retain(|(user_id, ..), _| *user_id != packet.sender_id);
        awaiting_decoded_output.remove(&packet.sender_id);
        expected_frame_numbers.remove(&packet.sender_id);
        if awaiting_keyframes.insert(packet.sender_id) {
          request_keyframe_for(
            &runtime,
            &server,
            &mut last_keyframe_requests,
            packet.sender_id,
            "video decode config changed",
          );
        }
      }

      let receive_to_queue = packet.queued_at.duration_since(packet.received_at);
      let queue_wait = batch_popped_at.duration_since(packet.queued_at);
      let frame_received_at = packet.received_at;
      let frame_number = packet.frame.frame_number;
      let keyframe = packet.frame.keyframe;
      if keyframe {
        let previous_keyframe = last_keyframes.insert(packet.sender_id, (frame_number, Instant::now()));
        let frames_since_previous =
          previous_keyframe.map(|(previous_frame, _)| frame_number.wrapping_sub(previous_frame));
        let ms_since_previous = previous_keyframe.map(|(_, previous_at)| duration_ms(previous_at.elapsed()));
        tracing::debug!(
          target: "video::watch",
          "watched stream keyframe received user={} frame={} codec={:?} size={}x{} hw_decode={} batch_queued={} queue_wait_ms={:.1} expected_frame={:?} awaiting_keyframe={} frames_since_previous={:?} ms_since_previous={:?}",
          packet.sender_id,
          frame_number,
          packet.frame.codec,
          packet.frame.width,
          packet.frame.height,
          packet_config.hardware_decoding,
          last_batch_queued,
          duration_ms(queue_wait),
          expected_frame_numbers.get(&packet.sender_id),
          awaiting_keyframes.contains(&packet.sender_id),
          frames_since_previous,
          ms_since_previous
        );
      }

      if awaiting_keyframes.contains(&packet.sender_id) {
        if !packet.frame.keyframe {
          tracing::trace!(
            target: "video::watch::frames",
            "watched stream skipped non-keyframe user={} frame={} reason=awaiting_keyframe",
            packet.sender_id,
            packet.frame.frame_number
          );
          let interval = if packet_config.hardware_decoding {
            KEYFRAME_REQUEST_RETRY_INTERVAL
          } else {
            SOFTWARE_BACKLOG_KEYFRAME_REQUEST_INTERVAL
          };
          request_keyframe_if_due_after(
            &runtime,
            &server,
            &mut last_keyframe_requests,
            packet.sender_id,
            "still waiting for video keyframe",
            interval,
          );
          continue;
        }
        awaiting_keyframes.remove(&packet.sender_id);
        decode_pool.clear_user_failures(packet.sender_id);
        tracing::debug!(
          target: "video::watch",
          "watched stream keyframe accepted user={} frame={} codec={:?} size={}x{} hw_decode={}",
          packet.sender_id,
          packet.frame.frame_number,
          packet.frame.codec,
          packet.frame.width,
          packet.frame.height,
          packet_config.hardware_decoding
        );
        tracing::debug!(target: "video::decode",
          "[video:decode] catch-up keyframe received for user {}: frame={}",
          packet.sender_id,
          packet.frame.frame_number
        );
      }

      if packet.frame.keyframe {
        expected_frame_numbers.insert(packet.sender_id, packet.frame.frame_number.wrapping_add(1));
      } else {
        match expected_frame_numbers.get(&packet.sender_id).copied() {
          Some(expected_frame_number) if packet.frame.frame_number == expected_frame_number => {
            expected_frame_numbers.insert(packet.sender_id, packet.frame.frame_number.wrapping_add(1));
          }
          Some(expected_frame_number) => {
            if frame_number_before(packet.frame.frame_number, expected_frame_number) {
              tracing::debug!(target: "video::decode",
                "[video:decode] dropping stale video frame for user {}: expected={} actual={}",
                packet.sender_id,
                expected_frame_number,
                packet.frame.frame_number
              );
              tracing::debug!(
                target: "video::watch",
                "watched stream stale frame dropped user={} transport={} expected_frame={} actual_frame={} keyframe={}",
                packet.sender_id,
                packet.transport.label(),
                expected_frame_number,
                packet.frame.frame_number,
                packet.frame.keyframe
              );
              continue;
            } else {
              expected_frame_numbers.insert(packet.sender_id, packet.frame.frame_number.wrapping_add(1));
              tracing::debug!(target: "video::decode",
                "[video:decode] continuing across video frame gap for user {}: expected={} actual={}",
                packet.sender_id,
                expected_frame_number,
                packet.frame.frame_number
              );
              tracing::debug!(
                target: "video::watch",
                "watched stream frame gap user={} transport={} expected_frame={} actual_frame={} keyframe={}",
                packet.sender_id,
                packet.transport.label(),
                expected_frame_number,
                packet.frame.frame_number,
                packet.frame.keyframe
              );
            }
          }
          None => {
            tracing::debug!(
              target: "video::watch",
              "watched stream decoder reset user={} reason=missing initial keyframe frame={} keyframe={}",
              packet.sender_id,
              packet.frame.frame_number,
              packet.frame.keyframe
            );
            awaiting_decoded_output.remove(&packet.sender_id);
            decode_pool.reset_user(packet.sender_id);
            if awaiting_keyframes.insert(packet.sender_id) {
              request_keyframe_for(
                &runtime,
                &server,
                &mut last_keyframe_requests,
                packet.sender_id,
                "missing initial keyframe",
              );
            }
            continue;
          }
        }
      }

      let received_count = increment_counter(&mut received_counts, packet.sender_id);
      let had_known_decoder_failure = decode_pool.has_decoder_failure(packet.sender_id, &packet_config);
      tracing::trace!(
        target: "video::watch::frames",
        "watched stream packet user={} frame={} transport={} keyframe={} codec={:?} size={}x{} hw_decode={} batch_queued={} receive_to_queue_ms={:.1} queue_wait_ms={:.1}",
        packet.sender_id,
        frame_number,
        packet.transport.label(),
        keyframe,
        packet.frame.codec,
        packet.frame.width,
        packet.frame.height,
        packet_config.hardware_decoding,
        last_batch_queued,
        duration_ms(receive_to_queue),
        duration_ms(queue_wait)
      );
      if should_log_video_count(received_count) {
        tracing::debug!(target: "video::decode",
          "[video:decode] received frame #{received_count} from user {}: frame={} codec={:?} size={}x{} keyframe={} output={} bytes={}",
          packet.sender_id,
          packet.frame.frame_number,
          packet.frame.codec,
          packet.frame.width,
          packet.frame.height,
          packet.frame.keyframe,
          output,
          packet.frame.encoded.len()
        );
      }

      #[cfg(target_os = "windows")]
      if output {
        let native_decode_start = Instant::now();
        let native_decode = try_present_windows_native_video_frame(
          &session,
          &mut decode_pool,
          &mut shared_nv12_planes_surfaces,
          &mut dx12_decode_surfaces,
          &mut decoded_counts,
          received_count,
          &packet.packet,
          &packet_config,
        );
        let native_decode_present = native_decode_start.elapsed();
        match native_decode {
          WindowsNativeVideoDecode::Presented {
            frame_number: presented_frame_number,
          } => {
            let image_id = session.video_frame_image_id(packet.sender_id).unwrap_or(0);
            let presented_keyframe = keyframe && presented_frame_number == frame_number;
            tracing::trace!(
              target: "video::watch::frames",
              "watched stream presented native user={} input_frame={} frame={} keyframe={} image_id={} image_version={} recv_to_present_ms={:.1} decode_present_ms={:.1}",
              packet.sender_id,
              frame_number,
              presented_frame_number,
              presented_keyframe,
              image_id,
              0,
              duration_ms(frame_received_at.elapsed()),
              duration_ms(native_decode_present)
            );
            let native_reorder_delta = frame_number.wrapping_sub(presented_frame_number);
            if presented_frame_number != frame_number && native_reorder_delta != 1 {
              tracing::debug!(
                target: "video::watch",
                "watched stream native presented reordered user={} input_frame={} presented_frame={} delta={} keyframe={}",
                packet.sender_id,
                frame_number,
                presented_frame_number,
                native_reorder_delta,
                keyframe
              );
            }
            log_native_video_present_timeline(
              packet.sender_id,
              packet.frame.codec,
              packet.frame.width,
              packet.frame.height,
              presented_frame_number,
              presented_keyframe,
              receive_to_queue,
              queue_wait,
              native_decode_present,
              frame_received_at.elapsed(),
              image_id,
              0,
            );
            log_watched_stream_present_jitter(
              &mut last_presented_frames,
              &mut watched_cadence_stats,
              packet.sender_id,
              "native",
              presented_frame_number,
              presented_keyframe,
              frame_received_at,
              image_id,
              0,
            );
            awaiting_decoded_output.remove(&packet.sender_id);
            continue;
          }
          WindowsNativeVideoDecode::Pending => {
            tracing::trace!(
              target: "video::watch::frames",
              "watched stream native pending user={} frame={} keyframe={}",
              packet.sender_id,
              frame_number,
              keyframe
            );
            awaiting_decoded_output.insert(packet.sender_id);
            continue;
          }
          WindowsNativeVideoDecode::Fallback => {
            tracing::debug!(
              target: "video::watch",
              "watched stream native fallback user={} frame={} keyframe={} codec={:?}",
              packet.sender_id,
              frame_number,
              keyframe,
              packet.frame.codec
            );
          }
        }
      }

      let output_buffer = if output {
        session.take_video_pixel_buffer(packet.sender_id, packet.frame.width, packet.frame.height)
      } else {
        None
      };
      let sender_id = packet.sender_id;
      let codec = packet.frame.codec;
      let decode_result = {
        let _span = profiler::span("video.decode.packet");
        let decode_start = Instant::now();
        let result = decode_pool.decode_cpu(packet.packet, &packet_config, output, output_buffer);
        (result, decode_start.elapsed())
      };
      match decode_result.0 {
        Ok(Some(frame)) => {
          awaiting_decoded_output.remove(&frame.sender_id);
          session.clear_video_error(frame.sender_id);
          let decoded_count = increment_counter(&mut decoded_counts, frame.sender_id);
          if should_log_video_count(decoded_count) {
            tracing::debug!(target: "video::decode",
              "[video:decode] decoded frame #{decoded_count} from user {}: codec={:?} size={}x{} format={:?} bytes={}",
              frame.sender_id,
              frame.codec,
              frame.width,
              frame.height,
              frame.format,
              frame.pixels.len()
            );
          }
          let present_start = Instant::now();
          session.present_video_frame(frame);
          let image_id = session.video_frame_image_id(sender_id).unwrap_or(0);
          tracing::trace!(
            target: "video::watch::frames",
            "watched stream presented cpu user={} frame={} keyframe={} output={} image_id={} image_version={} recv_to_present_ms={:.1} decode_ms={:.1} present_ms={:.1}",
            sender_id,
            frame_number,
            keyframe,
            output,
            image_id,
            0,
            duration_ms(frame_received_at.elapsed()),
            duration_ms(decode_result.1),
            duration_ms(present_start.elapsed())
          );
          log_cpu_video_present_timeline(
            sender_id,
            codec,
            packet_config.width,
            packet_config.height,
            frame_number,
            keyframe,
            receive_to_queue,
            queue_wait,
            decode_result.1,
            present_start.elapsed(),
            frame_received_at.elapsed(),
            image_id,
            0,
          );
          log_watched_stream_present_jitter(
            &mut last_presented_frames,
            &mut watched_cadence_stats,
            sender_id,
            "cpu",
            frame_number,
            keyframe,
            frame_received_at,
            image_id,
            0,
          );
        }
        Ok(None) => {
          tracing::trace!(
            target: "video::watch::frames",
            "watched stream decoded no output user={} frame={} keyframe={} output={}",
            sender_id,
            frame_number,
            keyframe,
            output
          );
          if output {
            awaiting_decoded_output.insert(sender_id);
          }
          if !had_known_decoder_failure && should_log_video_count(received_count) {
            tracing::debug!(target: "video::decode",
              "[video:decode] received frame produced no decoded output: output_requested={output}"
            );
          }
        }
        Err(error) => {
          awaiting_decoded_output.remove(&sender_id);
          expected_frame_numbers.remove(&sender_id);
          tracing::debug!(
            target: "video::watch",
            "watched stream decoder reset user={} reason=decode failed frame={} keyframe={} error={}",
            sender_id,
            frame_number,
            keyframe,
            error
          );
          if unsupported_av1_decode_error(codec, &error) {
            session.set_video_error(sender_id, unsupported_av1_stream_error());
          } else if native_decoder_unavailable_error(&error) {
            session.set_video_error(sender_id, native_decoder_unavailable_stream_error(error));
          } else {
            if awaiting_keyframes.insert(sender_id) {
              request_keyframe_for(
                &runtime,
                &server,
                &mut last_keyframe_requests,
                sender_id,
                "video decode failed",
              );
            }
          }
        }
      }
    }

    session.set_video_receiver_debug_snapshot(video_receiver_debug_snapshot(
      watched_user,
      last_batch_queued,
      dropped_count,
      &dropped_senders,
      &awaiting_keyframes,
      &awaiting_decoded_output,
      &expected_frame_numbers,
      &received_counts,
      &decoded_counts,
      &last_keyframe_requests,
    ));
  }

  queue.close();
  drop(reader_task);
  drop(datagram_reader_task);
  tracing::debug!(target: "video", "[video] receiver thread stopping");
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsNativeVideoDecode {
  Presented { frame_number: u32 },
  Pending,
  Fallback,
}

#[cfg(target_os = "windows")]
fn try_present_windows_native_video_frame<S: VideoReceiverSession>(
  session: &S,
  decode_pool: &mut VideoDecodePool,
  shared_nv12_planes_surfaces: &mut HashMap<(UserId, usize, usize), Arc<lurq::app::dx12_render::Dx12Nv12Surface>>,
  dx12_decode_surfaces: &mut HashMap<(UserId, u16, u16), VecDeque<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>>,
  decoded_counts: &mut HashMap<UserId, u64>,
  received_count: u64,
  packet: &ForwardedVideoFrame,
  packet_config: &VideoDecodeConfig,
) -> WindowsNativeVideoDecode {
  if !packet_config.hardware_decoding {
    return WindowsNativeVideoDecode::Fallback;
  }

  if let Some(shared_planes_result) = decode_pool.decode_to_shared_nv12_planes(packet, packet_config) {
    match shared_planes_result {
      Ok(Some((y_shared_handle, uv_shared_handle))) => {
        if let Some(surface) = session.shared_nv12_planes_video_surface_for_decode(
          shared_nv12_planes_surfaces,
          packet.sender_id,
          packet.frame.width,
          packet.frame.height,
          y_shared_handle,
          uv_shared_handle,
        ) {
          let decoded_count = increment_counter(decoded_counts, packet.sender_id);
          if should_log_video_count(decoded_count) {
            tracing::debug!(target: "video::decode",
              "[video:decode] decoded shared NV12 planes frame #{decoded_count} from user {}: codec={:?} size={}x{} y_handle=0x{y_shared_handle:x} uv_handle=0x{uv_shared_handle:x}",
              packet.sender_id,
              packet.frame.codec,
              packet.frame.width,
              packet.frame.height
            );
          }
          session.present_dx12_video_frame(
            packet.sender_id,
            packet.frame.frame_number,
            packet.frame.codec,
            packet.frame.width,
            packet.frame.height,
            surface,
          );
          return WindowsNativeVideoDecode::Presented {
            frame_number: packet.frame.frame_number,
          };
        }
        decode_pool.mark_shared_nv12_planes_failure(packet.sender_id, packet_config);
      }
      Ok(None) => {
        if should_log_video_count(received_count) {
          tracing::debug!(target: "video::decode", "[video:decode] received frame produced no shared NV12 planes decoded output yet");
        }
        return WindowsNativeVideoDecode::Pending;
      }
      Err(_) => {
        // The failure key is already recorded by decode_to_shared_nv12_planes.
        // Fall through to the regular decode path so playback still starts.
      }
    }
  }

  if !windows_dx12_surface_decode_allowed(packet.frame.codec) {
    return WindowsNativeVideoDecode::Fallback;
  }

  if let Some(surface) = session.dx12_video_surface_for_decode(
    dx12_decode_surfaces,
    packet.sender_id,
    packet.frame.width,
    packet.frame.height,
  ) && let Some(decoded_frame_number) = decode_pool.decode_to_dx12(packet, packet_config, &surface)
  {
    if let Some(decoded_frame_number) = decoded_frame_number {
      let decoded_count = increment_counter(decoded_counts, packet.sender_id);
      if should_log_video_count(decoded_count) {
        tracing::debug!(target: "video::decode",
          "[video:decode] decoded DX12 frame #{decoded_count} from user {}: input_frame={} output_frame={} codec={:?} size={}x{} format=Nv12",
          packet.sender_id,
          packet.frame.frame_number,
          decoded_frame_number,
          packet.frame.codec,
          packet.frame.width,
          packet.frame.height
        );
      }
      session.present_dx12_video_frame(
        packet.sender_id,
        decoded_frame_number,
        packet.frame.codec,
        packet.frame.width,
        packet.frame.height,
        surface,
      );
      WindowsNativeVideoDecode::Presented {
        frame_number: decoded_frame_number,
      }
    } else {
      if should_log_video_count(received_count) {
        tracing::debug!(target: "video::decode", "[video:decode] received frame produced no DX12 decoded output yet");
      }
      WindowsNativeVideoDecode::Pending
    }
  } else {
    WindowsNativeVideoDecode::Fallback
  }
}

#[cfg(target_os = "windows")]
fn windows_dx12_surface_decode_allowed(codec: VideoCodecId) -> bool {
  match *WINDOWS_DEFAULT_DXGI_ADAPTER_VENDOR_ID {
    Some(WINDOWS_NVIDIA_VENDOR_ID) => true,
    Some(WINDOWS_AMD_VENDOR_ID) => codec == VideoCodecId::H264,
    _ => false,
  }
}

fn order_watched_video_batch(
  batch: &mut [QueuedVideoPacket],
  watched_user_id: UserId,
  expected_frame_number: Option<u32>,
) {
  batch.sort_by_key(|packet| {
    if packet.sender_id != watched_user_id {
      return (1_u8, u32::MAX);
    }

    let frame_number = packet.frame.frame_number;
    let distance = expected_frame_number.map_or(frame_number, |expected| {
      if frame_number_before(frame_number, expected) {
        u32::MAX
      } else {
        frame_number.wrapping_sub(expected)
      }
    });

    (0_u8, distance)
  });
}

fn frame_number_before(frame_number: u32, expected_frame_number: u32) -> bool {
  let delta = expected_frame_number.wrapping_sub(frame_number);
  delta != 0 && delta < (u32::MAX / 2)
}

fn latest_watched_frame_number(batch: &[QueuedVideoPacket], watched_user_id: Option<UserId>) -> Option<u32> {
  batch
    .iter()
    .filter(|packet| Some(packet.sender_id) == watched_user_id)
    .map(|packet| packet.frame.frame_number)
    .reduce(|latest, frame_number| {
      if frame_number_after(frame_number, latest) {
        frame_number
      } else {
        latest
      }
    })
}

fn frame_number_after(frame_number: u32, previous_frame_number: u32) -> bool {
  let delta = frame_number.wrapping_sub(previous_frame_number);
  delta != 0 && delta < (u32::MAX / 2)
}

fn log_watched_stream_read_jitter(
  last_read: &mut Option<(UserId, u32, Instant)>,
  transport: VideoPacketTransport,
  user_id: UserId,
  frame_number: u32,
  keyframe: bool,
  codec: VideoCodecId,
  encoded_len: usize,
  read_wait: Duration,
  read_finished_at: Instant,
) {
  let previous = last_read.replace((user_id, frame_number, read_finished_at));
  let Some((previous_user_id, previous_frame, previous_at)) = previous else {
    return;
  };
  if previous_user_id != user_id {
    return;
  }

  let since_previous_read = read_finished_at.duration_since(previous_at);
  if read_wait < WATCHED_STREAM_RECEIVE_JITTER_THRESHOLD
    && since_previous_read < WATCHED_STREAM_RECEIVE_JITTER_THRESHOLD
  {
    return;
  }

  tracing::debug!(
    target: "video::watch",
    "watched stream read jitter transport={} user={} frame={} keyframe={} codec={:?} bytes={} frame_delta={} read_wait_ms={:.1} since_previous_read_ms={:.1}",
    transport.label(),
    user_id,
    frame_number,
    keyframe,
    codec,
    encoded_len,
    frame_number.wrapping_sub(previous_frame),
    duration_ms(read_wait),
    duration_ms(since_previous_read),
  );
}

fn log_watched_stream_present_jitter(
  last_presented_frames: &mut HashMap<UserId, (u32, Instant, Instant)>,
  cadence_stats: &mut HashMap<UserId, WatchedStreamCadenceStats>,
  user_id: UserId,
  kind: &'static str,
  frame_number: u32,
  keyframe: bool,
  frame_received_at: Instant,
  image_id: u64,
  image_version: u64,
) {
  let now = Instant::now();
  let previous = last_presented_frames.insert(user_id, (frame_number, now, frame_received_at));
  let Some((previous_frame, previous_at, previous_received_at)) = previous else {
    return;
  };

  let frame_delta = frame_number.wrapping_sub(previous_frame);
  let present_delta = now.duration_since(previous_at);
  let receive_delta = frame_received_at.duration_since(previous_received_at);
  let recv_to_present = frame_received_at.elapsed();
  log_watched_stream_cadence(
    cadence_stats,
    user_id,
    kind,
    frame_number,
    frame_delta,
    receive_delta,
    present_delta,
    recv_to_present,
  );

  let skipped_frames = frame_delta.saturating_sub(1);
  if present_delta < WATCHED_STREAM_PRESENT_JITTER_THRESHOLD
    && receive_delta < WATCHED_STREAM_RECEIVE_JITTER_THRESHOLD
    && skipped_frames == 0
  {
    return;
  }

  tracing::debug!(
    target: "video::watch",
    "watched stream present jitter user={} kind={} frame={} keyframe={} frame_delta={} skipped={} receive_delta_ms={:.1} present_delta_ms={:.1} recv_to_present_ms={:.1} image_id={} image_version={}",
    user_id,
    kind,
    frame_number,
    keyframe,
    frame_delta,
    skipped_frames,
    duration_ms(receive_delta),
    duration_ms(present_delta),
    duration_ms(recv_to_present),
    image_id,
    image_version
  );
}

fn log_watched_stream_cadence(
  cadence_stats: &mut HashMap<UserId, WatchedStreamCadenceStats>,
  user_id: UserId,
  kind: &'static str,
  frame_number: u32,
  frame_delta: u32,
  receive_delta: Duration,
  present_delta: Duration,
  recv_to_present: Duration,
) {
  let now = Instant::now();
  let stats = cadence_stats
    .entry(user_id)
    .or_insert_with(|| WatchedStreamCadenceStats {
      started_at: now,
      samples: 0,
      frame_delta_total: 0,
      skipped_frames: 0,
      present_delta_total: Duration::ZERO,
      present_delta_max: Duration::ZERO,
      receive_delta_total: Duration::ZERO,
      receive_delta_max: Duration::ZERO,
      recv_to_present_max: Duration::ZERO,
    });

  stats.samples += 1;
  stats.frame_delta_total += u64::from(frame_delta);
  stats.skipped_frames += u64::from(frame_delta.saturating_sub(1));
  stats.present_delta_total += present_delta;
  stats.present_delta_max = stats.present_delta_max.max(present_delta);
  stats.receive_delta_total += receive_delta;
  stats.receive_delta_max = stats.receive_delta_max.max(receive_delta);
  stats.recv_to_present_max = stats.recv_to_present_max.max(recv_to_present);

  if now.duration_since(stats.started_at) < WATCHED_STREAM_CADENCE_LOG_INTERVAL {
    return;
  }

  let samples = stats.samples.max(1);
  tracing::debug!(
    target: "video::watch",
    "watched stream cadence user={} kind={} last_frame={} samples={} frame_delta_total={} skipped={} receive_avg_ms={:.1} receive_max_ms={:.1} present_avg_ms={:.1} present_max_ms={:.1} recv_to_present_max_ms={:.1}",
    user_id,
    kind,
    frame_number,
    stats.samples,
    stats.frame_delta_total,
    stats.skipped_frames,
    duration_ms(stats.receive_delta_total / samples),
    duration_ms(stats.receive_delta_max),
    duration_ms(stats.present_delta_total / samples),
    duration_ms(stats.present_delta_max),
    duration_ms(stats.recv_to_present_max),
  );

  *stats = WatchedStreamCadenceStats {
    started_at: now,
    samples: 0,
    frame_delta_total: 0,
    skipped_frames: 0,
    present_delta_total: Duration::ZERO,
    present_delta_max: Duration::ZERO,
    receive_delta_total: Duration::ZERO,
    receive_delta_max: Duration::ZERO,
    recv_to_present_max: Duration::ZERO,
  };
}

fn video_receiver_debug_snapshot(
  watched_user_id: Option<UserId>,
  last_batch_queued: usize,
  last_batch_dropped: u64,
  dropped_senders: &HashMap<UserId, u64>,
  awaiting_keyframes: &HashSet<UserId>,
  awaiting_decoded_output: &HashSet<UserId>,
  expected_frame_numbers: &HashMap<UserId, u32>,
  received_counts: &HashMap<UserId, u64>,
  decoded_counts: &HashMap<UserId, u64>,
  last_keyframe_requests: &HashMap<UserId, Instant>,
) -> VideoReceiverDebugSnapshot {
  let now = Instant::now();
  let keyframe_request_ages = last_keyframe_requests
    .iter()
    .map(|(user_id, requested_at)| (*user_id, now.duration_since(*requested_at).as_millis()))
    .collect::<HashMap<_, _>>();
  VideoReceiverDebugSnapshot {
    watched_user_id,
    queue_limit: MAX_QUEUED_VIDEO_PACKETS,
    last_batch_queued,
    last_batch_dropped,
    last_dropped_senders: sorted_pairs(dropped_senders),
    awaiting_keyframes: sorted_user_ids(awaiting_keyframes),
    awaiting_decoded_output: sorted_user_ids(awaiting_decoded_output),
    expected_frame_numbers: sorted_pairs(expected_frame_numbers),
    received_counts: sorted_pairs(received_counts),
    decoded_counts: sorted_pairs(decoded_counts),
    keyframe_request_ages_ms: sorted_pairs(&keyframe_request_ages),
  }
}

fn sorted_user_ids(ids: &HashSet<UserId>) -> Vec<UserId> {
  let mut ids = ids.iter().copied().collect::<Vec<_>>();
  ids.sort_unstable();
  ids
}

fn sorted_pairs<T: Copy>(map: &HashMap<UserId, T>) -> Vec<(UserId, T)> {
  let mut pairs = map
    .iter()
    .map(|(user_id, value)| (*user_id, *value))
    .collect::<Vec<_>>();
  pairs.sort_unstable_by_key(|(user_id, _)| *user_id);
  pairs
}

fn request_keyframe_if_due_after(
  runtime: &tokio::runtime::Handle,
  server: &Arc<Server>,
  last_keyframe_requests: &mut HashMap<UserId, Instant>,
  user_id: UserId,
  reason: &str,
  interval: Duration,
) {
  let now = Instant::now();
  if last_keyframe_requests
    .get(&user_id)
    .is_some_and(|last| now.duration_since(*last) < interval)
  {
    return;
  }

  request_keyframe_for(runtime, server, last_keyframe_requests, user_id, reason);
}

fn request_keyframe_for(
  runtime: &tokio::runtime::Handle,
  server: &Arc<Server>,
  last_keyframe_requests: &mut HashMap<UserId, Instant>,
  user_id: UserId,
  reason: &str,
) {
  last_keyframe_requests.insert(user_id, Instant::now());
  match runtime.block_on(server.view_screen_share(user_id)) {
    Ok(()) => {
      tracing::debug!(
        target: "video::watch",
        "watched stream keyframe request user={} reason={} step=view_refresh result=ok",
        user_id,
        reason
      );
      tracing::debug!(target: "video", "[video] stream view refreshed for keyframe recovery: user={user_id} reason={reason}");
    }
    Err(error) => {
      tracing::debug!(
        target: "video::watch",
        "watched stream keyframe request user={} reason={} step=view_refresh result=error error={}",
        user_id,
        reason,
        error
      );
      tracing::warn!(target: "video", "[video] stream view refresh failed during keyframe recovery: user={user_id} reason={reason} error={error}");
    }
  }
  match runtime.block_on(server.request_keyframe_stream(user_id)) {
    Ok(()) => {
      tracing::debug!(
        target: "video::watch",
        "watched stream keyframe request user={} reason={} step=stream result=ok",
        user_id,
        reason
      );
      tracing::debug!(target: "video", "[video] keyframe requested for user {user_id}: reason={reason}");
    }
    Err(stream_error) => {
      tracing::debug!(
        target: "video::watch",
        "watched stream keyframe request user={} reason={} step=stream result=error error={}",
        user_id,
        reason,
        stream_error
      );
      tracing::debug!(target: "video", "[video] stream keyframe request failed for user {user_id}: reason={reason} error={stream_error}; trying datagram");
      match server.request_keyframe(user_id) {
        Ok(()) => {
          tracing::debug!(
            target: "video::watch",
            "watched stream keyframe request user={} reason={} step=datagram result=ok",
            user_id,
            reason
          );
          tracing::debug!(target: "video", "[video] datagram keyframe requested for user {user_id}: reason={reason}")
        }
        Err(datagram_error) => {
          tracing::debug!(
            target: "video::watch",
            "watched stream keyframe request user={} reason={} step=datagram result=error error={}",
            user_id,
            reason,
            datagram_error
          );
          tracing::warn!(target: "video", "[video] datagram keyframe request failed for user {user_id}: reason={reason} error={datagram_error}");
        }
      }
    }
  }
}

fn increment_counter(counters: &mut HashMap<UserId, u64>, user_id: UserId) -> u64 {
  let counter = counters.entry(user_id).or_insert(0);
  *counter += 1;
  *counter
}

fn should_log_video_count(count: u64) -> bool {
  count == 1 || count % 120 == 0
}

pub(super) struct VideoDecodePool {
  decoders: HashMap<UserId, VideoDecoder>,
  decoder_failures: HashSet<(UserId, VideoDecodeConfig)>,
  #[cfg(target_os = "windows")]
  dx12_failures: HashSet<(UserId, VideoDecodeConfig)>,
  #[cfg(target_os = "windows")]
  shared_nv12_planes_failures: HashSet<(UserId, VideoDecodeConfig)>,
}

impl VideoDecodePool {
  pub(super) fn new() -> Self {
    Self {
      decoders: HashMap::new(),
      decoder_failures: HashSet::new(),
      #[cfg(target_os = "windows")]
      dx12_failures: HashSet::new(),
      #[cfg(target_os = "windows")]
      shared_nv12_planes_failures: HashSet::new(),
    }
  }

  pub(super) fn retain_watched(&mut self, watched_user: Option<UserId>) {
    self.decoders.retain(|user_id, _| Some(*user_id) == watched_user);
    self
      .decoder_failures
      .retain(|(user_id, _)| Some(*user_id) == watched_user);
    #[cfg(target_os = "windows")]
    self.dx12_failures.retain(|(user_id, _)| Some(*user_id) == watched_user);
    #[cfg(target_os = "windows")]
    self
      .shared_nv12_planes_failures
      .retain(|(user_id, _)| Some(*user_id) == watched_user);
  }

  pub(super) fn remove_user(&mut self, user_id: UserId) {
    self.decoders.remove(&user_id);
  }

  pub(super) fn reset_user(&mut self, user_id: UserId) {
    self.decoders.remove(&user_id);
    self
      .decoder_failures
      .retain(|(failed_user_id, _)| *failed_user_id != user_id);
    #[cfg(target_os = "windows")]
    self
      .dx12_failures
      .retain(|(failed_user_id, _)| *failed_user_id != user_id);
    #[cfg(target_os = "windows")]
    self
      .shared_nv12_planes_failures
      .retain(|(failed_user_id, _)| *failed_user_id != user_id);
  }

  pub(super) fn clear_user_failures(&mut self, user_id: UserId) {
    self
      .decoder_failures
      .retain(|(failed_user_id, _)| *failed_user_id != user_id);
    #[cfg(target_os = "windows")]
    self
      .dx12_failures
      .retain(|(failed_user_id, _)| *failed_user_id != user_id);
    #[cfg(target_os = "windows")]
    self
      .shared_nv12_planes_failures
      .retain(|(failed_user_id, _)| *failed_user_id != user_id);
  }

  pub(super) fn decoder_config(&self, user_id: UserId) -> Option<&VideoDecodeConfig> {
    self.decoders.get(&user_id).map(VideoDecoder::config)
  }

  pub(super) fn decoder_config_mismatch(&self, user_id: UserId, config: &VideoDecodeConfig) -> bool {
    self
      .decoders
      .get(&user_id)
      .is_some_and(|decoder| decoder.config() != config)
  }

  pub(super) fn prewarm(&mut self, user_id: UserId, config: VideoDecodeConfig) -> Result<(), VideoError> {
    let decoder_needs_start = self
      .decoders
      .get(&user_id)
      .is_none_or(|decoder| decoder.config() != &config);
    if !decoder_needs_start {
      return Ok(());
    }

    self.decoders.remove(&user_id);
    let start = Instant::now();
    let decoder = VideoDecoder::start(config.clone())?;
    let start_elapsed = start.elapsed();
    let backend = decoder.backend();
    tracing::debug!(target: "video::decode",
      "[video:decode] decoder backend prewarmed for user {user_id}: backend={} codec={:?} size={}x{} init_ms={:.1}",
      native_video_backend_label(backend),
      config.codec,
      config.width,
      config.height,
      duration_ms(start_elapsed)
    );
    self.decoders.insert(user_id, decoder);
    Ok(())
  }

  pub(super) fn has_decoder_failure(&self, user_id: UserId, config: &VideoDecodeConfig) -> bool {
    self.decoder_failures.contains(&(user_id, config.to_owned()))
  }

  pub(super) fn decode_cpu(
    &mut self,
    packet: ForwardedVideoFrame,
    config: &VideoDecodeConfig,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<DecodedVideoFrame>, String> {
    let failure_key = (packet.sender_id, config.to_owned());

    if self.decoder_failures.contains(&failure_key) {
      return Ok(None);
    }

    let decoder_needs_start = self
      .decoders
      .get(&packet.sender_id)
      .is_none_or(|decoder| decoder.config() != config);
    if decoder_needs_start {
      self.decoders.remove(&packet.sender_id);
      let start = Instant::now();
      match VideoDecoder::start(config.clone()) {
        Ok(decoder) => {
          let start_elapsed = start.elapsed();
          let backend = decoder.backend();
          tracing::debug!(target: "video::decode",
            "[video:decode] decoder backend selected for user {}: backend={} codec={:?} size={}x{} init_ms={:.1}",
            packet.sender_id,
            native_video_backend_label(backend),
            config.codec,
            config.width,
            config.height,
            duration_ms(start_elapsed)
          );
          self.decoders.insert(packet.sender_id, decoder);
        }
        Err(error) => {
          tracing::debug!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
          self.decoder_failures.insert(failure_key);
          return Err(error.to_string());
        }
      }
    }

    let Some(decoder) = self.decoders.get_mut(&packet.sender_id) else {
      return Ok(None);
    };
    let backend = decoder.backend();
    let decode_start = Instant::now();
    match decoder.decode_with_output_buffer(&packet, output, output_buffer) {
      Ok(frame) => {
        log_decode_timing(&packet, backend, output, frame.is_some(), decode_start.elapsed());
        Ok(frame)
      }
      Err(error) => {
        log_decode_timing(&packet, backend, output, false, decode_start.elapsed());
        tracing::debug!(target: "video::decode", "[video:decode] failed to decode frame from user {}: {error}", packet.sender_id);
        self.decoders.remove(&packet.sender_id);
        self.decoder_failures.insert(failure_key);
        Err(error.to_string())
      }
    }
  }

  #[cfg(target_os = "windows")]
  pub(super) fn decode_to_dx12(
    &mut self,
    packet: &ForwardedVideoFrame,
    config: &VideoDecodeConfig,
    surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Option<Option<u32>> {
    let failure_key = (packet.sender_id, config.to_owned());

    if self.dx12_failures.contains(&failure_key) {
      return None;
    }
    if self.decoder_failures.contains(&failure_key) {
      return Some(None);
    }

    let decoder_needs_start = self
      .decoders
      .get(&packet.sender_id)
      .is_none_or(|decoder| decoder.config() != config);
    if decoder_needs_start {
      self.decoders.remove(&packet.sender_id);
      let start = Instant::now();
      match VideoDecoder::start(config.clone()) {
        Ok(decoder) => {
          let start_elapsed = start.elapsed();
          let backend = decoder.backend();
          tracing::debug!(target: "video::decode",
            "[video:decode] decoder backend selected for user {}: backend={} codec={:?} size={}x{} dx12_prepath=true init_ms={:.1}",
            packet.sender_id,
            native_video_backend_label(backend),
            config.codec,
            config.width,
            config.height,
            duration_ms(start_elapsed)
          );
          self.decoders.insert(packet.sender_id, decoder);
        }
        Err(error) => {
          tracing::debug!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
          self.decoder_failures.insert(failure_key);
          return Some(None);
        }
      }
    }

    let decoder = self.decoders.get_mut(&packet.sender_id)?;
    let dx12_backend_allowed = decoder.backend() == NativeVideoBackend::NvidiaNvdec
      || (decoder.backend() == NativeVideoBackend::AmdAmf && config.codec == VideoCodecId::H264);
    if !dx12_backend_allowed {
      return None;
    }

    match decoder.decode_to_dx12_surface(packet, surface) {
      Ok(decoded) => Some(decoded),
      Err(error) => {
        tracing::debug!(target: "video::decode",
          "[video:decode] failed to decode frame from user {} into DX12 surface: {error}",
          packet.sender_id
        );
        self.decoders.remove(&packet.sender_id);
        self.dx12_failures.insert(failure_key);
        None
      }
    }
  }

  #[cfg(target_os = "windows")]
  pub(super) fn decode_to_shared_nv12_planes(
    &mut self,
    packet: &ForwardedVideoFrame,
    config: &VideoDecodeConfig,
  ) -> Option<Result<Option<(usize, usize)>, String>> {
    if !*DX12_NATIVE_STREAM_DECODE_SUPPORTED {
      return None;
    }

    let failure_key = (packet.sender_id, config.to_owned());
    if self.shared_nv12_planes_failures.contains(&failure_key) {
      return None;
    }
    if self.decoder_failures.contains(&failure_key) {
      return None;
    }

    let decoder_needs_start = self
      .decoders
      .get(&packet.sender_id)
      .is_none_or(|decoder| decoder.config() != config);
    if decoder_needs_start {
      self.decoders.remove(&packet.sender_id);
      let start = Instant::now();
      match VideoDecoder::start(config.clone()) {
        Ok(decoder) => {
          let start_elapsed = start.elapsed();
          let backend = decoder.backend();
          tracing::debug!(target: "video::decode",
            "[video:decode] decoder backend selected for user {}: backend={} codec={:?} size={}x{} shared_nv12_planes_prepath={} init_ms={:.1}",
            packet.sender_id,
            native_video_backend_label(backend),
            config.codec,
            config.width,
            config.height,
            backend == NativeVideoBackend::AmdAmf,
            duration_ms(start_elapsed)
          );
          self.decoders.insert(packet.sender_id, decoder);
        }
        Err(error) => {
          tracing::debug!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
          self.decoder_failures.insert(failure_key);
          return None;
        }
      }
    }

    let decoder = self.decoders.get_mut(&packet.sender_id)?;
    if decoder.backend() != NativeVideoBackend::AmdAmf {
      return None;
    }

    Some(decoder.decode_to_shared_nv12_planes(packet).map_err(|error| {
      tracing::debug!(
        target: "video::decode",
        "[video:decode] failed to decode frame from user {} into shared NV12 plane textures: {error}",
        packet.sender_id
      );
      self.decoders.remove(&packet.sender_id);
      self.shared_nv12_planes_failures.insert(failure_key);
      error.to_string()
    }))
  }

  #[cfg(target_os = "windows")]
  pub(super) fn mark_shared_nv12_planes_failure(&mut self, user_id: UserId, config: &VideoDecodeConfig) {
    self.shared_nv12_planes_failures.insert((user_id, config.to_owned()));
    self.decoders.remove(&user_id);
  }
}

fn log_decode_timing(
  packet: &ForwardedVideoFrame,
  backend: NativeVideoBackend,
  output: bool,
  produced_frame: bool,
  elapsed: Duration,
) {
  if elapsed < SLOW_VIDEO_DECODE_LOG_THRESHOLD {
    return;
  }

  tracing::debug!(target: "video::decode",
    "[video:decode] slow frame decode: user={} backend={} codec={:?} size={}x{} frame={} keyframe={} output={} produced_frame={} decode_ms={:.1}",
    packet.sender_id,
    native_video_backend_label(backend),
    packet.frame.codec,
    packet.frame.width,
    packet.frame.height,
    packet.frame.frame_number,
    packet.frame.keyframe,
    output,
    produced_frame,
    duration_ms(elapsed)
  );
}

fn log_cpu_video_present_timeline(
  user_id: UserId,
  codec: VideoCodecId,
  width: u16,
  height: u16,
  frame_number: u32,
  keyframe: bool,
  receive_to_queue: Duration,
  queue_wait: Duration,
  decode: Duration,
  present: Duration,
  total: Duration,
  image_id: u64,
  image_version: u64,
) {
  let slow = total >= SLOW_VIDEO_PRESENT_TIMELINE_THRESHOLD;
  if slow {
    tracing::debug!(
      target: "video::decode",
      "[video:timeline] presented kind=cpu user={} codec={:?} size={}x{} frame={} keyframe={} image_id={} image_version={} recv_to_queue_ms={:.1} queue_wait_ms={:.1} decode_ms={:.1} present_ms={:.1} total_ms={:.1} slow={}",
      user_id,
      codec,
      width,
      height,
      frame_number,
      keyframe,
      image_id,
      image_version,
      duration_ms(receive_to_queue),
      duration_ms(queue_wait),
      duration_ms(decode),
      duration_ms(present),
      duration_ms(total),
      slow
    );
  } else if should_log_video_present_timeline_sample() {
    tracing::debug!(
      target: "video::timeline",
      "[video:timeline] presented kind=cpu user={} codec={:?} size={}x{} frame={} keyframe={} image_id={} image_version={} recv_to_queue_ms={:.1} queue_wait_ms={:.1} decode_ms={:.1} present_ms={:.1} total_ms={:.1} slow={}",
      user_id,
      codec,
      width,
      height,
      frame_number,
      keyframe,
      image_id,
      image_version,
      duration_ms(receive_to_queue),
      duration_ms(queue_wait),
      duration_ms(decode),
      duration_ms(present),
      duration_ms(total),
      slow
    );
  }
}

fn log_native_video_present_timeline(
  user_id: UserId,
  codec: VideoCodecId,
  width: u16,
  height: u16,
  frame_number: u32,
  keyframe: bool,
  receive_to_queue: Duration,
  queue_wait: Duration,
  decode_present: Duration,
  total: Duration,
  image_id: u64,
  image_version: u64,
) {
  let slow = total >= SLOW_VIDEO_PRESENT_TIMELINE_THRESHOLD;
  if slow {
    tracing::debug!(
      target: "video::decode",
      "[video:timeline] presented kind=native user={} codec={:?} size={}x{} frame={} keyframe={} image_id={} image_version={} recv_to_queue_ms={:.1} queue_wait_ms={:.1} decode_present_ms={:.1} total_ms={:.1} slow={}",
      user_id,
      codec,
      width,
      height,
      frame_number,
      keyframe,
      image_id,
      image_version,
      duration_ms(receive_to_queue),
      duration_ms(queue_wait),
      duration_ms(decode_present),
      duration_ms(total),
      slow
    );
  } else if should_log_video_present_timeline_sample() {
    tracing::debug!(
      target: "video::timeline",
      "[video:timeline] presented kind=native user={} codec={:?} size={}x{} frame={} keyframe={} image_id={} image_version={} recv_to_queue_ms={:.1} queue_wait_ms={:.1} decode_present_ms={:.1} total_ms={:.1} slow={}",
      user_id,
      codec,
      width,
      height,
      frame_number,
      keyframe,
      image_id,
      image_version,
      duration_ms(receive_to_queue),
      duration_ms(queue_wait),
      duration_ms(decode_present),
      duration_ms(total),
      slow
    );
  }
}

fn should_log_video_present_timeline_sample() -> bool {
  should_log_timeline_sample(
    &VIDEO_PRESENT_TIMELINE_LAST_INFO_MS,
    VIDEO_PRESENT_TIMELINE_SAMPLE_INTERVAL_MS,
  )
}

fn should_log_timeline_sample(last_log_ms: &AtomicU64, interval_ms: u64) -> bool {
  let now_ms = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis()
    .min(u128::from(u64::MAX)) as u64;
  let previous_ms = last_log_ms.load(Ordering::Relaxed);
  now_ms.saturating_sub(previous_ms) >= interval_ms
    && last_log_ms
      .compare_exchange(previous_ms, now_ms, Ordering::Relaxed, Ordering::Relaxed)
      .is_ok()
}

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

pub(super) fn unsupported_av1_decode_error(codec: VideoCodecId, error: &str) -> bool {
  codec == VideoCodecId::Av1
    && error.contains("macOS VideoToolbox AV1 is unavailable")
    && error.contains("software AV1 decode is disabled")
}

pub(super) fn unsupported_av1_stream_error() -> VideoStreamError {
  VideoStreamError {
    title: String::new(),
    message: String::new(),
    i18n_key: Some("lobby.stream_error.unsupported_av1"),
  }
}

pub(super) fn native_decoder_unavailable_error(error: &str) -> bool {
  error.contains("native decoder is not wired")
    || error.contains("has no native decoder wired")
    || error.contains("refusing decoder fallback")
    || error.contains("decoder fallback is disabled")
}

pub(super) fn native_decoder_unavailable_stream_error(reason: String) -> VideoStreamError {
  VideoStreamError {
    title: String::new(),
    message: reason,
    i18n_key: Some("lobby.stream_error.decoder_unavailable"),
  }
}

#[cfg(test)]
#[path = "../../tests/unit/session/video.rs"]
mod tests;
