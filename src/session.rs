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
      data::{ForwardedStreamAudioPacket, ForwardedVideoFrame},
    },
    server::{ReceivedAudioPacket, Server, ServerError},
  },
  services::{
    logger,
    video::{
      DecodedVideoFrame, DecodedVideoPixelFormat, NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig,
      VideoDecodeConfig, VideoDecoder,
    },
    voice::{LocalVoiceCallback, VoiceEngine},
  },
  storage::AppSettings,
};

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_USER_VOLUME: i32 = 100;
const MAX_QUEUED_VIDEO_PACKETS: usize = 12;
const MAX_DECODE_VIDEO_BATCH: usize = 10;
const VIDEO_REVISION_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedServerInfo {
  pub address: String,
  pub server_name: String,
  pub display_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub certificate_fingerprint: String,
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

fn decode_video_packet(
  decoders: &mut HashMap<UserId, VideoDecoder>,
  decoder_failures: &mut HashSet<(UserId, VideoDecodeConfig)>,
  packet: ForwardedVideoFrame,
  output: bool,
  output_buffer: Option<Vec<u8>>,
) -> Option<DecodedVideoFrame> {
  let config = VideoDecodeConfig {
    codec: packet.frame.codec,
    width: packet.frame.width,
    height: packet.frame.height,
  };
  let failure_key = (packet.sender_id, config.clone());

  if decoder_failures.contains(&failure_key) {
    return None;
  }

  if decoders
    .get(&packet.sender_id)
    .is_none_or(|decoder| decoder.config() != &config)
  {
    match VideoDecoder::start(config.clone()) {
      Ok(decoder) => {
        logger::log(&format!(
          "[video] decoder ready for user {}: codec={:?} size={}x{}",
          packet.sender_id, config.codec, config.width, config.height
        ));
        decoders.insert(packet.sender_id, decoder);
      }
      Err(error) => {
        logger::log(&format!(
          "[video] failed to start decoder for user {}: {error}",
          packet.sender_id
        ));
        decoder_failures.insert(failure_key);
        return None;
      }
    }
  }

  let decoder = decoders.get_mut(&packet.sender_id)?;
  match decoder.decode_with_output_buffer(&packet, output, output_buffer) {
    Ok(frame) => frame,
    Err(error) => {
      logger::log(&format!(
        "[video] failed to decode frame from user {}: {error}",
        packet.sender_id
      ));
      decoders.remove(&packet.sender_id);
      decoder_failures.insert(failure_key);
      None
    }
  }
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
        logger::log(&format!(
          "[video] decoder ready for user {}: codec={:?} size={}x{}",
          packet.sender_id, config.codec, config.width, config.height
        ));
        decoders.insert(packet.sender_id, decoder);
      }
      Err(error) => {
        logger::log(&format!(
          "[video] failed to start decoder for user {}: {error}",
          packet.sender_id
        ));
        decoder_failures.insert(failure_key);
        return Some(false);
      }
    }
  }

  let decoder = decoders.get_mut(&packet.sender_id)?;
  if decoder.backend() != NativeVideoBackend::NvidiaNvdec {
    return None;
  }

  match decoder.decode_to_dx12_surface(packet, surface) {
    Ok(decoded) => Some(decoded),
    Err(error) => {
      logger::log(&format!(
        "[video] failed to decode frame from user {} into DX12 surface: {error}",
        packet.sender_id
      ));
      decoders.remove(&packet.sender_id);
      dx12_failures.insert(failure_key);
      None
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
    {
      let mut packets = self.packets.lock().expect("video packet queue lock poisoned");
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
      #[cfg(target_os = "windows")]
      Self::Dx12Surface(surface) => surface.image_data(),
    }
  }

  fn set_cpu_pixels(&self, format: DecodedVideoPixelFormat, pixels: Vec<u8>) -> bool {
    let Self::Cpu(image) = self else {
      return false;
    };
    match format {
      DecodedVideoPixelFormat::Rgba8 => image.set_rgba(pixels),
      DecodedVideoPixelFormat::Nv12 => image.set_nv12(pixels),
    }
    true
  }

  fn take_cpu_buffer(&self) -> Option<Vec<u8>> {
    let Self::Cpu(image) = self else {
      return None;
    };
    match image.image_data().format() {
      lurq::images::ImagePixelFormat::Rgba8 => image.take_rgba_buffer(),
      lurq::images::ImagePixelFormat::Nv12 => image.take_nv12_buffer(),
    }
  }
}

