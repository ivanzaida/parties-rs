use std::{
  collections::{HashMap, HashSet, VecDeque},
  sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  thread,
  time::{Duration, Instant},
};

use lurq::{
  app::component::{ComponentInfo, DevtoolsInspectable},
  core::Signal,
  images::{ImageData, StreamingImage},
};

use crate::{
  network::{
    protocol::{
      ChannelId, Role, S2C, UserId,
      control::{
        ChannelInfo, ChannelUser as ProtocolChannelUser, ChatMessage as ProtocolChatMessage, ScreenShareMetadata,
        TextChannelInfo,
      },
      data::{ForwardedStreamAudioPacket, ForwardedVideoFrame, VideoControl, VideoFrame},
    },
    server::{ReceivedAudioPacket, Server, ServerError},
  },
  services::{
    notifications::{self, NotificationAudioSettings, NotificationSound},
    profiler,
    video::{
      DecodedVideoFrame, DecodedVideoPixelFormat, NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig,
      VideoDecodeConfig, VideoDecoder, VideoFrameLoopback,
    },
    voice::{LocalVoiceCallback, VoiceEngine},
  },
  storage::AppSettings,
};

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_USER_VOLUME: i32 = 100;
const MAX_QUEUED_VIDEO_PACKETS: usize = 48;
const LARGE_VIDEO_BATCH_LOG_THRESHOLD: usize = 12;
const VIDEO_REVISION_INTERVAL: Duration = Duration::from_millis(16);
#[cfg(target_os = "windows")]
const ENABLE_DX12_NATIVE_STREAM_DECODE: bool = true;
#[cfg(target_os = "windows")]
const SHARED_NV12_PLANES_SURFACE_CACHE_LIMIT: usize = 8;
#[cfg(target_os = "windows")]
const WINDOWS_NVIDIA_VENDOR_ID: u32 = 0x10DE;
#[cfg(target_os = "windows")]
const WINDOWS_AMD_VENDOR_ID: u32 = 0x1002;

#[cfg(target_os = "windows")]
static DX12_NATIVE_STREAM_DECODE_SUPPORTED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
  if !ENABLE_DX12_NATIVE_STREAM_DECODE {
    return false;
  }

  match windows_default_dxgi_adapter_vendor_id() {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedServerInfo {
  pub address: String,
  pub server_name: String,
  pub display_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub certificate_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoStreamError {
  pub title: String,
  pub message: String,
  pub i18n_key: Option<&'static str>,
}

impl DevtoolsInspectable for ConnectedServerInfo {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "address",
      std::any::type_name::<String>(),
      self.address.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.server_name.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "display_name",
      std::any::type_name::<String>(),
      self.display_name.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "role",
      std::any::type_name::<Role>(),
      format!("{:?}", self.role),
    ));
    buffer.push(ComponentInfo::with_value(
      "certificate_fingerprint",
      std::any::type_name::<String>(),
      self.certificate_fingerprint.clone(),
    ));
  }
}

fn native_video_backend_label(backend: NativeVideoBackend) -> &'static str {
  match backend {
    NativeVideoBackend::NvidiaNvenc => "NVIDIA NVENC",
    NativeVideoBackend::NvidiaNvdec => "NVIDIA NVDEC",
    NativeVideoBackend::AmdAmf => "AMD AMF",
    NativeVideoBackend::WindowsMediaFoundation => "Windows Media Foundation",
    NativeVideoBackend::OpenH264 => "OpenH264",
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

fn decode_video_packet(
  decoders: &mut HashMap<UserId, VideoDecoder>,
  decoder_failures: &mut HashSet<(UserId, VideoDecodeConfig)>,
  packet: ForwardedVideoFrame,
  output: bool,
  output_buffer: Option<Vec<u8>>,
) -> Result<Option<DecodedVideoFrame>, String> {
  let config = VideoDecodeConfig {
    codec: packet.frame.codec,
    width: packet.frame.width,
    height: packet.frame.height,
  };
  let failure_key = (packet.sender_id, config.clone());

  if decoder_failures.contains(&failure_key) {
    return Ok(None);
  }

  if decoders
    .get(&packet.sender_id)
    .is_none_or(|decoder| decoder.config() != &config)
  {
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
        decoders.insert(packet.sender_id, decoder);
      }
      Err(error) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
        decoder_failures.insert(failure_key);
        return Err(error.to_string());
      }
    }
  }

  let Some(decoder) = decoders.get_mut(&packet.sender_id) else {
    return Ok(None);
  };
  match decoder.decode_with_output_buffer(&packet, output, output_buffer) {
    Ok(frame) => Ok(frame),
    Err(error) => {
      tracing::warn!(target: "video::decode", "[video:decode] failed to decode frame from user {}: {error}", packet.sender_id);
      decoders.remove(&packet.sender_id);
      decoder_failures.insert(failure_key);
      Err(error.to_string())
    }
  }
}

fn unsupported_av1_decode_error(codec: crate::network::protocol::VideoCodecId, error: &str) -> bool {
  codec == crate::network::protocol::VideoCodecId::Av1
    && error.contains("macOS VideoToolbox AV1 is unavailable")
    && error.contains("software AV1 is disabled")
}

fn unsupported_av1_stream_error() -> VideoStreamError {
  VideoStreamError {
    title: String::new(),
    message: String::new(),
    i18n_key: Some("lobby.stream_error.unsupported_av1"),
  }
}

fn native_decoder_unavailable_error(error: &str) -> bool {
  error.contains("native decoder is not wired")
    || error.contains("has no native decoder wired")
    || error.contains("refusing decoder fallback")
    || error.contains("decoder fallback is disabled")
}

fn native_decoder_unavailable_stream_error(reason: String) -> VideoStreamError {
  VideoStreamError {
    title: String::new(),
    message: reason,
    i18n_key: Some("lobby.stream_error.decoder_unavailable"),
  }
}

fn video_decode_failure_key(user_id: UserId, frame: &VideoFrame) -> (UserId, VideoDecodeConfig) {
  (
    user_id,
    VideoDecodeConfig {
      codec: frame.codec,
      width: frame.width,
      height: frame.height,
    },
  )
}

#[cfg(target_os = "windows")]
fn decode_video_packet_to_dx12(
  decoders: &mut HashMap<UserId, VideoDecoder>,
  decoder_failures: &mut HashSet<(UserId, VideoDecodeConfig)>,
  dx12_failures: &mut HashSet<(UserId, VideoDecodeConfig)>,
  packet: &ForwardedVideoFrame,
  surface: &lurq::app::dx12_render::Dx12Nv12Surface,
) -> Option<bool> {
  let config = VideoDecodeConfig {
    codec: packet.frame.codec,
    width: packet.frame.width,
    height: packet.frame.height,
  };
  let failure_key = (packet.sender_id, config.clone());

  if dx12_failures.contains(&failure_key) {
    return None;
  }
  if decoder_failures.contains(&failure_key) {
    return Some(false);
  }

  if decoders
    .get(&packet.sender_id)
    .is_none_or(|decoder| decoder.config() != &config)
  {
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
        decoders.insert(packet.sender_id, decoder);
      }
      Err(error) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
        decoder_failures.insert(failure_key);
        return Some(false);
      }
    }
  }

  let decoder = decoders.get_mut(&packet.sender_id)?;
  if decoder.backend() != crate::services::video::NativeVideoBackend::NvidiaNvdec {
    return None;
  }

  match decoder.decode_to_dx12_surface(packet, surface) {
    Ok(decoded) => Some(decoded),
    Err(error) => {
      tracing::warn!(target: "video::decode",
        "[video:decode] failed to decode frame from user {} into DX12 surface: {error}",
        packet.sender_id
      );
      decoders.remove(&packet.sender_id);
      dx12_failures.insert(failure_key);
      None
    }
  }
}

