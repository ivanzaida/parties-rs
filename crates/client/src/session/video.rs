use std::{
  collections::{HashMap, HashSet, VecDeque},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread,
  time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};

use super::{LobbyConnectionWarningKind, VideoStreamError};
use crate::{
  network::{
    protocol::{
      UserId, VideoCodecId,
      data::{ForwardedVideoFrame, VideoControl},
    },
    server::{ReceivedVideoPacket, Server, ServerError},
  },
  services::{
    profiler,
    video::{DecodedVideoFrame, NativeVideoBackend, VideoDecodeConfig, VideoDecoder, VideoError},
  },
};

pub(super) const MAX_QUEUED_VIDEO_PACKETS: usize = 12;
pub(super) const LARGE_VIDEO_BATCH_LOG_THRESHOLD: usize = 3;
pub(super) const VIDEO_REVISION_INTERVAL: Duration = Duration::from_millis(16);
const KEYFRAME_REQUEST_RETRY_INTERVAL: Duration = Duration::from_millis(750);

#[cfg(target_os = "windows")]
pub(super) const SHARED_NV12_PLANES_SURFACE_CACHE_LIMIT: usize = 8;
#[cfg(target_os = "windows")]
pub(super) const DX12_DECODE_SURFACE_RING_SIZE: usize = 8;
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