#[derive(Clone)]
pub struct ServerSession {
  current: Arc<Mutex<Option<ConnectedServer>>>,
  tofu_warning: Arc<Mutex<Option<TofuWarning>>>,
  lobby: Arc<Mutex<LobbyState>>,
  receiver_started: Arc<Mutex<bool>>,
  local_voice_fallback: Arc<Mutex<(bool, bool)>>,
  muted_before_deafen: Arc<Mutex<Option<bool>>>,
  speaking_marks: Arc<Mutex<HashMap<UserId, u64>>>,
  speaking_mark_counter: Arc<Mutex<u64>>,
  speaking_clear_scheduled: Arc<Mutex<HashSet<UserId>>>,
  pending_keepalive_ping: Arc<Mutex<Option<Instant>>>,
  voice_engine: Arc<Mutex<Option<VoiceEngine>>>,
  video_broadcast: Arc<Mutex<Option<VideoBroadcast>>>,
  video_frames: Arc<Mutex<HashMap<UserId, VideoFrameImage>>>,
  #[cfg(target_os = "windows")]
  dx12_video_surfaces: Option<lurq::app::dx12_render::Dx12VideoSurfaceAllocator>,
  video_revision_marks: Arc<Mutex<HashMap<UserId, Instant>>>,
  stream_audio_counts: Arc<Mutex<HashMap<UserId, u64>>>,
  user_volumes: Arc<Mutex<HashMap<UserId, i32>>>,
  revision: Signal<u64>,
}