#[cfg(target_os = "windows")]
fn decode_video_packet_to_shared_nv12_planes(
  decoders: &mut HashMap<UserId, VideoDecoder>,
  decoder_failures: &mut HashSet<(UserId, VideoDecodeConfig)>,
  shared_nv12_planes_failures: &mut HashSet<(UserId, VideoDecodeConfig)>,
  packet: &ForwardedVideoFrame,
) -> Option<Result<Option<(usize, usize)>, String>> {
  if !*DX12_NATIVE_STREAM_DECODE_SUPPORTED {
    return None;
  }

  let config = VideoDecodeConfig {
    codec: packet.frame.codec,
    width: packet.frame.width,
    height: packet.frame.height,
  };
  let failure_key = (packet.sender_id, config.clone());
  if shared_nv12_planes_failures.contains(&failure_key) {
    return None;
  }
  if decoder_failures.contains(&failure_key) {
    return None;
  }

  if decoders
    .get(&packet.sender_id)
    .is_none_or(|decoder| decoder.config() != &config)
  {
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
          backend == crate::services::video::NativeVideoBackend::AmdAmf
        );
        decoders.insert(packet.sender_id, decoder);
      }
      Err(error) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to start decoder for user {}: {error}", packet.sender_id);
        decoder_failures.insert(failure_key);
        return Some(Err(error.to_string()));
      }
    }
  }

  let decoder = decoders.get_mut(&packet.sender_id)?;
  if decoder.backend() != crate::services::video::NativeVideoBackend::AmdAmf {
    return None;
  }

  match decoder.decode_to_shared_nv12_planes(packet) {
    Ok(handles) => Some(Ok(handles)),
    Err(error) => {
      tracing::warn!(target: "video::decode",
        "[video:decode] failed to decode frame from user {} into shared NV12 plane textures: {error}",
        packet.sender_id
      );
      decoders.remove(&packet.sender_id);
      shared_nv12_planes_failures.insert(failure_key);
      Some(Err(error.to_string()))
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

fn should_log_audio_count(count: u64) -> bool {
  count == 1 || count % 100 == 0
}

struct VideoPacketQueue {
  packets: Mutex<VecDeque<ForwardedVideoFrame>>,
  notify: Condvar,
  dropped: AtomicU64,
  closed: AtomicBool,
}

impl VideoPacketQueue {
  fn new() -> Self {
    Self {
      packets: Mutex::new(VecDeque::new()),
      notify: Condvar::new(),
      dropped: AtomicU64::new(0),
      closed: AtomicBool::new(false),
    }
  }

  fn push(&self, packet: ForwardedVideoFrame) {
    if self.closed.load(Ordering::Relaxed) {
      return;
    }
    {
      let mut packets = self.packets.lock().expect("video packet queue lock poisoned");
      if self.closed.load(Ordering::Relaxed) {
        return;
      }
      if packets.len() >= MAX_QUEUED_VIDEO_PACKETS {
        packets.pop_front();
        self.dropped.fetch_add(1, Ordering::Relaxed);
      }
      packets.push_back(packet);
    }
    self.notify.notify_one();
  }

  fn pop_batch_into(&self, stop: &AtomicBool, batch: &mut Vec<ForwardedVideoFrame>) -> Option<u64> {
    let mut packets = self.packets.lock().expect("video packet queue lock poisoned");
    while packets.is_empty() && !stop.load(Ordering::Relaxed) && !self.closed.load(Ordering::Relaxed) {
      let (guard, _) = self
        .notify
        .wait_timeout(packets, Duration::from_millis(100))
        .expect("video packet queue lock poisoned");
      packets = guard;
    }

    if packets.is_empty() {
      return None;
    }

    batch.clear();
    batch.extend(packets.drain(..));
    let dropped = self.dropped.swap(0, Ordering::Relaxed);
    Some(dropped)
  }

  fn close(&self) {
    self.closed.store(true, Ordering::Relaxed);
    self.notify.notify_all();
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TofuWarning {
  pub address: String,
  pub server_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub saved_fingerprint: String,
  pub received_fingerprint: String,
  pub server_password: String,
  pub display_name: String,
}

impl DevtoolsInspectable for TofuWarning {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "address",
      std::any::type_name::<String>(),
      self.address.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.server_name.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "role",
      std::any::type_name::<Role>(),
      format!("{:?}", self.role),
    ));
    buffer.push(ComponentInfo::with_value(
      "saved_fingerprint",
      std::any::type_name::<String>(),
      self.saved_fingerprint.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "received_fingerprint",
      std::any::type_name::<String>(),
      self.received_fingerprint.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "server_password",
      std::any::type_name::<String>(),
      if self.server_password.is_empty() {
        String::new()
      } else {
        "<stored>".to_owned()
      },
    ));
    buffer.push(ComponentInfo::with_value(
      "display_name",
      std::any::type_name::<String>(),
      self.display_name.clone(),
    ));
  }
}

#[allow(dead_code)]
pub struct ConnectedServer {
  pub info: ConnectedServerInfo,
  pub server: Arc<Server>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyChannel {
  pub id: ChannelId,
  pub name: String,
  pub max_users: u32,
  pub sort_order: u32,
  pub user_count: u32,
  pub key_received: bool,
}

impl From<ChannelInfo> for LobbyChannel {
  fn from(channel: ChannelInfo) -> Self {
    Self {
      id: channel.id,
      name: channel.name,
      max_users: channel.max_users,
      sort_order: channel.sort_order,
      user_count: channel.user_count,
      key_received: false,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyTextChannel {
  pub id: ChannelId,
  pub name: String,
  pub sort_order: u32,
}

impl From<TextChannelInfo> for LobbyTextChannel {
  fn from(channel: TextChannelInfo) -> Self {
    Self {
      id: channel.id,
      name: channel.name,
      sort_order: channel.sort_order,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyUser {
  pub user_id: UserId,
  pub username: String,
  pub role: Role,
  pub muted: bool,
  pub deafened: bool,
  pub speaking: bool,
}

impl From<ProtocolChannelUser> for LobbyUser {
  fn from(user: ProtocolChannelUser) -> Self {
    Self {
      user_id: user.user_id,
      username: user.username,
      role: user.role,
      muted: user.muted,
      deafened: user.deafened,
      speaking: false,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyScreenShare {
  pub sharer_user_id: UserId,
  pub metadata: ScreenShareMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LobbyState {
  pub channels: Vec<LobbyChannel>,
  pub selected_channel_id: Option<ChannelId>,
  pub stream_browser_channel_id: Option<ChannelId>,
  pub text_channels: Vec<LobbyTextChannel>,
  pub selected_text_channel_id: Option<ChannelId>,
  pub chat_messages_by_channel: HashMap<ChannelId, Vec<ProtocolChatMessage>>,
  pub unread_text_channel_ids: HashSet<ChannelId>,
  pub chat_history_loading: HashSet<ChannelId>,
  pub chat_history_has_more: HashMap<ChannelId, bool>,
  pub users: Vec<LobbyUser>,
  pub users_by_channel: HashMap<ChannelId, Vec<LobbyUser>>,
  pub screen_shares: Vec<LobbyScreenShare>,
  pub watching_user_id: Option<UserId>,
  pub receiver_running: bool,
  pub channel_list_received: bool,
  pub keepalive_ok: bool,
  pub ping_ms: Option<u32>,
  pub disconnected: bool,
  pub last_error: Option<String>,
}

#[allow(dead_code)]
enum VideoFrameImage {
  Cpu(StreamingImage),
  #[cfg(target_os = "macos")]
  MacosNative(ImageData),
  #[cfg(target_os = "windows")]
  Dx12Surface(Arc<lurq::app::dx12_render::Dx12Nv12Surface>),
}

impl VideoFrameImage {
  fn is_cpu_image(&self) -> bool {
    matches!(self, Self::Cpu(_))
  }

  fn image_data(&self) -> ImageData {
    match self {
      Self::Cpu(image) => image.image_data(),
      #[cfg(target_os = "macos")]
      Self::MacosNative(image) => image.clone(),
      #[cfg(target_os = "windows")]
      Self::Dx12Surface(surface) => surface.image_data(),
    }
  }

  fn set_cpu_pixels(&self, format: DecodedVideoPixelFormat, pixels: Vec<u8>) -> bool {
    match self {
      Self::Cpu(image) => {
        match format {
          DecodedVideoPixelFormat::Rgba8 => image.set_rgba(pixels),
          DecodedVideoPixelFormat::Nv12 => image.set_nv12(pixels),
        }
        true
      }
      #[cfg(target_os = "macos")]
      Self::MacosNative(_) => false,
      #[cfg(target_os = "windows")]
      Self::Dx12Surface(_) => false,
    }
  }

  fn take_cpu_buffer(&self) -> Option<Vec<u8>> {
    match self {
      Self::Cpu(image) => match image.image_data().format() {
        lurq::images::ImagePixelFormat::Rgba8 => image.take_rgba_buffer(),
        lurq::images::ImagePixelFormat::Nv12 => image.take_nv12_buffer(),
      },
      #[cfg(target_os = "macos")]
      Self::MacosNative(_) => None,
      #[cfg(target_os = "windows")]
      Self::Dx12Surface(_) => None,
    }
  }
}

#[derive(Clone)]
pub struct ServerSession {
  current: Arc<Mutex<Option<ConnectedServer>>>,
  tofu_warning: Arc<Mutex<Option<TofuWarning>>>,
  lobby: Arc<Mutex<LobbyState>>,
  receiver_started: Arc<Mutex<bool>>,
  receiver_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
  local_voice_fallback: Arc<Mutex<(bool, bool)>>,
  muted_before_deafen: Arc<Mutex<Option<bool>>>,
  speaking_marks: Arc<Mutex<HashMap<UserId, u64>>>,
  speaking_mark_counter: Arc<Mutex<u64>>,
  speaking_clear_scheduled: Arc<Mutex<HashSet<UserId>>>,
  pending_keepalive_ping: Arc<Mutex<Option<Instant>>>,
  voice_engine: Arc<Mutex<Option<VoiceEngine>>>,
  video_broadcast: Arc<Mutex<Option<VideoBroadcast>>>,
  video_packet_queue: Arc<Mutex<Arc<VideoPacketQueue>>>,
  video_frames: Arc<Mutex<HashMap<UserId, VideoFrameImage>>>,
  video_errors: Arc<Mutex<HashMap<UserId, VideoStreamError>>>,
  #[cfg(target_os = "windows")]
  dx12_video_surfaces: Option<lurq::app::dx12_render::Dx12VideoSurfaceAllocator>,
  video_revision_marks: Arc<Mutex<HashMap<UserId, Instant>>>,
  voice_audio_counts: Arc<Mutex<HashMap<UserId, u64>>>,
  stream_audio_counts: Arc<Mutex<HashMap<UserId, u64>>>,
  user_volumes: Arc<Mutex<HashMap<UserId, i32>>>,
  stream_volumes: Arc<Mutex<HashMap<UserId, i32>>>,
  notification_audio_settings: Arc<Mutex<NotificationAudioSettings>>,
  pending_reconnect_watch_user_id: Arc<Mutex<Option<UserId>>>,
  shutdown_requested: Arc<AtomicBool>,
  revision: Signal<u64>,
}

impl Default for ServerSession {
  fn default() -> Self {
    Self {
      current: Arc::new(Mutex::new(None)),
      tofu_warning: Arc::new(Mutex::new(None)),
      lobby: Arc::new(Mutex::new(LobbyState::default())),
      receiver_started: Arc::new(Mutex::new(false)),
      receiver_stop: Arc::new(Mutex::new(None)),
      local_voice_fallback: Arc::new(Mutex::new((false, false))),
      muted_before_deafen: Arc::new(Mutex::new(None)),
      speaking_marks: Arc::new(Mutex::new(HashMap::new())),
      speaking_mark_counter: Arc::new(Mutex::new(0)),
      speaking_clear_scheduled: Arc::new(Mutex::new(HashSet::new())),
      pending_keepalive_ping: Arc::new(Mutex::new(None)),
      voice_engine: Arc::new(Mutex::new(None)),
      video_broadcast: Arc::new(Mutex::new(None)),
      video_packet_queue: Arc::new(Mutex::new(Arc::new(VideoPacketQueue::new()))),
      video_frames: Arc::new(Mutex::new(HashMap::new())),
      video_errors: Arc::new(Mutex::new(HashMap::new())),
      #[cfg(target_os = "windows")]
      dx12_video_surfaces: None,
      video_revision_marks: Arc::new(Mutex::new(HashMap::new())),
      voice_audio_counts: Arc::new(Mutex::new(HashMap::new())),
      stream_audio_counts: Arc::new(Mutex::new(HashMap::new())),
      user_volumes: Arc::new(Mutex::new(HashMap::new())),
      stream_volumes: Arc::new(Mutex::new(HashMap::new())),
      notification_audio_settings: Arc::new(Mutex::new(NotificationAudioSettings::default())),
      pending_reconnect_watch_user_id: Arc::new(Mutex::new(None)),
      shutdown_requested: Arc::new(AtomicBool::new(false)),
      revision: Signal::new(0),
    }
  }
}

#[allow(dead_code)]
impl ServerSession {
  #[cfg(target_os = "windows")]
  pub fn with_dx12_video_surface_allocator(
    dx12_video_surfaces: lurq::app::dx12_render::Dx12VideoSurfaceAllocator,
  ) -> Self {
    Self {
      dx12_video_surfaces: Some(dx12_video_surfaces),
      ..Self::default()
    }
  }

  #[cfg(target_os = "windows")]
  fn dx12_video_surface_allocator(&self) -> Option<lurq::app::dx12_render::Dx12VideoSurfaceAllocator> {
    self.dx12_video_surfaces.clone()
  }

  #[cfg(not(target_os = "windows"))]
  fn dx12_video_surface_allocator(&self) -> Option<()> {
    None
  }

  fn reset_video_packet_queue(&self) -> Arc<VideoPacketQueue> {
    let queue = Arc::new(VideoPacketQueue::new());
    *self.video_packet_queue.lock().expect("server session lock poisoned") = queue.clone();
    queue
  }

  fn current_video_packet_queue(&self) -> Arc<VideoPacketQueue> {
    self
      .video_packet_queue
      .lock()
      .expect("server session lock poisoned")
      .clone()
  }

  fn push_local_video_frame(&self, sender_id: UserId, frame: VideoFrame) {
    if self.watching_user_id() != Some(sender_id) {
      return;
    }
    self
      .current_video_packet_queue()
      .push(ForwardedVideoFrame { sender_id, frame });
  }

  fn local_video_loopback(&self, sender_id: UserId) -> VideoFrameLoopback {
    let session = self.clone();
    Arc::new(move |frame| {
      session.push_local_video_frame(sender_id, frame);
    })
  }

  pub fn set_connected(&self, connected: ConnectedServer) {
    tracing::info!(target: "session",
      "[session] connected: server='{}' address={} local_user={} display='{}' role={:?}",
      connected.info.server_name,
      connected.info.address,
      connected.info.user_id,
      connected.info.display_name,
      connected.info.role
    );
    self.shutdown_requested.store(false, Ordering::Relaxed);
    self.stop_lobby_receivers();
    self.stop_voice();
    self.stop_video_broadcast();
    *self.current.lock().expect("server session lock poisoned") = Some(connected);
    *self.lobby.lock().expect("server session lock poisoned") = LobbyState::default();
    *self.receiver_started.lock().expect("server session lock poisoned") = false;
    *self.local_voice_fallback.lock().expect("server session lock poisoned") = (false, false);
    *self.muted_before_deafen.lock().expect("server session lock poisoned") = None;
    self
      .speaking_marks
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.video_frames.lock().expect("server session lock poisoned").clear();
    self.video_errors.lock().expect("server session lock poisoned").clear();
    self
      .video_revision_marks
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self
      .voice_audio_counts
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self
      .stream_audio_counts
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.user_volumes.lock().expect("server session lock poisoned").clear();
    self
      .stream_volumes
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.bump_revision();
  }

  pub fn clear(&self) {
    if let Some(info) = self.info() {
      tracing::info!(target: "session",
        "[session] clearing connected session: server='{}' address={} local_user={}",
        info.server_name,
        info.address,
        info.user_id
      );
    }
    self.shutdown_requested.store(false, Ordering::Relaxed);
    self.stop_lobby_receivers();
    self.stop_voice();
    self.stop_video_broadcast();
    *self.current.lock().expect("server session lock poisoned") = None;
    self.clear_tofu_warning();
    *self.lobby.lock().expect("server session lock poisoned") = LobbyState::default();
    *self.receiver_started.lock().expect("server session lock poisoned") = false;
    *self.local_voice_fallback.lock().expect("server session lock poisoned") = (false, false);
    *self.muted_before_deafen.lock().expect("server session lock poisoned") = None;
    self
      .speaking_marks
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.video_frames.lock().expect("server session lock poisoned").clear();
    self.video_errors.lock().expect("server session lock poisoned").clear();
    self
      .video_revision_marks
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self
      .voice_audio_counts
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self
      .stream_audio_counts
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.user_volumes.lock().expect("server session lock poisoned").clear();
    self
      .stream_volumes
      .lock()
      .expect("server session lock poisoned")
      .clear();
    *self
      .pending_reconnect_watch_user_id
      .lock()
      .expect("server session lock poisoned") = None;
    self.bump_revision();
  }

  pub fn disconnect(&self) {
    tracing::info!(target: "session", "[session] disconnect requested by client");
    let was_in_voice = self
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .selected_channel_id
      .is_some();
    if let Some(server) = self.server() {
      server.disconnect();
    }
    self.clear();
    if was_in_voice {
      self.play_voice_leave_notification();
    }
  }

  pub fn disconnect_for_shutdown(&self) {
    tracing::info!(target: "session", "[session] disconnect requested for shutdown");
    self.shutdown_requested.store(true, Ordering::Relaxed);
    self.stop_lobby_receivers();
    self.stop_voice();
    self.stop_video_broadcast();
    if let Some(server) = self.server() {
      server.disconnect();
    }
  }

  pub fn shutdown_requested(&self) -> bool {
    self.shutdown_requested.load(Ordering::Relaxed)
  }

  pub fn info(&self) -> Option<ConnectedServerInfo> {
    self
      .current
      .lock()
      .expect("server session lock poisoned")
      .as_ref()
      .map(|connected| connected.info.clone())
  }

  pub fn server(&self) -> Option<Arc<Server>> {
    self
      .current
      .lock()
      .expect("server session lock poisoned")
      .as_ref()
      .map(|connected| connected.server.clone())
  }

  fn stop_lobby_receivers(&self) {
    if let Some(stop) = self
      .receiver_stop
      .lock()
      .expect("server session lock poisoned")
      .as_ref()
    {
      stop.store(true, Ordering::Relaxed);
    }
  }

  pub fn video_frame(&self, user_id: UserId) -> Option<ImageData> {
    self
      .video_frames
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .map(VideoFrameImage::image_data)
  }

  pub fn video_error(&self, user_id: UserId) -> Option<VideoStreamError> {
    self
      .video_errors
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .cloned()
  }

  pub fn local_voice_state(&self) -> Option<(bool, bool)> {
    self.info()?;
    Some(*self.local_voice_fallback.lock().expect("server session lock poisoned"))
  }

  pub fn set_local_voice_state(&self, muted: bool, deafened: bool) {
    *self.local_voice_fallback.lock().expect("server session lock poisoned") = (muted, deafened);
    if let Some(engine) = self.voice_engine.lock().expect("server session lock poisoned").as_ref() {
      engine.set_voice_state(muted, deafened);
    }

    let Some(user_id) = self.info().map(|info| info.user_id) else {
      self.bump_revision();
      return;
    };

    if muted || deafened {
      self.clear_user_speaking(user_id);
    }

    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      for user in &mut lobby.users {
        if user.user_id == user_id {
          user.muted = muted;
          user.deafened = deafened;
        }
      }
      for users in lobby.users_by_channel.values_mut() {
        for user in users {
          if user.user_id == user_id {
            user.muted = muted;
            user.deafened = deafened;
          }
        }
      }
    }

    self.bump_revision();
  }

  pub fn set_voice_activation_threshold(&self, value: i32) {
    if let Some(engine) = self.voice_engine.lock().expect("server session lock poisoned").as_ref() {
      engine.set_voice_activation_threshold(value);
    }
  }

  pub fn set_push_to_talk_release_delay_ms(&self, value: i32) {
    if let Some(engine) = self.voice_engine.lock().expect("server session lock poisoned").as_ref() {
      engine.set_push_to_talk_release_delay_ms(value);
    }
  }

  pub fn set_notification_audio_settings(&self, settings: &AppSettings) {
    *self
      .notification_audio_settings
      .lock()
      .expect("server session lock poisoned") = NotificationAudioSettings::from_app_settings(settings);
  }

  fn play_notification_sound(&self, sound: NotificationSound) {
    let settings = self
      .notification_audio_settings
      .lock()
      .expect("server session lock poisoned")
      .clone();
    notifications::play(sound, settings);
  }

  pub fn play_voice_join_notification(&self) {
    self.play_notification_sound(NotificationSound::VoiceJoin);
  }

  pub fn play_voice_leave_notification(&self) {
    self.play_notification_sound(NotificationSound::VoiceLeave);
  }

  pub fn play_local_voice_state_change_notification(&self) {
    self.play_notification_sound(NotificationSound::ModerationAction);
  }

  pub fn set_push_to_talk_active(&self, active: bool) {
    let release_delay_ms = {
      let voice_engine = self.voice_engine.lock().expect("server session lock poisoned");
      if let Some(engine) = voice_engine.as_ref() {
        engine.set_push_to_talk_active(active);
        engine.push_to_talk_release_delay_ms()
      } else {
        0
      }
    };

    let Some(user_id) = self.info().map(|info| info.user_id) else {
      return;
    };
    let (muted, deafened) = self.local_voice_state().unwrap_or((false, false));
    if active && !muted && !deafened {
      self.set_user_speaking(user_id, true);
    } else if !active && release_delay_ms == 0 {
      self.clear_user_speaking(user_id);
    }
  }

  pub fn user_volume(&self, user_id: UserId) -> i32 {
    self
      .user_volumes
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .copied()
      .unwrap_or(DEFAULT_USER_VOLUME)
  }

  pub fn set_user_volume(&self, user_id: UserId, volume: i32) {
    let volume = volume.clamp(0, 100);
    {
      let mut user_volumes = self.user_volumes.lock().expect("server session lock poisoned");
      if volume == DEFAULT_USER_VOLUME {
        user_volumes.remove(&user_id);
      } else {
        user_volumes.insert(user_id, volume);
      }
    }
    if let Some(engine) = self.voice_engine.lock().expect("server session lock poisoned").as_ref() {
      engine.set_user_volume(user_id, volume);
    }
  }

  pub fn stream_volume(&self, user_id: UserId) -> i32 {
    self
      .stream_volumes
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .copied()
      .unwrap_or(DEFAULT_USER_VOLUME)
  }

  pub fn set_stream_volume(&self, user_id: UserId, volume: i32) {
    let volume = volume.clamp(0, 100);
    {
      let mut stream_volumes = self.stream_volumes.lock().expect("server session lock poisoned");
      if volume == DEFAULT_USER_VOLUME {
        stream_volumes.remove(&user_id);
      } else {
        stream_volumes.insert(user_id, volume);
      }
    }
    if let Some(engine) = self.voice_engine.lock().expect("server session lock poisoned").as_ref() {
      engine.set_stream_volume(user_id, volume);
    }
  }

  pub fn remember_muted_before_deafen(&self, muted: bool) {
    *self.muted_before_deafen.lock().expect("server session lock poisoned") = Some(muted);
  }

  pub fn take_muted_before_deafen(&self) -> Option<bool> {
    self
      .muted_before_deafen
      .lock()
      .expect("server session lock poisoned")
      .take()
  }

  pub fn mark_user_speaking(&self, user_id: UserId) {
    let mark = {
      let mut counter = self.speaking_mark_counter.lock().expect("server session lock poisoned");
      *counter = counter.wrapping_add(1);
      let mark = *counter;
      let mut marks = self.speaking_marks.lock().expect("server session lock poisoned");
      marks.insert(user_id, mark);
      mark
    };

    self.set_user_speaking(user_id, true);

    let should_schedule = self
      .speaking_clear_scheduled
      .lock()
      .expect("server session lock poisoned")
      .insert(user_id);
    if should_schedule {
      let session = self.clone();
      thread::spawn(move || {
        session.clear_user_speaking_after_idle(user_id, mark);
      });
    }
  }

  fn clear_user_speaking(&self, user_id: UserId) {
    self
      .speaking_marks
      .lock()
      .expect("server session lock poisoned")
      .remove(&user_id);
    self.set_user_speaking(user_id, false);
  }

  fn clear_user_speaking_after_idle(&self, user_id: UserId, mut observed_mark: u64) {
    loop {
      thread::sleep(Duration::from_millis(850));

      let mut marks = self.speaking_marks.lock().expect("server session lock poisoned");
      match marks.get(&user_id).copied() {
        Some(current_mark) if current_mark == observed_mark => {
          marks.remove(&user_id);
          self
            .speaking_clear_scheduled
            .lock()
            .expect("server session lock poisoned")
            .remove(&user_id);
          drop(marks);
          self.set_user_speaking(user_id, false);
          return;
        }
        Some(current_mark) => {
          observed_mark = current_mark;
        }
        None => {
          self
            .speaking_clear_scheduled
            .lock()
            .expect("server session lock poisoned")
            .remove(&user_id);
          return;
        }
      }
    }
  }

  fn set_user_speaking(&self, user_id: UserId, speaking: bool) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      for users in lobby.users_by_channel.values_mut() {
        if let Some(user) = users.iter_mut().find(|user| user.user_id == user_id) {
          user.speaking = speaking;
        }
      }
      Self::sync_selected_users(&mut lobby);
    }
    self.bump_revision();
  }

  pub fn set_tofu_warning(&self, warning: TofuWarning) {
    *self.tofu_warning.lock().expect("server session lock poisoned") = Some(warning);
  }

  pub fn clear_tofu_warning(&self) {
    *self.tofu_warning.lock().expect("server session lock poisoned") = None;
  }

  pub fn tofu_warning(&self) -> Option<TofuWarning> {
    self.tofu_warning.lock().expect("server session lock poisoned").clone()
  }

  pub fn lobby(&self) -> LobbyState {
    self.lobby.lock().expect("server session lock poisoned").clone()
  }

  pub fn revision(&self) -> Signal<u64> {
    self.revision.clone()
  }

  fn bump_revision(&self) {
    self.revision.update(|revision| *revision = revision.wrapping_add(1));
  }

  fn sync_selected_users(lobby: &mut LobbyState) {
    lobby.users = lobby
      .selected_channel_id
      .and_then(|channel_id| lobby.users_by_channel.get(&channel_id).cloned())
      .unwrap_or_default();
  }

  fn sync_cached_channel_counts(lobby: &mut LobbyState) {
    for channel in &mut lobby.channels {
      if let Some(users) = lobby.users_by_channel.get(&channel.id) {
        channel.user_count = users.len() as u32;
      }
    }
  }

  pub fn select_channel(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      let previous = lobby.selected_channel_id;
      lobby.selected_channel_id = Some(channel_id);
      lobby.selected_text_channel_id = None;
      lobby.stream_browser_channel_id = None;
      for channel in &mut lobby.channels {
        channel.key_received = false;
      }
      Self::sync_selected_users(&mut lobby);
      tracing::debug!(target: "lobby",
        "[lobby] selected voice channel: previous={previous:?} current={channel_id} users={}",
        lobby.users.len()
      );
    }
    self.bump_revision();
  }

  pub fn leave_channel_locally(&self) {
    let local_user_id = self.info().map(|info| info.user_id);
    self.stop_voice();
    self.stop_video_broadcast();
    let mut left_voice = false;
    let mut watching_change = None;

    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if let Some(channel_id) = lobby.selected_channel_id.take() {
        left_voice = true;
        tracing::info!(target: "lobby", "[lobby] leaving voice channel locally: channel={channel_id} local_user={local_user_id:?}");
        if let Some(user_id) = local_user_id
          && let Some(users) = lobby.users_by_channel.get_mut(&channel_id)
        {
          users.retain(|user| user.user_id != user_id);
        }
      }
      if let Some(user_id) = local_user_id {
        lobby.screen_shares.retain(|share| share.sharer_user_id != user_id);
      }
      let (previous_user_id, changed) = Self::set_watching_user_in_lobby(&mut lobby, None);
      if changed {
        watching_change = Some(previous_user_id);
      }
      lobby.stream_browser_channel_id = None;
      lobby.users.clear();
      Self::sync_cached_channel_counts(&mut lobby);
    }

    if let Some(previous_user_id) = watching_change {
      self.finish_watching_user_change(previous_user_id, None);
    }
    if let Some(user_id) = local_user_id {
      self.clear_video_cache_for_user(user_id);
    }

    if let Some(user_id) = local_user_id {
      self
        .speaking_marks
        .lock()
        .expect("server session lock poisoned")
        .remove(&user_id);
      self
        .speaking_clear_scheduled
        .lock()
        .expect("server session lock poisoned")
        .remove(&user_id);
    }

    self.bump_revision();
    if left_voice {
      self.play_voice_leave_notification();
    }
  }

  pub fn select_text_channel(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      let previous = lobby.selected_text_channel_id;
      lobby.selected_text_channel_id = Some(channel_id);
      lobby.unread_text_channel_ids.remove(&channel_id);
      lobby.stream_browser_channel_id = None;
      tracing::debug!(target: "lobby", "[lobby] selected text channel: previous={previous:?} current={channel_id}");
    }
    self.bump_revision();
  }

  pub fn open_stream_browser(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if lobby.selected_channel_id == Some(channel_id) && lobby.channels.iter().any(|channel| channel.id == channel_id)
      {
        lobby.selected_text_channel_id = None;
        lobby.stream_browser_channel_id = Some(channel_id);
        tracing::info!(target: "video", "[video] stream browser opened: channel={channel_id}");
      }
    }
    self.bump_revision();
  }

  pub fn close_stream_browser(&self) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if lobby.stream_browser_channel_id.is_some() {
        tracing::info!(target: "video",
          "[video] stream browser closed: previous={:?}",
          lobby.stream_browser_channel_id
        );
      }
      lobby.stream_browser_channel_id = None;
    }
    self.bump_revision();
  }

  pub fn begin_chat_history_request(&self, channel_id: ChannelId) -> bool {
    let should_begin = {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if lobby.chat_history_has_more.get(&channel_id) == Some(&false)
        || lobby.chat_history_loading.contains(&channel_id)
      {
        false
      } else {
        lobby.chat_history_loading.insert(channel_id);
        true
      }
    };

    if should_begin {
      self.bump_revision();
    }

    should_begin
  }

  pub fn finish_chat_history_request(&self, channel_id: ChannelId, has_more: bool) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.chat_history_loading.remove(&channel_id);
      lobby.chat_history_has_more.insert(channel_id, has_more);
    }
    self.bump_revision();
  }

  pub fn set_watching_user(&self, user_id: Option<UserId>) {
    *self
      .pending_reconnect_watch_user_id
      .lock()
      .expect("server session lock poisoned") = None;
    let (previous_user_id, changed, view_changed) = {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      let previous_text_channel_id = lobby.selected_text_channel_id;
      if user_id.is_some() {
        lobby.selected_text_channel_id = None;
      }
      let (previous_user_id, changed) = Self::set_watching_user_in_lobby(&mut lobby, user_id);
      (
        previous_user_id,
        changed,
        previous_text_channel_id != lobby.selected_text_channel_id,
      )
    };
    if changed {
      tracing::info!(target: "video", "[video] watched stream changed: previous={previous_user_id:?} current={user_id:?}");
      self.clear_stream_audio(previous_user_id);
    }
    self.retain_video_cache(user_id);
    if changed || view_changed {
      self.bump_revision();
    }
  }

  pub fn has_pending_reconnect_watch(&self) -> bool {
    self
      .pending_reconnect_watch_user_id
      .lock()
      .expect("server session lock poisoned")
      .is_some()
  }

  pub async fn restore_pending_reconnect_watch(&self, settings: AppSettings, timeout: Duration) {
    let Some(user_id) = self.pending_reconnect_watch_user_id() else {
      return;
    };

    tracing::info!(target: "video", "[video] waiting to restore watched stream after reconnect: user={user_id}");
    let started_at = Instant::now();
    loop {
      if !self.pending_reconnect_watch_matches(user_id) {
        return;
      }

      if self.reconnect_watch_target_available(user_id) {
        if !self.take_pending_reconnect_watch_if_matches(user_id) {
          return;
        }
        if let Err(error) = self.request_reconnect_stream_view(user_id).await {
          tracing::warn!(target: "video", "[video] failed to restore watched stream after reconnect: user={user_id} error={error}");
          return;
        }

        self.set_watching_user(Some(user_id));
        if let Err(error) = self.ensure_stream_audio_playback(settings) {
          tracing::warn!(target: "audio::decode", "[audio:decode] stream playback unavailable after reconnect restore: {error}");
        }
        tracing::info!(target: "video", "[video] restored watched stream after reconnect: user={user_id}");
        return;
      }

      if started_at.elapsed() >= timeout {
        if self.take_pending_reconnect_watch_if_matches(user_id) {
          tracing::info!(target: "video",
            "[video] skipped watched stream restore after reconnect: user={user_id} reason=stream not advertised"
          );
        }
        return;
      }

      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  }

  fn pending_reconnect_watch_user_id(&self) -> Option<UserId> {
    *self
      .pending_reconnect_watch_user_id
      .lock()
      .expect("server session lock poisoned")
  }

  fn pending_reconnect_watch_matches(&self, user_id: UserId) -> bool {
    self.pending_reconnect_watch_user_id() == Some(user_id)
  }

  fn take_pending_reconnect_watch_if_matches(&self, user_id: UserId) -> bool {
    let mut pending = self
      .pending_reconnect_watch_user_id
      .lock()
      .expect("server session lock poisoned");
    if *pending == Some(user_id) {
      *pending = None;
      true
    } else {
      false
    }
  }

  fn reconnect_watch_target_available(&self, user_id: UserId) -> bool {
    let lobby = self.lobby.lock().expect("server session lock poisoned");
    lobby.screen_shares.iter().any(|share| share.sharer_user_id == user_id)
      && user_in_selected_voice_channel(&lobby, user_id)
  }

  async fn request_reconnect_stream_view(&self, user_id: UserId) -> Result<(), String> {
    let Some(server) = self.server() else {
      return Err("no connected server".to_owned());
    };
    tracing::info!(target: "video", "[video] requesting reconnect stream restore for user {user_id}");
    server
      .view_screen_share(user_id)
      .await
      .map_err(|error| error.to_string())?;
    match server.request_keyframe_stream(user_id).await {
      Ok(()) => {
        tracing::debug!(target: "video", "[video] keyframe requested on restored stream for user {user_id}");
      }
      Err(stream_error) => {
        tracing::warn!(target: "video",
          "[video] restored stream keyframe request failed for user {user_id}: {stream_error}; trying datagram"
        );
        if let Err(datagram_error) = server.request_keyframe(user_id) {
          return Err(datagram_error.to_string());
        }
        tracing::debug!(target: "video", "[video] restored stream keyframe requested by datagram for user {user_id}");
      }
    }
    Ok(())
  }

  fn set_watching_user_in_lobby(lobby: &mut LobbyState, user_id: Option<UserId>) -> (Option<UserId>, bool) {
    let previous_user_id = lobby.watching_user_id;
    lobby.watching_user_id = user_id;
    (previous_user_id, previous_user_id != user_id)
  }

  fn finish_watching_user_change(&self, previous_user_id: Option<UserId>, user_id: Option<UserId>) {
    self.clear_stream_audio(previous_user_id);
    self.retain_video_cache(user_id);
  }

  fn watching_user_id(&self) -> Option<UserId> {
    self
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .watching_user_id
  }

  fn video_decode_config_for_share(&self, user_id: UserId) -> Option<VideoDecodeConfig> {
    let lobby = self.lobby.lock().expect("server session lock poisoned");
    let metadata = &lobby
      .screen_shares
      .iter()
      .find(|share| share.sharer_user_id == user_id)?
      .metadata;
    if !metadata.codec.is_supported_stream_codec() || metadata.width == 0 || metadata.height == 0 {
      return None;
    }
    Some(VideoDecodeConfig {
      codec: metadata.codec,
      width: metadata.width,
      height: metadata.height,
    })
  }

  fn retain_video_cache(&self, watched_user_id: Option<UserId>) {
    let mut frames = self.video_frames.lock().expect("server session lock poisoned");
    let mut marks = self.video_revision_marks.lock().expect("server session lock poisoned");
    let mut errors = self.video_errors.lock().expect("server session lock poisoned");
    match watched_user_id {
      Some(user_id) => {
        frames.retain(|cached_user_id, _| *cached_user_id == user_id);
        marks.retain(|cached_user_id, _| *cached_user_id == user_id);
        errors.retain(|cached_user_id, _| *cached_user_id == user_id);
      }
      None => {
        frames.clear();
        marks.clear();
        errors.clear();
      }
    }
  }

  fn clear_video_cache_for_user(&self, user_id: UserId) {
    self
      .video_frames
      .lock()
      .expect("server session lock poisoned")
      .remove(&user_id);
    self
      .video_revision_marks
      .lock()
      .expect("server session lock poisoned")
      .remove(&user_id);
    self
      .video_errors
      .lock()
      .expect("server session lock poisoned")
      .remove(&user_id);
  }

  fn set_video_error(&self, user_id: UserId, error: VideoStreamError) {
    self
      .video_frames
      .lock()
      .expect("server session lock poisoned")
      .remove(&user_id);
    let changed = {
      let mut errors = self.video_errors.lock().expect("server session lock poisoned");
      let changed = errors.get(&user_id) != Some(&error);
      errors.insert(user_id, error);
      changed
    };
    if changed {
      self.bump_revision();
    }
  }

  fn clear_video_error(&self, user_id: UserId) {
    let cleared = self
      .video_errors
      .lock()
      .expect("server session lock poisoned")
      .remove(&user_id)
      .is_some();
    if cleared {
      self.bump_revision();
    }
  }

  pub fn start_voice(&self, settings: AppSettings, no_connected_server: &str) -> Result<(), String> {
    let server = self.server().ok_or_else(|| no_connected_server.to_owned())?;
    let (muted, deafened) = self.local_voice_state().unwrap_or((false, false));
    let on_local_voice = self.local_voice_callback();
    let engine =
      VoiceEngine::start(server, settings, muted, deafened, on_local_voice).map_err(|error| error.to_string())?;
    for (user_id, volume) in self
      .user_volumes
      .lock()
      .expect("server session lock poisoned")
      .iter()
      .map(|(user_id, volume)| (*user_id, *volume))
      .collect::<Vec<_>>()
    {
      engine.set_user_volume(user_id, volume);
    }
    for (user_id, volume) in self
      .stream_volumes
      .lock()
      .expect("server session lock poisoned")
      .iter()
      .map(|(user_id, volume)| (*user_id, *volume))
      .collect::<Vec<_>>()
    {
      engine.set_stream_volume(user_id, volume);
    }
    let mut voice_engine = self.voice_engine.lock().expect("server session lock poisoned");
    *voice_engine = Some(engine);
    Ok(())
  }

  pub fn ensure_stream_audio_playback(&self, settings: AppSettings) -> Result<(), String> {
    if self
      .voice_engine
      .lock()
      .expect("server session lock poisoned")
      .is_some()
    {
      return Ok(());
    }

    let (_, deafened) = self.local_voice_state().unwrap_or((false, false));
    let engine = VoiceEngine::start_playback(settings, deafened).map_err(|error| error.to_string())?;
    for (user_id, volume) in self
      .user_volumes
      .lock()
      .expect("server session lock poisoned")
      .iter()
      .map(|(user_id, volume)| (*user_id, *volume))
      .collect::<Vec<_>>()
    {
      engine.set_user_volume(user_id, volume);
    }
    for (user_id, volume) in self
      .stream_volumes
      .lock()
      .expect("server session lock poisoned")
      .iter()
      .map(|(user_id, volume)| (*user_id, *volume))
      .collect::<Vec<_>>()
    {
      engine.set_stream_volume(user_id, volume);
    }
    let mut voice_engine = self.voice_engine.lock().expect("server session lock poisoned");
    *voice_engine = Some(engine);
    Ok(())
  }

  pub fn voice_active(&self) -> bool {
    self
      .voice_engine
      .lock()
      .expect("server session lock poisoned")
      .as_ref()
      .is_some_and(VoiceEngine::captures_voice)
  }

  pub fn stop_voice(&self) {
    self.voice_engine.lock().expect("server session lock poisoned").take();
  }

  pub fn start_video_broadcast(&self, config: VideoBroadcastConfig, no_connected_server: &str) -> Result<(), String> {
    let server = self.server().ok_or_else(|| no_connected_server.to_owned())?;
    let local_user_id = self.info().ok_or_else(|| no_connected_server.to_owned())?.user_id;
    let broadcast = VideoBroadcast::start_with_loopback(server, config, Some(self.local_video_loopback(local_user_id)))
      .map_err(|error| {
        let error = error.to_string();
        tracing::error!(target: "video::encode", "[video:encode] VideoBroadcast::start failed: {error}");
        error
      })?;
    let backend = broadcast.backend();
    tracing::info!(target: "video::encode",
      "[video:encode] local broadcast backend selected: backend={}",
      native_video_backend_label(backend)
    );
    let mut video_broadcast = self.video_broadcast.lock().expect("server session lock poisoned");
    video_broadcast.replace(broadcast);
    tracing::info!(target: "video::encode", "[video:encode] local broadcaster stored in session");
    Ok(())
  }

  pub fn stop_video_broadcast(&self) {
    let stopped = self
      .video_broadcast
      .lock()
      .expect("server session lock poisoned")
      .take()
      .is_some();
    if stopped {
      tracing::info!(target: "video::encode", "[video:encode] local broadcaster stopped");
    }
  }

  fn local_voice_callback(&self) -> LocalVoiceCallback {
    let session = self.clone();
    let local_user_id = self.info().map(|info| info.user_id);

    Arc::new(move || {
      let Some(user_id) = local_user_id else {
        return;
      };
      session.mark_user_speaking(user_id);
    })
  }

  pub async fn run_lobby_receiver(&self) {
    let Some(server) = self.server() else {
      tracing::warn!(target: "network", "[network] lobby receiver not started: no connected server");
      return;
    };
    if self.shutdown_requested.load(Ordering::Relaxed) {
      tracing::warn!(target: "network", "[network] lobby receiver not started: shutdown is in progress");
      return;
    }
    if self.lobby.lock().expect("server session lock poisoned").disconnected {
      tracing::warn!(target: "network", "[network] lobby receiver not started: lobby is disconnected");
      return;
    }

    {
      let mut started = self.receiver_started.lock().expect("server session lock poisoned");
      if *started {
        tracing::warn!(target: "network", "[network] lobby receiver already running");
        return;
      }
      *started = true;
    }
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.receiver_running = true;
      lobby.last_error = None;
    }
    tracing::info!(target: "network", "[network] lobby receiver started");
    self.bump_revision();

    let session = self.clone();
    let ping_session = self.clone();
    let ping_server = server.clone();
    let ping_task = tokio::spawn(async move {
      ping_session.run_keepalive_sender(ping_server).await;
    });
    let voice_session = self.clone();
    let voice_server = server.clone();
    let voice_task = tokio::spawn(async move {
      voice_session.run_voice_activity_receiver(voice_server).await;
    });
    let video_stop = Arc::new(AtomicBool::new(false));
    *self.receiver_stop.lock().expect("server session lock poisoned") = Some(video_stop.clone());
    let video_thread = {
      let video_session = self.clone();
      let video_server = server.clone();
      let video_runtime = tokio::runtime::Handle::current();
      let video_stop = video_stop.clone();
      thread::Builder::new()
        .name("parties-video-receiver".to_owned())
        .spawn(move || {
          video_session.run_video_receiver(video_server, video_runtime, video_stop);
        })
        .ok()
    };

    loop {
      match server.recv().await {
        Ok(message) => {
          session.apply_server_message(message);
        }
        Err(error) => {
          let error = error.to_string();
          tracing::warn!(target: "network", "[network] lobby receiver error: {error}");
          session.mark_lobby_error(error);
          break;
        }
      };
    }

    voice_task.abort();
    video_stop.store(true, Ordering::Relaxed);
    server.wake_video_datagram_reader();
    ping_task.abort();
    drop(video_thread);
    *session.receiver_stop.lock().expect("server session lock poisoned") = None;
    *session.receiver_started.lock().expect("server session lock poisoned") = false;
    session
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .receiver_running = false;
    tracing::info!(target: "network", "[network] lobby receiver stopped");
    session.bump_revision();
  }

  async fn run_keepalive_sender(&self, server: Arc<Server>) {
    loop {
      *self
        .pending_keepalive_ping
        .lock()
        .expect("server session lock poisoned") = Some(Instant::now());
      let _ = server.ping().await;
      tokio::time::sleep(KEEPALIVE_INTERVAL).await;
    }
  }

  async fn run_voice_activity_receiver(&self, server: Arc<Server>) {
    loop {
      match server.recv_audio().await {
        Ok(ReceivedAudioPacket::Voice(packet)) => {
          let speaking = self.handle_voice_packet(packet.clone());
          if speaking {
            self.mark_user_speaking(packet.sender_id);
          }
        }
        Ok(ReceivedAudioPacket::Stream(packet)) => {
          self.handle_stream_audio_packet(packet);
        }
        Ok(ReceivedAudioPacket::VideoControl(control)) => {
          self.handle_video_control_packet(control);
        }
        Err(ServerError::Protocol(error)) => {
          tracing::warn!(target: "voice", "[voice] ignored malformed audio packet: {error}");
          continue;
        }
        Err(error) => {
          tracing::warn!(target: "voice", "[voice] voice receiver stopped: {error}");
          break;
        }
      }
    }
  }

  fn run_video_receiver(&self, server: Arc<Server>, runtime: tokio::runtime::Handle, stop: Arc<AtomicBool>) {
    tracing::info!(target: "video", "[video] receiver thread started");
    let _dx12_video_surfaces = self.dx12_video_surface_allocator();
    let queue = self.reset_video_packet_queue();
    let reader_thread = {
      let server = server.clone();
      let runtime = runtime.clone();
      let stop = stop.clone();
      let queue = queue.clone();
      thread::Builder::new()
        .name("parties-video-reader".to_owned())
        .spawn(move || {
          while !stop.load(Ordering::Relaxed) {
            match runtime.block_on(server.recv_video_frame()) {
              Ok(packet) => queue.push(packet),
              Err(ServerError::Protocol(error)) => {
                tracing::warn!(target: "video", "[video] ignored malformed video packet: {error}");
                continue;
              }
              Err(error) => {
                tracing::warn!(target: "video", "[video] video reader stopped: {error}");
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
      thread::Builder::new()
        .name("parties-video-datagram-reader".to_owned())
        .spawn(move || {
          while !stop.load(Ordering::Relaxed) {
            match runtime.block_on(server.recv_forwarded_video_datagram_until(stop.as_ref())) {
              Ok(Some(packet)) => queue.push(packet),
              Ok(None) => break,
              Err(ServerError::Protocol(error)) => {
                tracing::warn!(target: "video", "[video] ignored malformed video datagram: {error}");
                continue;
              }
              Err(error) => {
                tracing::warn!(target: "video", "[video] video datagram reader stopped: {error}");
                break;
              }
            }
          }
        })
        .ok()
    };
    let mut decoders = HashMap::<UserId, VideoDecoder>::new();
    let mut decoder_failures = HashSet::<(UserId, VideoDecodeConfig)>::new();
    #[cfg(target_os = "windows")]
    let mut dx12_decode_failures = HashSet::<(UserId, VideoDecodeConfig)>::new();
    #[cfg(target_os = "windows")]
    let mut shared_nv12_planes_decode_failures = HashSet::<(UserId, VideoDecodeConfig)>::new();
    #[cfg(target_os = "windows")]
    let mut shared_nv12_planes_surfaces =
      HashMap::<(UserId, usize, usize), Arc<lurq::app::dx12_render::Dx12Nv12Surface>>::new();
    let mut awaiting_keyframes = self.watching_user_id().into_iter().collect::<HashSet<_>>();
    let mut awaiting_decoded_output = HashSet::<UserId>::new();
    let mut received_counts = HashMap::<UserId, u64>::new();
    let mut decoded_counts = HashMap::<UserId, u64>::new();
    let mut last_watched_user = self.watching_user_id();
    let mut batch = Vec::<ForwardedVideoFrame>::with_capacity(MAX_QUEUED_VIDEO_PACKETS);
    let request_keyframe_for = |user_id: UserId, reason: &str| match runtime
      .block_on(server.request_keyframe_stream(user_id))
    {
      Ok(()) => {
        tracing::debug!(target: "video", "[video] keyframe requested for user {user_id}: reason={reason}");
      }
      Err(stream_error) => {
        tracing::warn!(target: "video", "[video] stream keyframe request failed for user {user_id}: reason={reason} error={stream_error}; trying datagram");
        match server.request_keyframe(user_id) {
          Ok(()) => {
            tracing::debug!(target: "video", "[video] datagram keyframe requested for user {user_id}: reason={reason}");
          }
          Err(datagram_error) => {
            tracing::warn!(target: "video", "[video] datagram keyframe request failed for user {user_id}: reason={reason} error={datagram_error}");
          }
        }
      }
    };

    while !stop.load(Ordering::Relaxed) {
      let Some(dropped_count) = ({
        let _span = profiler::span("video.receive.pop_batch");
        queue.pop_batch_into(&stop, &mut batch)
      }) else {
        break;
      };
      let _batch_span = profiler::span("video.receive.process_batch");

      let watched_user = self.watching_user_id();
      if watched_user != last_watched_user {
        decoders.retain(|user_id, _| Some(*user_id) == watched_user);
        decoder_failures.retain(|(user_id, _)| Some(*user_id) == watched_user);
        awaiting_decoded_output.retain(|user_id| Some(*user_id) == watched_user);
        #[cfg(target_os = "windows")]
        dx12_decode_failures.retain(|(user_id, _)| Some(*user_id) == watched_user);
        #[cfg(target_os = "windows")]
        shared_nv12_planes_decode_failures.retain(|(user_id, _)| Some(*user_id) == watched_user);
        #[cfg(target_os = "windows")]
        shared_nv12_planes_surfaces.retain(|(user_id, _, _), _| Some(*user_id) == watched_user);
        awaiting_keyframes.retain(|user_id| Some(*user_id) == watched_user);
        if let Some(user_id) = watched_user {
          awaiting_keyframes.insert(user_id);
          request_keyframe_for(user_id, "watch target changed");
          if let Some(config) = self.video_decode_config_for_share(user_id)
            && decoders.get(&user_id).is_none_or(|decoder| decoder.config() != &config)
          {
            match VideoDecoder::start(config.clone()) {
              Ok(decoder) => {
                let backend = decoder.backend();
                tracing::info!(target: "video::decode",
                  "[video:decode] decoder backend prewarmed for user {user_id}: backend={} codec={:?} size={}x{}",
                  native_video_backend_label(backend),
                  config.codec,
                  config.width,
                  config.height
                );
                decoders.insert(user_id, decoder);
              }
              Err(error) => {
                let error = error.to_string();
                tracing::warn!(target: "video::decode", "[video:decode] failed to prewarm decoder for user {user_id}: {error}");
                if native_decoder_unavailable_error(&error) {
                  self.set_video_error(user_id, native_decoder_unavailable_stream_error(error));
                }
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
        let affected_users = batch
          .iter()
          .filter(|packet| Some(packet.sender_id) == watched_user)
          .map(|packet| packet.sender_id)
          .collect::<HashSet<_>>();
        for user_id in &affected_users {
          let failed_decode_config = decoders
            .get(user_id)
            .map(|decoder| ((*user_id), decoder.config().clone()))
            .is_some_and(|failure_key| decoder_failures.contains(&failure_key));
          if failed_decode_config {
            continue;
          }
          awaiting_keyframes.insert(*user_id);
          request_keyframe_for(*user_id, "stale video backlog dropped");
        }
        tracing::warn!(target: "video",
          "[video] dropping stale video backlog: queued={} dropped={} users={}",
          batch.len(),
          dropped_count,
          affected_users.len()
        );
      }

      let latest_watched_packet_index = batch
        .iter()
        .enumerate()
        .filter(|(_, packet)| Some(packet.sender_id) == watched_user)
        .map(|(index, _)| index)
        .last();

      for (packet_index, packet) in batch.drain(..).enumerate() {
        if Some(packet.sender_id) != self.watching_user_id() {
          decoders.remove(&packet.sender_id);
          awaiting_keyframes.remove(&packet.sender_id);
          awaiting_decoded_output.remove(&packet.sender_id);
          continue;
        }

        if awaiting_keyframes.contains(&packet.sender_id) {
          if !packet.frame.keyframe {
            continue;
          }
          awaiting_keyframes.remove(&packet.sender_id);
          decoder_failures.retain(|(user_id, _)| *user_id != packet.sender_id);
          #[cfg(target_os = "windows")]
          dx12_decode_failures.retain(|(user_id, _)| *user_id != packet.sender_id);
          #[cfg(target_os = "windows")]
          shared_nv12_planes_decode_failures.retain(|(user_id, _)| *user_id != packet.sender_id);
          tracing::debug!(target: "video::decode",
            "[video:decode] catch-up keyframe received for user {}: frame={}",
            packet.sender_id,
            packet.frame.frame_number
          );
        }

        {
          let received_count = increment_counter(&mut received_counts, packet.sender_id);
          let output =
            Some(packet_index) == latest_watched_packet_index || awaiting_decoded_output.contains(&packet.sender_id);
          let decode_failure_key = video_decode_failure_key(packet.sender_id, &packet.frame);
          let had_known_decoder_failure = decoder_failures.contains(&decode_failure_key);
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
          if let Some(shared_planes_result) = decode_video_packet_to_shared_nv12_planes(
            &mut decoders,
            &mut decoder_failures,
            &mut shared_nv12_planes_decode_failures,
            &packet,
          ) {
            match shared_planes_result {
              Ok(Some((y_shared_handle, uv_shared_handle))) => {
                if let Some(surface) = self.shared_nv12_planes_video_surface_for_decode(
                  &mut shared_nv12_planes_surfaces,
                  packet.sender_id,
                  packet.frame.width,
                  packet.frame.height,
                  y_shared_handle,
                  uv_shared_handle,
                ) {
                  let decoded_count = increment_counter(&mut decoded_counts, packet.sender_id);
                  if should_log_video_count(decoded_count) {
                    tracing::debug!(target: "video::decode",
                      "[video:decode] decoded shared NV12 planes frame #{decoded_count} from user {}: codec={:?} size={}x{} y_handle=0x{y_shared_handle:x} uv_handle=0x{uv_shared_handle:x}",
                      packet.sender_id,
                      packet.frame.codec,
                      packet.frame.width,
                      packet.frame.height
                    );
                  }
                  self.handle_dx12_video_frame(
                    packet.sender_id,
                    packet.frame.codec,
                    packet.frame.width,
                    packet.frame.height,
                    surface,
                  );
                  continue;
                }
                shared_nv12_planes_decode_failures.insert(decode_failure_key.clone());
                decoders.remove(&packet.sender_id);
              }
              Ok(None) => {
                if should_log_video_count(received_count) {
                  tracing::info!(target: "video::decode", "[video:decode] received frame produced no shared NV12 planes decoded output yet");
                }
                continue;
              }
              Err(_) => {
                // The failure key is already recorded by decode_video_packet_to_shared_nv12_planes.
                // Fall through to the regular decode path so playback still starts.
              }
            }
          }

          #[cfg(target_os = "windows")]
          if let Some(surface) =
            self.dx12_video_surface_for_decode(packet.sender_id, packet.frame.width, packet.frame.height)
            && let Some(decoded) = decode_video_packet_to_dx12(
              &mut decoders,
              &mut decoder_failures,
              &mut dx12_decode_failures,
              &packet,
              &surface,
            )
          {
            if decoded {
              let decoded_count = increment_counter(&mut decoded_counts, packet.sender_id);
              if should_log_video_count(decoded_count) {
                tracing::debug!(target: "video::decode",
                  "[video:decode] decoded DX12 frame #{decoded_count} from user {}: codec={:?} size={}x{} format=Nv12",
                  packet.sender_id,
                  packet.frame.codec,
                  packet.frame.width,
                  packet.frame.height
                );
              }
              self.handle_dx12_video_frame(
                packet.sender_id,
                packet.frame.codec,
                packet.frame.width,
                packet.frame.height,
                surface,
              );
            } else if should_log_video_count(received_count) {
              tracing::info!(target: "video::decode", "[video:decode] received frame produced no DX12 decoded output yet");
            }
            continue;
          }

          let missing_initial_image =
            packet.frame.keyframe && !self.has_video_frame(packet.sender_id, packet.frame.width, packet.frame.height);
          let output = output || missing_initial_image;
          let output_buffer = if output {
            self.take_video_pixel_buffer(packet.sender_id, packet.frame.width, packet.frame.height)
          } else {
            None
          };
          let sender_id = packet.sender_id;
          let codec = packet.frame.codec;
          let decode_result = {
            let _span = profiler::span("video.decode.packet");
            decode_video_packet(&mut decoders, &mut decoder_failures, packet, output, output_buffer)
          };
          match decode_result {
            Ok(Some(frame)) => {
              awaiting_decoded_output.remove(&frame.sender_id);
              self.clear_video_error(frame.sender_id);
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
              self.handle_video_frame(frame);
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
              if unsupported_av1_decode_error(codec, &error) {
                self.set_video_error(sender_id, unsupported_av1_stream_error());
              } else if native_decoder_unavailable_error(&error) {
                self.set_video_error(sender_id, native_decoder_unavailable_stream_error(error));
              }
            }
          }
        }
      }
    }

    queue.close();
    drop(reader_thread);
    drop(datagram_reader_thread);
    tracing::info!(target: "video", "[video] receiver thread stopping");
  }

  fn handle_voice_packet(&self, packet: crate::network::protocol::data::ForwardedVoicePacket) -> bool {
    if self.info().is_some_and(|info| info.user_id == packet.sender_id) {
      return false;
    }

    let sender_id = packet.sender_id;
    let sequence = packet.sequence;
    let packet_len = packet.opus.len();
    let received_count = {
      let mut counts = self.voice_audio_counts.lock().expect("server session lock poisoned");
      increment_counter(&mut counts, sender_id)
    };
    if should_log_audio_count(received_count) {
      tracing::debug!(target: "audio::decode",
        "[audio:decode] received voice #{received_count} from user {}: sequence={} bytes={}",
        sender_id,
        sequence,
        packet_len
      );
    }

    let status = self
      .voice_engine
      .lock()
      .expect("server session lock poisoned")
      .as_mut()
      .map(|engine| engine.push_packet(packet));
    if should_log_audio_count(received_count) {
      tracing::debug!(target: "audio::decode",
        "[audio:decode] voice audio {} for user {} speaking={}",
        match status {
          Some(status) if status.queued => "queued",
          Some(_) => "dropped",
          None => "dropped: no voice engine",
        },
        sender_id,
        status.is_some_and(|status| status.speaking)
      );
    }
    status.map(|status| status.speaking).unwrap_or(true)
  }

  fn handle_video_control_packet(&self, control: VideoControl) {
    if let VideoControl::Pli { user_id } = control
      && let Some(broadcast) = self
        .video_broadcast
        .lock()
        .expect("video broadcast lock poisoned")
        .as_ref()
    {
      broadcast.request_keyframe();
      tracing::debug!(target: "video", "[video] local keyframe requested by viewer {user_id}");
    }
  }

  fn handle_stream_audio_packet(&self, packet: ForwardedStreamAudioPacket) {
    if self.info().is_some_and(|info| info.user_id == packet.sender_id) {
      return;
    }
    let watched_user_id = self.watching_user_id();
    let received_count = {
      let mut counts = self.stream_audio_counts.lock().expect("server session lock poisoned");
      increment_counter(&mut counts, packet.sender_id)
    };
    if should_log_audio_count(received_count) {
      tracing::debug!(target: "audio::decode",
        "[audio:decode] received stream audio #{received_count} from user {}: watched={watched_user_id:?} bytes={}",
        packet.sender_id,
        packet.opus.len()
      );
    }
    if watched_user_id != Some(packet.sender_id) {
      return;
    }

    let queued = self
      .voice_engine
      .lock()
      .expect("server session lock poisoned")
      .as_mut()
      .is_some_and(|engine| engine.push_stream_audio_packet(packet));
    if should_log_audio_count(received_count) {
      tracing::debug!(target: "audio::decode",
        "[audio:decode] stream audio {} for watched user {}",
        if queued { "queued" } else { "dropped" },
        watched_user_id.unwrap_or_default()
      );
    }
  }

  fn clear_stream_audio(&self, user_id: Option<UserId>) {
    let voice_engine = self.voice_engine.lock().expect("server session lock poisoned");
    let Some(engine) = voice_engine.as_ref() else {
      return;
    };

    if let Some(user_id) = user_id {
      engine.clear_stream_audio(user_id);
    } else {
      engine.clear_all_stream_audio();
    }
  }

  fn handle_video_frame(&self, frame: DecodedVideoFrame) {
    let _span = profiler::span("video.render.handle_frame");
    if self.watching_user_id() != Some(frame.sender_id) {
      return;
    }
    self.clear_video_error(frame.sender_id);

    let mut force_revision = true;
    #[cfg(target_os = "macos")]
    let prefer_cpu_frame = self.info().is_some_and(|info| info.user_id == frame.sender_id) && !frame.pixels.is_empty();
    #[cfg(target_os = "macos")]
    if !prefer_cpu_frame && let Some(native_image) = frame.native_image.clone() {
      {
        let mut frames = self.video_frames.lock().expect("server session lock poisoned");
        frames.insert(frame.sender_id, VideoFrameImage::MacosNative(native_image));
      }

      {
        let mut lobby = self.lobby.lock().expect("server session lock poisoned");
        if let Some(share) = lobby
          .screen_shares
          .iter_mut()
          .find(|share| share.sharer_user_id == frame.sender_id)
        {
          let metadata = ScreenShareMetadata {
            codec: frame.codec,
            width: frame.width,
            height: frame.height,
          };
          if share.metadata != metadata {
            share.metadata = metadata;
          }
        }
      }

      self.bump_revision();
      return;
    }

    {
      let _span = profiler::span("video.render.cpu_image_update");
      let mut frames = self.video_frames.lock().expect("server session lock poisoned");
      match frames.get(&frame.sender_id) {
        Some(image)
          if image.is_cpu_image()
            && image.image_data().width() == u32::from(frame.width)
            && image.image_data().height() == u32::from(frame.height)
            && image.image_data().format() == decoded_pixel_format_to_lurq(frame.format) =>
        {
          let _span = profiler::span("video.render.cpu_pixels_replace");
          image.set_cpu_pixels(frame.format, frame.pixels);
        }
        _ => {
          tracing::debug!(target: "video::decode",
            "[video:decode] creating streamed image for user {}: {}x{} format={:?}",
            frame.sender_id,
            frame.width,
            frame.height,
            frame.format
          );
          force_revision = true;
          let image = {
            let _span = profiler::span("video.render.cpu_image_create");
            match frame.format {
              DecodedVideoPixelFormat::Rgba8 => {
                StreamingImage::new_rgba_manual_redraw(frame.pixels, u32::from(frame.width), u32::from(frame.height))
              }
              DecodedVideoPixelFormat::Nv12 => {
                StreamingImage::new_nv12_manual_redraw(frame.pixels, u32::from(frame.width), u32::from(frame.height))
              }
            }
          };
          frames.insert(frame.sender_id, VideoFrameImage::Cpu(image));
        }
      }
    }

    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if let Some(share) = lobby
        .screen_shares
        .iter_mut()
        .find(|share| share.sharer_user_id == frame.sender_id)
      {
        let metadata = ScreenShareMetadata {
          codec: frame.codec,
          width: frame.width,
          height: frame.height,
        };
        if share.metadata != metadata {
          force_revision = true;
          share.metadata = metadata;
        }
      }
    }

    if force_revision || self.should_bump_video_revision(frame.sender_id) {
      self.bump_revision();
    }
  }

  #[cfg(target_os = "windows")]
  fn shared_nv12_planes_video_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, usize, usize), Arc<lurq::app::dx12_render::Dx12Nv12Surface>>,
    user_id: UserId,
    width: u16,
    height: u16,
    y_shared_handle: usize,
    uv_shared_handle: usize,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
    if y_shared_handle == 0 || uv_shared_handle == 0 {
      return None;
    }

    if let Some(surface) = surface_cache.get(&(user_id, y_shared_handle, uv_shared_handle)) {
      let image = surface.image_data();
      if image.width() == u32::from(width)
        && image.height() == u32::from(height)
        && image.format() == lurq::images::ImagePixelFormat::Nv12
        && !surface.is_packed_nv12()
        && surface.y_shared_handle_raw() as usize == y_shared_handle
        && surface.uv_shared_handle_raw() as usize == uv_shared_handle
      {
        return Some(surface.clone());
      }
      surface_cache.remove(&(user_id, y_shared_handle, uv_shared_handle));
    }

    {
      let frames = self.video_frames.lock().expect("server session lock poisoned");
      if let Some(VideoFrameImage::Dx12Surface(surface)) = frames.get(&user_id) {
        let image = surface.image_data();
        if image.width() == u32::from(width)
          && image.height() == u32::from(height)
          && image.format() == lurq::images::ImagePixelFormat::Nv12
          && !surface.is_packed_nv12()
          && surface.y_shared_handle_raw() as usize == y_shared_handle
          && surface.uv_shared_handle_raw() as usize == uv_shared_handle
        {
          return Some(surface.clone());
        }
      }
    }

    let allocator = self.dx12_video_surface_allocator()?;
    match allocator.open_shared_nv12_planes_surface(
      u32::from(width),
      u32::from(height),
      y_shared_handle as isize,
      uv_shared_handle as isize,
    ) {
      Ok(Some(surface)) => {
        let surface = Arc::new(surface);
        let native = surface.native_image_data();
        tracing::info!(target: "video::decode",
          "[video:decode] opened shared NV12 planes DX12 surface: user={user_id} image={} y_handle=0x{y_shared_handle:x} uv_handle=0x{uv_shared_handle:x} size={}x{} cache_entries={}",
          native.id(),
          width,
          height,
          surface_cache.len() + 1
        );
        if surface_cache.len() >= SHARED_NV12_PLANES_SURFACE_CACHE_LIMIT {
          surface_cache.retain(|(cached_user_id, cached_y_handle, cached_uv_handle), _| {
            *cached_user_id == user_id && *cached_y_handle == y_shared_handle && *cached_uv_handle == uv_shared_handle
          });
        }
        surface_cache.insert((user_id, y_shared_handle, uv_shared_handle), surface.clone());
        Some(surface)
      }
      Ok(None) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to open shared NV12 planes surface: DX12 video surface allocator is not ready");
        None
      }
      Err(error) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to open shared NV12 planes surface: y_handle=0x{y_shared_handle:x} uv_handle=0x{uv_shared_handle:x} size={}x{} error={error}", width, height);
        None
      }
    }
  }

  #[cfg(target_os = "windows")]
  fn dx12_video_surface_for_decode(
    &self,
    user_id: UserId,
    width: u16,
    height: u16,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
    if !*DX12_NATIVE_STREAM_DECODE_SUPPORTED {
      return None;
    }

    {
      let frames = self.video_frames.lock().expect("server session lock poisoned");
      if let Some(VideoFrameImage::Dx12Surface(surface)) = frames.get(&user_id) {
        let image = surface.image_data();
        if image.width() == u32::from(width)
          && image.height() == u32::from(height)
          && image.format() == lurq::images::ImagePixelFormat::Nv12
        {
          return Some(surface.clone());
        }
      }
    }

    let allocator = self.dx12_video_surface_allocator()?;
    match allocator.create_nv12_surface(u32::from(width), u32::from(height)) {
      Ok(Some(surface)) => Some(Arc::new(surface)),
      Ok(None) => None,
      Err(error) => {
        tracing::warn!(target: "video::decode", "[video:decode] failed to allocate DX12 video surface: {error}");
        None
      }
    }
  }

  #[cfg(target_os = "windows")]
  fn handle_dx12_video_frame(
    &self,
    sender_id: UserId,
    codec: crate::network::protocol::VideoCodecId,
    width: u16,
    height: u16,
    surface: Arc<lurq::app::dx12_render::Dx12Nv12Surface>,
  ) {
    let mut force_revision = false;
    let native = surface.native_image_data();
    let previous_version = native.version();
    native.bump_version();
    let bumped_version = native.version();
    let shared_handle = surface.y_shared_handle_raw();
    let packed_nv12 = surface.is_packed_nv12();
    {
      let mut frames = self.video_frames.lock().expect("server session lock poisoned");
      let replace = !matches!(
        frames.get(&sender_id),
        Some(VideoFrameImage::Dx12Surface(existing))
          if Arc::ptr_eq(existing, &surface)
            && existing.image_data().width() == u32::from(width)
            && existing.image_data().height() == u32::from(height)
      );
      if replace {
        if bumped_version == 1 || bumped_version % 120 == 0 {
          tracing::info!(target: "video::decode",
            "[video:decode] storing DX12 video frame for user {sender_id}: size={width}x{height} packed={packed_nv12} handle=0x{shared_handle:x} version={previous_version}->{bumped_version} replace=true"
          );
        }
        frames.insert(sender_id, VideoFrameImage::Dx12Surface(surface));
      } else if bumped_version == 1 || bumped_version % 120 == 0 {
        tracing::info!(target: "video::decode",
          "[video:decode] updating DX12 video frame for user {sender_id}: size={width}x{height} packed={packed_nv12} handle=0x{shared_handle:x} version={previous_version}->{bumped_version} replace=false"
        );
      }
    }

    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if let Some(share) = lobby
        .screen_shares
        .iter_mut()
        .find(|share| share.sharer_user_id == sender_id)
      {
        let metadata = ScreenShareMetadata { codec, width, height };
        if share.metadata != metadata {
          force_revision = true;
          share.metadata = metadata;
        }
      }
    }

    let bump_revision = force_revision || self.should_bump_video_revision(sender_id);
    if bump_revision && (bumped_version == 1 || bumped_version % 120 == 0) {
      tracing::info!(target: "video::decode",
        "[video:decode] bumping video revision for DX12 frame: user={sender_id} packed={packed_nv12} handle=0x{shared_handle:x} version={bumped_version} forced={force_revision}"
      );
    }
    if bump_revision {
      self.bump_revision();
    }
  }

  fn take_video_pixel_buffer(&self, user_id: UserId, width: u16, height: u16) -> Option<Vec<u8>> {
    self
      .video_frames
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .filter(|image| {
        image.image_data().width() == u32::from(width) && image.image_data().height() == u32::from(height)
      })
      .and_then(VideoFrameImage::take_cpu_buffer)
  }

  fn has_video_frame(&self, user_id: UserId, width: u16, height: u16) -> bool {
    self
      .video_frames
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .is_some_and(|image| {
        image.image_data().width() == u32::from(width) && image.image_data().height() == u32::from(height)
      })
  }

  fn should_bump_video_revision(&self, user_id: UserId) -> bool {
    let now = Instant::now();
    let mut marks = self.video_revision_marks.lock().expect("server session lock poisoned");
    match marks.get_mut(&user_id) {
      Some(last) if now.duration_since(*last) < VIDEO_REVISION_INTERVAL => false,
      Some(last) => {
        *last = now;
        true
      }
      None => {
        marks.insert(user_id, now);
        true
      }
    }
  }

  pub fn mark_lobby_error(&self, message: String) {
    if self.shutdown_requested.load(Ordering::Relaxed) {
      tracing::warn!(target: "network", "[network] ignoring lobby receiver error during shutdown: {message}");
      return;
    }

    tracing::warn!(target: "network", "[network] marking lobby disconnected: {message}");
    self.stop_video_broadcast();
    let mut watching_change = None;
    let reconnect_watch_user_id;
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      reconnect_watch_user_id = lobby.watching_user_id;
      lobby.receiver_running = false;
      lobby.disconnected = true;
      lobby.last_error = Some(message);
      lobby.stream_browser_channel_id = None;
      lobby.screen_shares.clear();
      let (previous_user_id, changed) = Self::set_watching_user_in_lobby(&mut lobby, None);
      if changed {
        watching_change = Some(previous_user_id);
      }
    }
    *self
      .pending_reconnect_watch_user_id
      .lock()
      .expect("server session lock poisoned") = reconnect_watch_user_id;
    if let Some(user_id) = reconnect_watch_user_id {
      tracing::info!(target: "video", "[video] saved watched stream target for reconnect: user={user_id}");
    }
    if let Some(previous_user_id) = watching_change {
      self.finish_watching_user_change(previous_user_id, None);
    }
    self.bump_revision();
  }

  fn apply_server_message(&self, message: S2C) {
    let local_info = self.info();
    let local_user_id = local_info.as_ref().map(|info| info.user_id);
    let local_display_name = local_info
      .as_ref()
      .map(|info| info.display_name.trim().to_owned())
      .unwrap_or_default();
    let local_voice_state = *self.local_voice_fallback.lock().expect("server session lock poisoned");
    let mut local_voice_update = None;
    let mut stop_local_voice = false;
    let mut clear_speaking_user = None;
    let mut notification_sound = None;
    let mut watching_change = None;
    let mut lobby = self.lobby.lock().expect("server session lock poisoned");

    match message {
      S2C::ChannelList(list) => {
        tracing::debug!(target: "lobby",
          "[lobby] received voice channel list: channels={} selected={:?}",
          list.channels.len(),
          lobby.selected_channel_id
        );
        let selected = lobby.selected_channel_id;
        lobby.channels = list.channels.into_iter().map(LobbyChannel::from).collect();
        lobby.channels.sort_by_key(|channel| channel.sort_order);
        let channel_ids: Vec<_> = lobby.channels.iter().map(|channel| channel.id).collect();
        lobby
          .users_by_channel
          .retain(|channel_id, _| channel_ids.contains(channel_id));
        lobby.channel_list_received = true;

        if selected.is_some_and(|id| lobby.channels.iter().any(|channel| channel.id == id)) {
          lobby.selected_channel_id = selected;
          Self::sync_selected_users(&mut lobby);
        } else {
          lobby.selected_channel_id = None;
          lobby.stream_browser_channel_id = None;
          lobby.users.clear();
        }
      }
      S2C::ChatChannelList { channels } => {
        tracing::debug!(target: "lobby",
          "[lobby] received text channel list: channels={} selected={:?}",
          channels.len(),
          lobby.selected_text_channel_id
        );
        let selected = lobby.selected_text_channel_id;
        lobby.text_channels = channels.into_iter().map(LobbyTextChannel::from).collect();
        lobby.text_channels.sort_by_key(|channel| channel.sort_order);
        let channel_ids: Vec<_> = lobby.text_channels.iter().map(|channel| channel.id).collect();
        lobby
          .chat_messages_by_channel
          .retain(|channel_id, _| channel_ids.contains(channel_id));
        lobby
          .unread_text_channel_ids
          .retain(|channel_id| channel_ids.contains(channel_id));
        lobby
          .chat_history_loading
          .retain(|channel_id| channel_ids.contains(channel_id));
        lobby
          .chat_history_has_more
          .retain(|channel_id, _| channel_ids.contains(channel_id));

        if selected.is_some_and(|id| lobby.text_channels.iter().any(|channel| channel.id == id)) {
          lobby.selected_text_channel_id = selected;
        } else {
          lobby.selected_text_channel_id = lobby.text_channels.first().map(|channel| channel.id);
        }
      }
      S2C::ChatMessage(message) => {
        let should_notify = local_user_id != Some(message.sender_id);
        let message_mentions_local_user =
          should_notify && message_mentions_display_name(&message.text, &local_display_name);
        tracing::debug!(target: "chat",
          "[chat] received message: id={} channel={} sender={} local={} notify={}",
          message.id,
          message.channel_id,
          message.sender_id,
          !should_notify,
          should_notify
        );
        if should_notify && lobby.selected_text_channel_id != Some(message.channel_id) {
          lobby.unread_text_channel_ids.insert(message.channel_id);
        }
        Self::merge_chat_messages(
          lobby.chat_messages_by_channel.entry(message.channel_id).or_default(),
          [message],
        );
        if should_notify {
          notification_sound = Some(if message_mentions_local_user {
            NotificationSound::Mention
          } else {
            NotificationSound::ChatMessage
          });
        }
      }
      S2C::ChatHistoryResp(response) => {
        tracing::debug!(target: "chat",
          "[chat] received history: channel={} messages={} has_more={}",
          response.channel_id,
          response.messages.len(),
          response.has_more
        );
        lobby.chat_history_loading.remove(&response.channel_id);
        lobby
          .chat_history_has_more
          .insert(response.channel_id, response.has_more);
        Self::merge_chat_messages(
          lobby.chat_messages_by_channel.entry(response.channel_id).or_default(),
          response.messages,
        );
      }
      S2C::ChatMessageDeleted { message_id, channel_id } => {
        tracing::info!(target: "chat", "[chat] message deleted: id={message_id} channel={channel_id}");
        if let Some(messages) = lobby.chat_messages_by_channel.get_mut(&channel_id) {
          messages.retain(|message| message.id != message_id);
        }
      }
      S2C::ChannelUserList(list) => {
        tracing::debug!(target: "lobby",
          "[lobby] received channel user list: channel={} users={} selected={:?}",
          list.channel_id,
          list.users.len(),
          lobby.selected_channel_id
        );
        let mut users = list.users.into_iter().map(LobbyUser::from).collect::<Vec<_>>();
        Self::apply_local_voice_state(&mut users, local_user_id, local_voice_state);
        for user in &users {
          for (channel_id, cached_users) in &mut lobby.users_by_channel {
            if *channel_id != list.channel_id {
              cached_users.retain(|cached| cached.user_id != user.user_id);
            }
          }
        }
        lobby.users_by_channel.insert(list.channel_id, users);
        Self::sync_selected_users(&mut lobby);
        Self::sync_cached_channel_counts(&mut lobby);
      }
      S2C::UserJoinedChannel(joined) => {
        let joined_user_id = joined.user_id;
        let joined_username = joined.username.clone();
        let joined_channel_id = joined.channel_id;
        let joined_role = joined.role;
        let selected_channel_id = lobby.selected_channel_id;
        let was_in_selected_channel = selected_channel_id
          .and_then(|channel_id| lobby.users_by_channel.get(&channel_id))
          .is_some_and(|users| users.iter().any(|user| user.user_id == joined_user_id));
        for (channel_id, users) in &mut lobby.users_by_channel {
          if *channel_id != joined_channel_id {
            users.retain(|user| user.user_id != joined_user_id);
          }
        }
        let users = lobby.users_by_channel.entry(joined_channel_id).or_default();
        let inserted = if users.iter().any(|user| user.user_id == joined_user_id) {
          false
        } else {
          let local = local_user_id == Some(joined_user_id);
          users.push(LobbyUser {
            user_id: joined_user_id,
            username: joined.username,
            role: joined.role,
            muted: local && local_voice_state.0,
            deafened: local && local_voice_state.1,
            speaking: false,
          });
          true
        };
        if lobby.selected_channel_id == Some(joined_channel_id) {
          Self::sync_selected_users(&mut lobby);
        }
        tracing::debug!(target: "lobby",
          "[lobby] user joined voice channel: user={} name='{}' channel={} role={:?} local={} inserted={} selected={:?}",
          joined_user_id,
          joined_username,
          joined_channel_id,
          joined_role,
          local_user_id == Some(joined_user_id),
          inserted,
          selected_channel_id
        );
        if inserted {
          Self::sync_cached_channel_counts(&mut lobby);
          if local_user_id != Some(joined_user_id) {
            if selected_channel_id == Some(joined_channel_id) {
              notification_sound = Some(NotificationSound::VoiceJoin);
            } else if was_in_selected_channel {
              notification_sound = Some(NotificationSound::VoiceLeave);
            }
          }
        }
      }
      S2C::UserLeftChannel(left) => {
        let local_left = local_user_id == Some(left.user_id);
        let was_in_selected_channel = lobby
          .selected_channel_id
          .and_then(|channel_id| lobby.users_by_channel.get(&channel_id))
          .is_some_and(|users| users.iter().any(|user| user.user_id == left.user_id));
        for users in lobby.users_by_channel.values_mut() {
          users.retain(|user| user.user_id != left.user_id);
        }
        tracing::debug!(target: "lobby",
          "[lobby] user left voice channel: user={} channel={} local={} was_selected_channel={}",
          left.user_id,
          left.channel_id,
          local_left,
          was_in_selected_channel
        );
        if local_left {
          stop_local_voice = true;
        }
        clear_speaking_user = Some(left.user_id);
        lobby.screen_shares.retain(|share| share.sharer_user_id != left.user_id);
        self.clear_video_cache_for_user(left.user_id);
        if local_left || lobby.watching_user_id == Some(left.user_id) {
          let (previous_user_id, changed) = Self::set_watching_user_in_lobby(&mut lobby, None);
          if changed {
            watching_change = Some(previous_user_id);
          }
        }
        if local_left && lobby.selected_channel_id == Some(left.channel_id) {
          lobby.selected_channel_id = None;
          lobby.stream_browser_channel_id = None;
          lobby.users.clear();
        } else if lobby.selected_channel_id == Some(left.channel_id) {
          Self::sync_selected_users(&mut lobby);
        }
        Self::sync_cached_channel_counts(&mut lobby);
        if local_left && was_in_selected_channel {
          notification_sound = Some(NotificationSound::UserKicked);
        } else if !local_left && was_in_selected_channel {
          notification_sound = Some(NotificationSound::VoiceLeave);
        }
      }
      S2C::UserVoiceState(state) => {
        let local_state_changed_externally =
          local_user_id == Some(state.user_id) && local_voice_state != (state.muted, state.deafened);
        tracing::debug!(target: "voice",
          "[voice] user state changed: user={} muted={} deafened={} local={}",
          state.user_id,
          state.muted,
          state.deafened,
          local_user_id == Some(state.user_id)
        );
        if local_user_id == Some(state.user_id) {
          local_voice_update = Some((state.muted, state.deafened));
          if local_state_changed_externally {
            notification_sound = Some(NotificationSound::ModerationAction);
          }
        }
        for users in lobby.users_by_channel.values_mut() {
          if let Some(user) = users.iter_mut().find(|user| user.user_id == state.user_id) {
            user.muted = state.muted;
            user.deafened = state.deafened;
          }
        }
        Self::sync_selected_users(&mut lobby);
      }
      S2C::UserRoleChanged(changed) => {
        tracing::debug!(target: "lobby",
          "[lobby] user role changed: user={} role={:?} local={}",
          changed.user_id,
          changed.role,
          local_user_id == Some(changed.user_id)
        );
        for users in lobby.users_by_channel.values_mut() {
          if let Some(user) = users.iter_mut().find(|user| user.user_id == changed.user_id) {
            user.role = changed.role;
          }
        }
        Self::sync_selected_users(&mut lobby);
        if let Some(current) = self.current.lock().expect("server session lock poisoned").as_mut()
          && current.info.user_id == changed.user_id
        {
          current.info.role = changed.role;
        }
      }
      S2C::KeepalivePong => {
        lobby.keepalive_ok = true;
        if let Some(sent_at) = self
          .pending_keepalive_ping
          .lock()
          .expect("server session lock poisoned")
          .take()
        {
          lobby.ping_ms = Some(sent_at.elapsed().as_millis().min(u128::from(u32::MAX)) as u32);
        }
      }
      S2C::ChannelKey(key) => {
        tracing::debug!(target: "lobby",
          "[lobby] received channel key: channel={} bytes={}",
          key.channel_id,
          key.key.len()
        );
        if let Some(channel) = lobby.channels.iter_mut().find(|channel| channel.id == key.channel_id) {
          channel.key_received = true;
        }
      }
      S2C::ScreenShareStarted(started) => {
        let should_notify_stream_started = local_user_id != Some(started.sharer_user_id)
          && user_in_selected_voice_channel(&lobby, started.sharer_user_id);
        tracing::info!(target: "video",
          "[video] screen share started: user={} codec={:?} size={}x{} local={}",
          started.sharer_user_id,
          started.metadata.codec,
          started.metadata.width,
          started.metadata.height,
          local_user_id == Some(started.sharer_user_id)
        );
        if let Some(existing) = lobby
          .screen_shares
          .iter_mut()
          .find(|share| share.sharer_user_id == started.sharer_user_id)
        {
          existing.metadata = started.metadata;
        } else {
          lobby.screen_shares.push(LobbyScreenShare {
            sharer_user_id: started.sharer_user_id,
            metadata: started.metadata,
          });
        }
        if should_notify_stream_started {
          notification_sound = Some(NotificationSound::StreamStarted);
        }
      }
      S2C::ScreenShareStopped { sharer_user_id } => {
        let was_watching_stopped_stream = lobby.watching_user_id == Some(sharer_user_id);
        tracing::warn!(target: "video",
          "[video] screen share stopped: user={} local={} watched={}",
          sharer_user_id,
          local_user_id == Some(sharer_user_id),
          was_watching_stopped_stream
        );
        lobby
          .screen_shares
          .retain(|share| share.sharer_user_id != sharer_user_id);
        self.clear_video_cache_for_user(sharer_user_id);
        if was_watching_stopped_stream {
          let (previous_user_id, changed) = Self::set_watching_user_in_lobby(&mut lobby, None);
          if changed {
            watching_change = Some(previous_user_id);
          }
          if local_user_id != Some(sharer_user_id) {
            notification_sound = Some(NotificationSound::StreamEnded);
          }
        }
      }
      S2C::ScreenShareDenied { reason } => {
        tracing::warn!(target: "video", "[video] screen share denied: {reason}");
        lobby.last_error = Some(reason);
        notification_sound = Some(NotificationSound::ModerationAction);
      }
      S2C::ServerError { message: reason } => {
        tracing::error!(target: "network", "[network] server error: {reason}");
        if reason.to_ascii_lowercase().contains("kick") {
          notification_sound = Some(NotificationSound::UserKicked);
        }
        lobby.last_error = Some(reason);
      }
      S2C::AdminResult(result) => {
        tracing::info!(target: "admin",
          "[admin] result: success={} message='{}'",
          result.success,
          result.message
        );
        lobby.last_error = if result.success { None } else { Some(result.message) };
      }
      S2C::AuthResponse(_)
      | S2C::ChatFileUploadResp(_)
      | S2C::ChatFileReady { .. }
      | S2C::ChatSearchResp { .. }
      | S2C::ChatPinnedResp { .. } => {}
    }

    drop(lobby);
    if let Some(previous_user_id) = watching_change {
      self.finish_watching_user_change(previous_user_id, None);
    }
    if let Some(sound) = notification_sound {
      self.play_notification_sound(sound);
    }
    if let Some(state) = local_voice_update {
      *self.local_voice_fallback.lock().expect("server session lock poisoned") = state;
    }
    if let Some(user_id) = clear_speaking_user {
      self
        .speaking_marks
        .lock()
        .expect("server session lock poisoned")
        .remove(&user_id);
      self
        .speaking_clear_scheduled
        .lock()
        .expect("server session lock poisoned")
        .remove(&user_id);
    }
    if stop_local_voice {
      self.stop_voice();
      self.stop_video_broadcast();
      if let Some(user_id) = local_user_id {
        self
          .speaking_marks
          .lock()
          .expect("server session lock poisoned")
          .remove(&user_id);
        self
          .speaking_clear_scheduled
          .lock()
          .expect("server session lock poisoned")
          .remove(&user_id);
      }
    }
    self.bump_revision();
  }

  fn apply_local_voice_state(users: &mut [LobbyUser], local_user_id: Option<UserId>, state: (bool, bool)) {
    let Some(local_user_id) = local_user_id else {
      return;
    };

    if let Some(user) = users.iter_mut().find(|user| user.user_id == local_user_id) {
      user.muted = state.0;
      user.deafened = state.1;
    }
  }

  fn merge_chat_messages(
    messages: &mut Vec<ProtocolChatMessage>,
    incoming: impl IntoIterator<Item = ProtocolChatMessage>,
  ) {
    for message in incoming {
      if let Some(existing) = messages.iter_mut().find(|existing| existing.id == message.id) {
        *existing = message;
      } else {
        messages.push(message);
      }
    }

    messages.sort_by_key(|message| (message.timestamp, message.id));

    const MAX_CACHED_MESSAGES_PER_CHANNEL: usize = 250;
    if messages.len() > MAX_CACHED_MESSAGES_PER_CHANNEL {
      let trim = messages.len() - MAX_CACHED_MESSAGES_PER_CHANNEL;
      messages.drain(..trim);
    }
  }
}

fn user_in_selected_voice_channel(lobby: &LobbyState, user_id: UserId) -> bool {
  lobby
    .selected_channel_id
    .and_then(|channel_id| lobby.users_by_channel.get(&channel_id))
    .is_some_and(|users| users.iter().any(|user| user.user_id == user_id))
}

fn message_mentions_display_name(text: &str, display_name: &str) -> bool {
  let display_name = display_name.trim();
  if display_name.is_empty() {
    return false;
  }

  let text = text.to_ascii_lowercase();
  let display_name = display_name.to_ascii_lowercase();
  if text.contains(&format!("@{display_name}")) {
    return true;
  }
  if display_name.contains(char::is_whitespace) {
    return text.contains(&display_name);
  }

  text
    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
    .any(|token| token == display_name)
}

fn decoded_pixel_format_to_lurq(format: DecodedVideoPixelFormat) -> lurq::images::ImagePixelFormat {
  match format {
    DecodedVideoPixelFormat::Rgba8 => lurq::images::ImagePixelFormat::Rgba8,
    DecodedVideoPixelFormat::Nv12 => lurq::images::ImagePixelFormat::Nv12,
  }
}

#[cfg(test)]
mod tests {
  use super::message_mentions_display_name;

  #[test]
  fn mention_detection_matches_at_display_name() {
    assert!(message_mentions_display_name("hey @Lurk", "lurk"));
  }

  #[test]
  fn mention_detection_matches_display_name_token() {
    assert!(message_mentions_display_name("thanks Lurk!", "lurk"));
  }

  #[test]
  fn mention_detection_does_not_match_partial_words() {
    assert!(!message_mentions_display_name("the lurking issue", "lurk"));
  }
}