#[cfg(target_os = "windows")]
pub(super) static DX12_NATIVE_STREAM_DECODE_SUPPORTED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
  if !ENABLE_DX12_NATIVE_STREAM_DECODE {
    return false;
  }

  match *WINDOWS_DEFAULT_DXGI_ADAPTER_VENDOR_ID {
    Some(WINDOWS_NVIDIA_VENDOR_ID) => {
      tracing::info!(target: "video::decode", "[video:decode] DX12 native stream decode enabled: default DXGI adapter is NVIDIA, NVDEC interop is allowed");
      true
    }
    Some(WINDOWS_AMD_VENDOR_ID) => {
      tracing::info!(target: "video::decode", "[video:decode] DX12 native stream decode enabled: default DXGI adapter is AMD, AMF shared NV12 planes interop is allowed");
      true
    }
    Some(vendor_id) => {
      tracing::warn!(target: "video::decode",
        "[video:decode] DX12 native stream decode disabled: default DXGI adapter vendor_id=0x{vendor_id:04x} is not NVIDIA or AMD"
      );
      false
    }
    None => {
      tracing::warn!(target: "video::decode", "[video:decode] DX12 native stream decode disabled: failed to resolve default DXGI adapter");
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
  packets: VecDeque<ForwardedVideoFrame>,
  dropped_senders: HashMap<UserId, u64>,
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

  pub(super) fn push(&self, packet: ForwardedVideoFrame) {
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

  pub(super) fn close(&self) {
    self.closed.store(true, Ordering::Relaxed);
    self.notify.notify_all();
  }

  pub(super) fn pop_batch_into(
    &self,
    stop: &AtomicBool,
    batch: &mut Vec<ForwardedVideoFrame>,
    dropped_senders: &mut HashMap<UserId, u64>,
  ) -> Option<u64> {
    let mut state = self.state.lock();
    while state.packets.is_empty() && !self.closed.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
      self.notify.wait_for(&mut state, Duration::from_millis(100));
    }

    if state.packets.is_empty() {
      return None;
    }

    batch.clear();
    batch.extend(state.packets.drain(..));
    dropped_senders.clear();
    dropped_senders.extend(state.dropped_senders.drain());
    let dropped = dropped_senders.values().sum();
    Some(dropped)
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
  tracing::info!(target: "video", "[video] receiver thread started");
  let queue = session.reset_video_packet_queue();
  let reader_thread = {
    let server = server.clone();
    let runtime = runtime.clone();
    let stop = stop.clone();
    let queue = queue.clone();
    let session = session.clone();
    thread::Builder::new()
      .name("parties-video-reader".to_owned())
      .spawn(move || {
        while !stop.load(Ordering::Relaxed) {
          match runtime.block_on(server.recv_video()) {
            Ok(ReceivedVideoPacket::Frame(packet)) => {
              session.mark_video_network_activity();
              queue.push(packet);
            }
            Ok(ReceivedVideoPacket::VideoControl(control)) => {
              session.mark_video_network_activity();
              session.handle_video_control_packet(control);
            }
            Err(ServerError::Protocol(error)) => {
              tracing::warn!(target: "video", "[video] ignored malformed video packet: {error}");
              continue;
            }
            Err(error) => {
              let error = error.to_string();
              tracing::warn!(
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
      .ok()
  };
  let datagram_reader_thread = {
    let server = server.clone();
    let runtime = runtime.clone();
    let stop = stop.clone();
    let queue = queue.clone();
    let session = session.clone();
    thread::Builder::new()
      .name("parties-video-datagram-reader".to_owned())
      .spawn(move || {
        while !stop.load(Ordering::Relaxed) {
          match runtime.block_on(server.recv_forwarded_video_datagram_until(stop.as_ref())) {
            Ok(Some(packet)) => {
              session.mark_video_network_activity();
              queue.push(packet);
            }
            Ok(None) => break,
            Err(ServerError::Protocol(error)) => {
              tracing::warn!(target: "video", "[video] ignored malformed video datagram: {error}");
              continue;
            }
            Err(error) => {
              let error = error.to_string();
              tracing::warn!(
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
      .ok()
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
  let mut last_watched_user = session.watching_user_id();
  let mut batch = Vec::<ForwardedVideoFrame>::with_capacity(MAX_QUEUED_VIDEO_PACKETS);

  while !stop.load(Ordering::Relaxed) {
    let Some(dropped_count) = ({
      let _span = profiler::span("video.receive.pop_batch");
      queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders)
    }) else {
      break;
    };
    let _batch_span = profiler::span("video.receive.process_batch");

    let watched_user = session.watching_user_id();
    if watched_user != last_watched_user {
      decode_pool.retain_watched(watched_user);
      awaiting_decoded_output.retain(|user_id| Some(*user_id) == watched_user);
      expected_frame_numbers.retain(|user_id, _| Some(*user_id) == watched_user);
      last_keyframe_requests.retain(|user_id, _| Some(*user_id) == watched_user);
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
            tracing::warn!(target: "video::decode", "[video:decode] failed to prewarm decoder for user {user_id}: {error}");
            if native_decoder_unavailable_error(&error) {
              session.set_video_error(user_id, native_decoder_unavailable_stream_error(error));
            }
          }
        }
      }
      last_watched_user = watched_user;
      tracing::debug!(target: "video", "[video] watch target changed: {watched_user:?}");
    }

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
          tracing::warn!(target: "video",
            "[video] watched stream packet dropped but no replacement packet was queued: user={user_id} dropped={}",
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
      tracing::warn!(target: "video",
        "[video] dropping stale video backlog: queued={} dropped={} users={}",
        batch.len(),
        dropped_count,
        affected_users.len()
      );
    }

    let last_batch_queued = batch.len();
    let latest_watched_packet_index = batch
      .iter()
      .enumerate()
      .filter(|(_, packet)| Some(packet.sender_id) == watched_user)
      .map(|(index, _)| index)
      .last();

    for (packet_index, packet) in batch.drain(..).enumerate() {
      if Some(packet.sender_id) != watched_user {
        decode_pool.remove_user(packet.sender_id);
        awaiting_keyframes.remove(&packet.sender_id);
        awaiting_decoded_output.remove(&packet.sender_id);
        expected_frame_numbers.remove(&packet.sender_id);
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

      if awaiting_keyframes.contains(&packet.sender_id) {
        if !packet.frame.keyframe {
          request_keyframe_if_due(
            &runtime,
            &server,
            &mut last_keyframe_requests,
            packet.sender_id,
            "still waiting for video keyframe",
          );
          continue;
        }
        awaiting_keyframes.remove(&packet.sender_id);
        last_keyframe_requests.remove(&packet.sender_id);
        decode_pool.clear_user_failures(packet.sender_id);
        tracing::info!(target: "video::decode",
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
            awaiting_decoded_output.remove(&packet.sender_id);
            expected_frame_numbers.remove(&packet.sender_id);
            decode_pool.reset_user(packet.sender_id);
            if awaiting_keyframes.insert(packet.sender_id) {
              request_keyframe_for(
                &runtime,
                &server,
                &mut last_keyframe_requests,
                packet.sender_id,
                "video frame gap detected",
              );
            }
            tracing::warn!(target: "video::decode",
              "[video:decode] video frame gap for user {}: expected={} actual={}; waiting for keyframe",
              packet.sender_id,
              expected_frame_number,
              packet.frame.frame_number
            );
            continue;
          }
          None => {
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
      let output =
        Some(packet_index) == latest_watched_packet_index || awaiting_decoded_output.contains(&packet.sender_id);
      let had_known_decoder_failure = decode_pool.has_decoder_failure(packet.sender_id, &packet_config);
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
      {
        let native_decode = try_present_windows_native_video_frame(
          &session,
          &mut decode_pool,
          &mut shared_nv12_planes_surfaces,
          &mut dx12_decode_surfaces,
          &mut decoded_counts,
          received_count,
          &packet,
          &packet_config,
        );
        match native_decode {
          WindowsNativeVideoDecode::Presented => {
            awaiting_decoded_output.remove(&packet.sender_id);
            continue;
          }
          WindowsNativeVideoDecode::Pending => {
            awaiting_decoded_output.insert(packet.sender_id);
            continue;
          }
          WindowsNativeVideoDecode::Fallback => {}
        }
      }

      let missing_initial_image =
        packet.frame.keyframe && !session.has_video_frame(packet.sender_id, packet.frame.width, packet.frame.height);
      let output = output || missing_initial_image;
      let output_buffer = if output {
        session.take_video_pixel_buffer(packet.sender_id, packet.frame.width, packet.frame.height)
      } else {
        None
      };
      let sender_id = packet.sender_id;
      let codec = packet.frame.codec;
      let decode_result = {
        let _span = profiler::span("video.decode.packet");
        decode_pool.decode_cpu(packet, &packet_config, output, output_buffer)
      };
      match decode_result {
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
          session.present_video_frame(frame);
        }
        Ok(None) => {
          if output {
            awaiting_decoded_output.insert(sender_id);
          }
          if !had_known_decoder_failure && should_log_video_count(received_count) {
            tracing::info!(target: "video::decode", "[video:decode] received frame produced no decoded output yet");
          }
        }
        Err(error) => {
          awaiting_decoded_output.remove(&sender_id);
          expected_frame_numbers.remove(&sender_id);
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
  drop(reader_thread);
  drop(datagram_reader_thread);
  tracing::info!(target: "video", "[video] receiver thread stopping");
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsNativeVideoDecode {
  Presented,
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
            packet.frame.codec,
            packet.frame.width,
            packet.frame.height,
            surface,
          );
          return WindowsNativeVideoDecode::Presented;
        }
        decode_pool.mark_shared_nv12_planes_failure(packet.sender_id, packet_config);
      }
      Ok(None) => {
        if should_log_video_count(received_count) {
          tracing::info!(target: "video::decode", "[video:decode] received frame produced no shared NV12 planes decoded output yet");
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
  ) && let Some(decoded) = decode_pool.decode_to_dx12(packet, packet_config, &surface)
  {
    if decoded {
      let decoded_count = increment_counter(decoded_counts, packet.sender_id);
      if should_log_video_count(decoded_count) {
        tracing::debug!(target: "video::decode",
          "[video:decode] decoded DX12 frame #{decoded_count} from user {}: codec={:?} size={}x{} format=Nv12",
          packet.sender_id,
          packet.frame.codec,
          packet.frame.width,
          packet.frame.height
        );
      }
      session.present_dx12_video_frame(
        packet.sender_id,
        packet.frame.codec,
        packet.frame.width,
        packet.frame.height,
        surface,
      );
      WindowsNativeVideoDecode::Presented
    } else {
      if should_log_video_count(received_count) {
        tracing::info!(target: "video::decode", "[video:decode] received frame produced no DX12 decoded output yet");
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

fn request_keyframe_if_due(
  runtime: &tokio::runtime::Handle,
  server: &Arc<Server>,
  last_keyframe_requests: &mut HashMap<UserId, Instant>,
  user_id: UserId,
  reason: &str,
) {
  let now = Instant::now();
  if last_keyframe_requests
    .get(&user_id)
    .is_some_and(|last| now.duration_since(*last) < KEYFRAME_REQUEST_RETRY_INTERVAL)
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
      tracing::debug!(target: "video", "[video] stream view refreshed for keyframe recovery: user={user_id} reason={reason}");
    }
    Err(error) => {
      tracing::warn!(target: "video", "[video] stream view refresh failed during keyframe recovery: user={user_id} reason={reason} error={error}");
    }
  }
  match runtime.block_on(server.request_keyframe_stream(user_id)) {
    Ok(()) => {
      tracing::info!(target: "video", "[video] keyframe requested for user {user_id}: reason={reason}");
    }
    Err(stream_error) => {
      tracing::warn!(target: "video", "[video] stream keyframe request failed for user {user_id}: reason={reason} error={stream_error}; trying datagram");
      match server.request_keyframe(user_id) {
        Ok(()) => {
          tracing::info!(target: "video", "[video] datagram keyframe requested for user {user_id}: reason={reason}")
        }
        Err(datagram_error) => {
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
    let decoder = VideoDecoder::start(config.clone())?;
    let backend = decoder.backend();
    tracing::info!(target: "video::decode",
      "[video:decode] decoder backend prewarmed for user {user_id}: backend={} codec={:?} size={}x{}",
      native_video_backend_label(backend),
      config.codec,
      config.width,
      config.height
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
      match VideoDecoder::start(config.clone()) {
        Ok(decoder) => {
          let backend = decoder.backend();
          tracing::info!(target: "video::decode",
            "[video:decode] decoder backend selected for user {}: backend={} codec={:?} size={}x{}",
            packet.sender_id,
            native_video_backend_label(backend),
            config.codec,
            config.width,
            config.height
          );
          self.decoders.insert(packet.sender_id, decoder);
        }
        Err(error) => {
          tracing::warn!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
          self.decoder_failures.insert(failure_key);
          return Err(error.to_string());
        }
      }
    }

    let Some(decoder) = self.decoders.get_mut(&packet.sender_id) else {
      return Ok(None);
    };
    match decoder.decode_with_output_buffer(&packet, output, output_buffer) {
      Ok(frame) => Ok(frame),
      Err(error) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to decode frame from user {}: {error}", packet.sender_id);
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
  ) -> Option<bool> {
    let failure_key = (packet.sender_id, config.to_owned());

    if self.dx12_failures.contains(&failure_key) {
      return None;
    }
    if self.decoder_failures.contains(&failure_key) {
      return Some(false);
    }

    let decoder_needs_start = self
      .decoders
      .get(&packet.sender_id)
      .is_none_or(|decoder| decoder.config() != config);
    if decoder_needs_start {
      self.decoders.remove(&packet.sender_id);
      match VideoDecoder::start(config.clone()) {
        Ok(decoder) => {
          let backend = decoder.backend();
          tracing::info!(target: "video::decode",
            "[video:decode] decoder backend selected for user {}: backend={} codec={:?} size={}x{} dx12_prepath=true",
            packet.sender_id,
            native_video_backend_label(backend),
            config.codec,
            config.width,
            config.height
          );
          self.decoders.insert(packet.sender_id, decoder);
        }
        Err(error) => {
          tracing::warn!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
          self.decoder_failures.insert(failure_key);
          return Some(false);
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
        tracing::warn!(target: "video::decode",
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
      match VideoDecoder::start(config.clone()) {
        Ok(decoder) => {
          let backend = decoder.backend();
          tracing::info!(target: "video::decode",
            "[video:decode] decoder backend selected for user {}: backend={} codec={:?} size={}x{} shared_nv12_planes_prepath={}",
            packet.sender_id,
            native_video_backend_label(backend),
            config.codec,
            config.width,
            config.height,
            backend == NativeVideoBackend::AmdAmf
          );
          self.decoders.insert(packet.sender_id, decoder);
        }
        Err(error) => {
          tracing::warn!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
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
      tracing::warn!(
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