impl Default for ServerSession {
  fn default() -> Self {
    Self {
      current: Arc::new(Mutex::new(None)),
      tofu_warning: Arc::new(Mutex::new(None)),
      lobby: Arc::new(Mutex::new(LobbyState::default())),
      receiver_started: Arc::new(Mutex::new(false)),
      local_voice_fallback: Arc::new(Mutex::new((false, false))),
      muted_before_deafen: Arc::new(Mutex::new(None)),
      speaking_marks: Arc::new(Mutex::new(HashMap::new())),
      speaking_mark_counter: Arc::new(Mutex::new(0)),
      speaking_clear_scheduled: Arc::new(Mutex::new(HashSet::new())),
      pending_keepalive_ping: Arc::new(Mutex::new(None)),
      voice_engine: Arc::new(Mutex::new(None)),
      video_broadcast: Arc::new(Mutex::new(None)),
      video_frames: Arc::new(Mutex::new(HashMap::new())),
      #[cfg(target_os = "windows")]
      dx12_video_surfaces: None,
      video_revision_marks: Arc::new(Mutex::new(HashMap::new())),
      stream_audio_counts: Arc::new(Mutex::new(HashMap::new())),
      user_volumes: Arc::new(Mutex::new(HashMap::new())),
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

  pub fn set_connected(&self, connected: ConnectedServer) {
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
    self
      .video_revision_marks
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self
      .stream_audio_counts
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.user_volumes.lock().expect("server session lock poisoned").clear();
    self.bump_revision();
  }

  pub fn clear(&self) {
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
    self
      .video_revision_marks
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self
      .stream_audio_counts
      .lock()
      .expect("server session lock poisoned")
      .clear();
    self.user_volumes.lock().expect("server session lock poisoned").clear();
    self.bump_revision();
  }

  pub fn disconnect(&self) {
    logger::log("[session] disconnect requested by client");
    if let Some(server) = self.server() {
      server.disconnect();
    }
    self.clear();
  }

  pub fn disconnect_for_shutdown(&self) {
    logger::log("[session] disconnect requested for shutdown");
    self.stop_voice();
    self.stop_video_broadcast();
    if let Some(server) = self.server() {
      server.disconnect();
    }
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

  pub fn video_frame(&self, user_id: UserId) -> Option<ImageData> {
    self
      .video_frames
      .lock()
      .expect("server session lock poisoned")
      .get(&user_id)
      .map(VideoFrameImage::image_data)
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

  pub fn set_push_to_talk_active(&self, active: bool) {
    if let Some(engine) = self.voice_engine.lock().expect("server session lock poisoned").as_ref() {
      engine.set_push_to_talk_active(active);
    }

    let Some(user_id) = self.info().map(|info| info.user_id) else {
      return;
    };
    let (muted, deafened) = self.local_voice_state().unwrap_or((false, false));
    if active && !muted && !deafened {
      self.set_user_speaking(user_id, true);
    } else if !active {
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
      lobby.selected_channel_id = Some(channel_id);
      lobby.selected_text_channel_id = None;
      lobby.stream_browser_channel_id = None;
      for channel in &mut lobby.channels {
        channel.key_received = false;
      }
      Self::sync_selected_users(&mut lobby);
    }
    self.bump_revision();
  }

  pub fn leave_channel_locally(&self) {
    let local_user_id = self.info().map(|info| info.user_id);
    self.stop_voice();
    self.stop_video_broadcast();

    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      if let Some(channel_id) = lobby.selected_channel_id.take()
        && let Some(user_id) = local_user_id
        && let Some(users) = lobby.users_by_channel.get_mut(&channel_id)
      {
        users.retain(|user| user.user_id != user_id);
      }
      lobby.stream_browser_channel_id = None;
      lobby.users.clear();
      Self::sync_cached_channel_counts(&mut lobby);
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
  }

  pub fn select_text_channel(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.selected_text_channel_id = Some(channel_id);
      lobby.stream_browser_channel_id = None;
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
      }
    }
    self.bump_revision();
  }

  pub fn close_stream_browser(&self) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
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
    let previous_user_id = {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      let previous_user_id = lobby.watching_user_id;
      lobby.watching_user_id = user_id;
      previous_user_id
    };
    if previous_user_id != user_id {
      self.clear_stream_audio(previous_user_id);
    }
    self.retain_video_cache(user_id);
    self.bump_revision();
  }

  fn watching_user_id(&self) -> Option<UserId> {
    self
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .watching_user_id
  }

  fn retain_video_cache(&self, watched_user_id: Option<UserId>) {
    let mut frames = self.video_frames.lock().expect("server session lock poisoned");
    let mut marks = self.video_revision_marks.lock().expect("server session lock poisoned");
    match watched_user_id {
      Some(user_id) => {
        frames.retain(|cached_user_id, _| *cached_user_id == user_id);
        marks.retain(|cached_user_id, _| *cached_user_id == user_id);
      }
      None => {
        frames.clear();
        marks.clear();
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
  }

  pub fn start_voice(&self, settings: AppSettings) -> Result<(), String> {
    let server = self.server().ok_or_else(|| "No connected server.".to_owned())?;
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

  pub fn start_video_broadcast(&self, config: VideoBroadcastConfig) -> Result<(), String> {
    let server = self.server().ok_or_else(|| "No connected server.".to_owned())?;
    let broadcast = VideoBroadcast::start(server, config).map_err(|error| {
      let error = error.to_string();
      logger::log(&format!("[video] VideoBroadcast::start failed: {error}"));
      error
    })?;
    let mut video_broadcast = self.video_broadcast.lock().expect("server session lock poisoned");
    video_broadcast.replace(broadcast);
    logger::log("[video] local broadcaster stored in session");
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
      logger::log("[video] local broadcaster stopped");
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
      return;
    };
    if self.lobby.lock().expect("server session lock poisoned").disconnected {
      return;
    }

    {
      let mut started = self.receiver_started.lock().expect("server session lock poisoned");
      if *started {
        return;
      }
      *started = true;
    }
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.receiver_running = true;
      lobby.last_error = None;
    }
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
          logger::log(&format!("[network] lobby receiver error: {error}"));
          session.mark_lobby_error(error);
          break;
        }
      };
    }

    voice_task.abort();
    video_stop.store(true, Ordering::Relaxed);
    ping_task.abort();
    drop(video_thread);
    *session.receiver_started.lock().expect("server session lock poisoned") = false;
    session
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .receiver_running = false;
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
        Err(ServerError::Protocol(error)) => {
          logger::log(&format!("[voice] ignored malformed audio packet: {error}"));
          continue;
        }
        Err(error) => {
          logger::log(&format!("[voice] voice receiver stopped: {error}"));
          break;
        }
      }
    }
  }

  fn run_video_receiver(&self, server: Arc<Server>, runtime: tokio::runtime::Handle, stop: Arc<AtomicBool>) {
    logger::log("[video] receiver thread started");
    let _dx12_video_surfaces = self.dx12_video_surface_allocator();
    let queue = Arc::new(VideoPacketQueue::new());
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
                logger::log(&format!("[video] ignored malformed video packet: {error}"));
                continue;
              }
              Err(error) => {
                logger::log(&format!("[video] video reader stopped: {error}"));
                break;
              }
            }
          }
          queue.close();
        })
        .ok()
    };
    let mut decoders = HashMap::<UserId, VideoDecoder>::new();
    let mut decoder_failures = HashSet::<(UserId, VideoDecodeConfig)>::new();
    #[cfg(target_os = "windows")]
    let mut dx12_decode_failures = HashSet::<(UserId, VideoDecodeConfig)>::new();
    let mut awaiting_keyframes = HashSet::<UserId>::new();
    let mut received_counts = HashMap::<UserId, u64>::new();
    let mut decoded_counts = HashMap::<UserId, u64>::new();
    let mut last_watched_user = self.watching_user_id();
    let mut batch = Vec::<ForwardedVideoFrame>::with_capacity(MAX_QUEUED_VIDEO_PACKETS);

    while !stop.load(Ordering::Relaxed) {
      let Some(dropped_count) = queue.pop_batch_into(&stop, &mut batch) else {
        break;
      };

      let watched_user = self.watching_user_id();
      if watched_user != last_watched_user {
        decoders.retain(|user_id, _| Some(*user_id) == watched_user);
        decoder_failures.retain(|(user_id, _)| Some(*user_id) == watched_user);
        #[cfg(target_os = "windows")]
        dx12_decode_failures.retain(|(user_id, _)| Some(*user_id) == watched_user);
        awaiting_keyframes.retain(|user_id| Some(*user_id) == watched_user);
        last_watched_user = watched_user;
        logger::log(&format!("[video] watch target changed: {watched_user:?}"));
      }

      if dropped_count > 0 || batch.len() > MAX_DECODE_VIDEO_BATCH {
        let affected_users = batch
          .iter()
          .filter(|packet| Some(packet.sender_id) == watched_user)
          .map(|packet| packet.sender_id)
          .collect::<HashSet<_>>();
        for user_id in &affected_users {
          decoders.remove(user_id);
          awaiting_keyframes.insert(*user_id);
          if let Err(error) = runtime.block_on(server.request_keyframe_stream(*user_id)) {
            logger::log(&format!(
              "[video] failed to request catch-up keyframe for user {user_id}: {error}"
            ));
          }
        }
        logger::log(&format!(
          "[video] dropping stale video backlog: queued={} dropped={} users={}",
          batch.len(),
          dropped_count,
          affected_users.len()
        ));
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
          continue;
        }

        if awaiting_keyframes.contains(&packet.sender_id) {
          if !packet.frame.keyframe {
            continue;
          }
          awaiting_keyframes.remove(&packet.sender_id);
          logger::log(&format!(
            "[video] catch-up keyframe received for user {}: frame={}",
            packet.sender_id, packet.frame.frame_number
          ));
        }

        {
          let received_count = increment_counter(&mut received_counts, packet.sender_id);
          let output = Some(packet_index) == latest_watched_packet_index;
          if should_log_video_count(received_count) {
            logger::log(&format!(
              "[video] received frame #{received_count} from user {}: frame={} codec={:?} size={}x{} keyframe={} output={} bytes={}",
              packet.sender_id,
              packet.frame.frame_number,
              packet.frame.codec,
              packet.frame.width,
              packet.frame.height,
              packet.frame.keyframe,
              output,
              packet.frame.encoded.len()
            ));
          }

          #[cfg(target_os = "windows")]
          if output
            && let Some(surface) =
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
                logger::log(&format!(
                  "[video] decoded DX12 frame #{decoded_count} from user {}: codec={:?} size={}x{} format=Nv12",
                  packet.sender_id, packet.frame.codec, packet.frame.width, packet.frame.height
                ));
              }
              self.handle_dx12_video_frame(
                packet.sender_id,
                packet.frame.codec,
                packet.frame.width,
                packet.frame.height,
                surface,
              );
            } else if should_log_video_count(received_count) {
              logger::log("[video] received frame produced no DX12 decoded output yet");
            }
            continue;
          }

          let output_buffer = if output {
            self.take_video_pixel_buffer(packet.sender_id, packet.frame.width, packet.frame.height)
          } else {
            None
          };
          if let Some(frame) = decode_video_packet(&mut decoders, &mut decoder_failures, packet, output, output_buffer)
          {
            let decoded_count = increment_counter(&mut decoded_counts, frame.sender_id);
            if should_log_video_count(decoded_count) {
              logger::log(&format!(
                "[video] decoded frame #{decoded_count} from user {}: codec={:?} size={}x{} format={:?} bytes={}",
                frame.sender_id,
                frame.codec,
                frame.width,
                frame.height,
                frame.format,
                frame.pixels.len()
              ));
            }
            self.handle_video_frame(frame);
          } else if should_log_video_count(received_count) {
            logger::log("[video] received frame produced no decoded output yet");
          }
        }
      }
    }

    queue.close();
    drop(reader_thread);
    logger::log("[video] receiver thread stopping");
  }

  fn handle_voice_packet(&self, packet: crate::network::protocol::data::ForwardedVoicePacket) -> bool {
    if self.info().is_some_and(|info| info.user_id == packet.sender_id) {
      return false;
    }

    self
      .voice_engine
      .lock()
      .expect("server session lock poisoned")
      .as_mut()
      .map(|engine| engine.push_packet(packet))
      .unwrap_or(true)
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
      logger::log(&format!(
        "[audio] received stream audio #{received_count} from user {}: watched={watched_user_id:?} bytes={}",
        packet.sender_id,
        packet.opus.len()
      ));
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
      logger::log(&format!(
        "[audio] stream audio {} for watched user {}",
        if queued { "queued" } else { "dropped" },
        watched_user_id.unwrap_or_default()
      ));
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
    let mut force_revision = false;
    {
      let mut frames = self.video_frames.lock().expect("server session lock poisoned");
      match frames.get(&frame.sender_id) {
        Some(image)
          if image.is_cpu_image()
            && image.image_data().width() == u32::from(frame.width)
            && image.image_data().height() == u32::from(frame.height)
            && image.image_data().format() == decoded_pixel_format_to_lurq(frame.format) =>
        {
          image.set_cpu_pixels(frame.format, frame.pixels);
        }
        _ => {
          logger::log(&format!(
            "[video] creating streamed image for user {}: {}x{} format={:?}",
            frame.sender_id, frame.width, frame.height, frame.format
          ));
          force_revision = true;
          let image = match frame.format {
            DecodedVideoPixelFormat::Rgba8 => {
              StreamingImage::new_rgba_manual_redraw(frame.pixels, u32::from(frame.width), u32::from(frame.height))
            }
            DecodedVideoPixelFormat::Nv12 => {
              StreamingImage::new_nv12_manual_redraw(frame.pixels, u32::from(frame.width), u32::from(frame.height))
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
  fn dx12_video_surface_for_decode(
    &self,
    user_id: UserId,
    width: u16,
    height: u16,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
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
        logger::log(&format!("[video] failed to allocate DX12 video surface: {error}"));
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
        logger::log(&format!(
          "[video] creating DX12 streamed image for user {sender_id}: {width}x{height} format=Nv12"
        ));
        frames.insert(sender_id, VideoFrameImage::Dx12Surface(surface));
        force_revision = true;
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

    if force_revision || self.should_bump_video_revision(sender_id) {
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
    logger::log(&format!("[network] marking lobby disconnected: {message}"));
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.receiver_running = false;
      lobby.disconnected = true;
      lobby.last_error = Some(message);
    }
    self.bump_revision();
  }

  fn apply_server_message(&self, message: S2C) {
    let local_user_id = self.info().map(|info| info.user_id);
    let local_voice_state = *self.local_voice_fallback.lock().expect("server session lock poisoned");
    let mut local_voice_update = None;
    let mut stop_local_voice = false;
    let mut clear_speaking_user = None;
    let mut lobby = self.lobby.lock().expect("server session lock poisoned");

    match message {
      S2C::ChannelList(list) => {
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
        let selected = lobby.selected_text_channel_id;
        lobby.text_channels = channels.into_iter().map(LobbyTextChannel::from).collect();
        lobby.text_channels.sort_by_key(|channel| channel.sort_order);
        let channel_ids: Vec<_> = lobby.text_channels.iter().map(|channel| channel.id).collect();
        lobby
          .chat_messages_by_channel
          .retain(|channel_id, _| channel_ids.contains(channel_id));
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
        Self::merge_chat_messages(
          lobby.chat_messages_by_channel.entry(message.channel_id).or_default(),
          [message],
        );
      }
      S2C::ChatHistoryResp(response) => {
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
        if let Some(messages) = lobby.chat_messages_by_channel.get_mut(&channel_id) {
          messages.retain(|message| message.id != message_id);
        }
      }
      S2C::ChannelUserList(list) => {
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
        for (channel_id, users) in &mut lobby.users_by_channel {
          if *channel_id != joined.channel_id {
            users.retain(|user| user.user_id != joined.user_id);
          }
        }
        let users = lobby.users_by_channel.entry(joined.channel_id).or_default();
        let inserted = if users.iter().any(|user| user.user_id == joined.user_id) {
          false
        } else {
          let local = local_user_id == Some(joined.user_id);
          users.push(LobbyUser {
            user_id: joined.user_id,
            username: joined.username,
            role: joined.role,
            muted: local && local_voice_state.0,
            deafened: local && local_voice_state.1,
            speaking: false,
          });
          true
        };
        if lobby.selected_channel_id == Some(joined.channel_id) {
          Self::sync_selected_users(&mut lobby);
        }
        if inserted {
          Self::sync_cached_channel_counts(&mut lobby);
        }
      }
      S2C::UserLeftChannel(left) => {
        let local_left = local_user_id == Some(left.user_id);
        for users in lobby.users_by_channel.values_mut() {
          users.retain(|user| user.user_id != left.user_id);
        }
        if local_left {
          stop_local_voice = true;
        }
        clear_speaking_user = Some(left.user_id);
        lobby.screen_shares.retain(|share| share.sharer_user_id != left.user_id);
        self.clear_video_cache_for_user(left.user_id);
        if lobby.watching_user_id == Some(left.user_id) {
          lobby.watching_user_id = None;
        }
        if local_left && lobby.selected_channel_id == Some(left.channel_id) {
          lobby.selected_channel_id = None;
          lobby.stream_browser_channel_id = None;
          lobby.users.clear();
        } else if lobby.selected_channel_id == Some(left.channel_id) {
          Self::sync_selected_users(&mut lobby);
        }
        Self::sync_cached_channel_counts(&mut lobby);
      }
      S2C::UserVoiceState(state) => {
        if local_user_id == Some(state.user_id) {
          local_voice_update = Some((state.muted, state.deafened));
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
        if let Some(channel) = lobby.channels.iter_mut().find(|channel| channel.id == key.channel_id) {
          channel.key_received = true;
        }
      }
      S2C::ScreenShareStarted(started) => {
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
      }
      S2C::ScreenShareStopped { sharer_user_id } => {
        lobby
          .screen_shares
          .retain(|share| share.sharer_user_id != sharer_user_id);
        self.clear_video_cache_for_user(sharer_user_id);
        if lobby.watching_user_id == Some(sharer_user_id) {
          lobby.watching_user_id = None;
        }
      }
      S2C::ScreenShareDenied { reason } | S2C::ServerError { message: reason } => {
        lobby.last_error = Some(reason);
      }
      S2C::AdminResult(result) => {
        lobby.last_error = if result.success { None } else { Some(result.message) };
      }
      S2C::AuthResponse(_)
      | S2C::ChatFileUploadResp(_)
      | S2C::ChatFileReady { .. }
      | S2C::ChatSearchResp { .. }
      | S2C::ChatPinnedResp { .. } => {}
    }

    drop(lobby);
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

fn decoded_pixel_format_to_lurq(format: DecodedVideoPixelFormat) -> lurq::images::ImagePixelFormat {
  match format {
    DecodedVideoPixelFormat::Rgba8 => lurq::images::ImagePixelFormat::Rgba8,
    DecodedVideoPixelFormat::Nv12 => lurq::images::ImagePixelFormat::Nv12,
  }
}
