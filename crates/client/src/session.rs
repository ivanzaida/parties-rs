#[cfg(target_os = "windows")]
use std::collections::{HashMap, VecDeque};
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::{Duration, Instant},
};

use lurq::{core::Signal, images::ImageData};
use parking_lot::Mutex;

use crate::{
  network::{
    protocol::{
      ChannelId, S2C, UserId,
      data::{ForwardedStreamAudioPacket, VideoControl, VideoFrame},
    },
    server::Server,
  },
  services::{
    notifications::NotificationSound,
    profiler,
    video::{DecodedVideoFrame, VideoBroadcastConfig, VideoDecodeConfig, VideoFrameLoopback},
    voice::{LocalSpeakingActivityCallback, LocalVoiceCallback},
  },
  storage::AppSettings,
};

pub mod chat_commands;
pub mod chat_history;
mod connection;
mod lobby;
mod speaking;
mod video;
mod video_sink;
mod video_stream;
mod voice_runtime;
mod voice_state;

pub use connection::{ConnectedServer, ConnectedServerInfo, TofuWarning};
pub use lobby::{
  DEBUG_CHAT_CHANNEL_ID, LobbyChannel, LobbyConnectionWarning, LobbyConnectionWarningKind, LobbyScreenShare,
  LobbyState, LobbyTextChannel, LobbyUser,
};
pub use video::VideoReceiverDebugSnapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoStreamError {
  pub title: String,
  pub message: String,
  pub i18n_key: Option<&'static str>,
}

#[derive(Clone)]
pub struct ServerSession {
  connection: Arc<connection::ConnectionRuntime>,
  lobby: Arc<Mutex<LobbyState>>,
  speaking: Arc<speaking::SpeakingTracker>,
  voice: Arc<voice_runtime::VoiceRuntime>,
  voice_state: Arc<voice_state::VoiceState>,
  voice_settings: Arc<Mutex<AppSettings>>,
  streams: Arc<video_stream::StreamRuntime>,
  video_sink: Arc<video_sink::VideoFrameSink>,
  video_hardware_decoding: Arc<AtomicBool>,
  revision: Signal<u64>,
}

impl Default for ServerSession {
  fn default() -> Self {
    let lobby = Arc::new(Mutex::new(LobbyState::default()));
    let revision = Signal::new(0);
    Self {
      connection: Arc::new(connection::ConnectionRuntime::new()),
      lobby: lobby.clone(),
      speaking: Arc::new(speaking::SpeakingTracker::new()),
      voice: Arc::new(voice_runtime::VoiceRuntime::new()),
      voice_state: Arc::new(voice_state::VoiceState::new()),
      voice_settings: Arc::new(Mutex::new(AppSettings::default())),
      streams: Arc::new(video_stream::StreamRuntime::new()),
      video_sink: Arc::new(video_sink::VideoFrameSink::new(lobby, revision.clone())),
      video_hardware_decoding: Arc::new(AtomicBool::new(true)),
      revision,
    }
  }
}

#[allow(dead_code)]
impl ServerSession {
  #[cfg(target_os = "windows")]
  pub fn with_dx12_video_surface_allocator(
    dx12_video_surfaces: lurq::app::dx12_render::Dx12VideoSurfaceAllocator,
  ) -> Self {
    let mut session = Self::default();
    session.video_sink = Arc::new(video_sink::VideoFrameSink::with_dx12_video_surface_allocator(
      session.lobby.clone(),
      session.revision.clone(),
      dx12_video_surfaces,
    ));
    session
  }

  fn reset_video_packet_queue(&self) -> Arc<video::VideoPacketQueue> {
    self.streams.reset_packet_queue()
  }

  fn set_video_receiver_debug_snapshot(&self, snapshot: VideoReceiverDebugSnapshot) {
    self.streams.set_receiver_debug_snapshot(snapshot);
  }

  pub fn video_receiver_debug_snapshot(&self) -> VideoReceiverDebugSnapshot {
    self.streams.receiver_debug_snapshot()
  }

  fn push_local_video_frame(&self, sender_id: UserId, frame: VideoFrame) {
    if self.watching_user_id() != Some(sender_id) {
      return;
    }
    self.streams.push_loopback_frame(sender_id, frame);
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
    self.connection.set_connected(connected);
    self.stop_voice();
    self.stop_video_broadcast();
    *self.lobby.lock() = LobbyState::default();
    self.voice_state.reset_local();
    self.speaking.clear_all();
    self.video_sink.clear_all();
    self.voice.clear_counts();
    self.voice.clear_volumes();
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
    self.connection.clear();
    self.stop_voice();
    self.stop_video_broadcast();
    *self.lobby.lock() = LobbyState::default();
    self.voice_state.reset_local();
    self.speaking.clear_all();
    self.video_sink.clear_all();
    self.voice.clear_counts();
    self.voice.clear_volumes();
    self.streams.clear_pending_reconnect_watch();
    self.bump_revision();
  }

  pub fn disconnect(&self) {
    tracing::info!(target: "session", "[session] disconnect requested by client");
    let was_in_voice = self.lobby.lock().selected_channel_id.is_some();
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
    self.connection.request_shutdown();
    self.stop_voice();
    self.stop_video_broadcast();
    if let Some(server) = self.server() {
      server.disconnect();
    }
  }

  pub fn shutdown_requested(&self) -> bool {
    self.connection.shutdown_requested()
  }

  pub fn info(&self) -> Option<ConnectedServerInfo> {
    self.connection.info()
  }

  pub fn server(&self) -> Option<Arc<Server>> {
    self.connection.server()
  }

  fn mark_network_activity(&self) {
    self.connection.mark_network_activity();
  }

  fn network_idle_for(&self, now: Instant) -> Duration {
    self.connection.network_idle_for(now)
  }

  fn connection_debug_context(&self) -> String {
    let now = Instant::now();
    let network_idle_ms = self.network_idle_for(now).as_millis();
    let pending_ping_age_ms = self.connection.pending_ping_age_ms(now);
    let (selected_channel_id, selected_users, disconnected, receiver_running) = {
      let lobby = self.lobby.lock();
      (
        lobby.selected_channel_id,
        lobby.users.len(),
        lobby.disconnected,
        lobby.receiver_running,
      )
    };
    let (muted, deafened) = self.local_voice_state().unwrap_or((false, false));
    let (voice_engine_present, captures_voice) = self.voice.engine_status();
    let info = self.info();
    format!(
      "server={} address={} local_user={:?} channel={selected_channel_id:?} channel_users={selected_users} muted={muted} deafened={deafened} voice_engine={voice_engine_present} captures_voice={captures_voice} disconnected={disconnected} receiver_running={receiver_running} pending_ping_age_ms={pending_ping_age_ms:?} network_idle_ms={network_idle_ms} shutdown={}",
      info.as_ref().map(|info| info.server_name.as_str()).unwrap_or("<none>"),
      info.as_ref().map(|info| info.address.as_str()).unwrap_or("<none>"),
      info.as_ref().map(|info| info.user_id),
      self.connection.shutdown_requested()
    )
  }

  fn set_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String) {
    let mut should_bump = false;
    {
      let mut lobby = self.lobby.lock();
      if lobby.disconnected || self.connection.shutdown_requested() {
        return;
      }
      let warning = LobbyConnectionWarning { kind, message };
      if lobby.connection_warning.as_ref() != Some(&warning) {
        lobby.connection_warning = Some(warning);
        should_bump = true;
      }
    }
    if should_bump {
      self.bump_revision();
    }
  }

  fn stop_lobby_receivers(&self) {
    self.connection.stop_receivers();
  }

  pub fn video_frame(&self, user_id: UserId) -> Option<ImageData> {
    self.video_sink.image_data(user_id)
  }

  pub fn video_error(&self, user_id: UserId) -> Option<VideoStreamError> {
    self.video_sink.error(user_id)
  }

  pub fn local_voice_state(&self) -> Option<(bool, bool)> {
    self.info()?;
    Some(self.voice_state.local_voice_state())
  }

  pub fn set_local_voice_state(&self, muted: bool, deafened: bool) {
    self.voice_state.set_local_voice_state(muted, deafened);
    self.voice.set_voice_state(muted, deafened);

    let Some(user_id) = self.info().map(|info| info.user_id) else {
      self.bump_revision();
      return;
    };

    if muted || deafened {
      self.clear_user_speaking(user_id);
    }

    {
      let mut lobby = self.lobby.lock();
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
    self.voice.set_voice_activation_threshold(value);
  }

  pub fn set_voice_normalization(&self, value: bool) {
    self.voice.set_voice_normalization(value);
  }

  pub fn set_voice_normalization_target_level(&self, value: i32) {
    self.voice.set_voice_normalization_target_level(value);
  }

  pub fn set_push_to_talk_release_delay_ms(&self, value: i32) {
    self.voice.set_push_to_talk_release_delay_ms(value);
  }

  pub fn set_notification_audio_settings(&self, settings: &AppSettings) {
    self.voice_state.set_notification_audio_settings(settings);
    let mut voice_settings = self.voice_settings.lock();
    voice_settings
      .audio_output_device
      .clone_from(&settings.audio_output_device);
    voice_settings.notification_volume = settings.notification_volume;
    voice_settings
      .notification_sound_overrides
      .clone_from(&settings.notification_sound_overrides);
  }

  pub fn set_video_hardware_decoding(&self, enabled: bool) {
    self.video_hardware_decoding.store(enabled, Ordering::Relaxed);
  }

  fn play_notification_sound(&self, sound: NotificationSound) {
    self.voice_state.play_notification_sound(sound);
  }

  pub fn play_voice_join_notification(&self) {
    self.play_notification_sound(NotificationSound::VoiceJoin);
  }

  pub fn queue_voice_join_sound_to_channel(&self, settings: &AppSettings) {
    match self.voice.queue_outgoing_voice_join_sound(
      &settings.notification_sound_overrides,
      settings.notification_volume,
      self.local_intro_speaking_callback(),
    ) {
      Ok(true) => {
        tracing::info!(target: "voice", "[voice] queued outgoing voice join sound");
      }
      Ok(false) => tracing::debug!(
        target: "voice",
        "[voice] skipped outgoing voice join sound: no selected sound or no active voice capture"
      ),
      Err(error) => tracing::warn!(target: "voice", "[voice] failed to queue outgoing voice join sound: {error}"),
    }
  }

  pub fn play_voice_leave_notification(&self) {
    self.play_notification_sound(NotificationSound::VoiceLeave);
  }

  pub fn play_local_voice_state_change_notification(&self) {
    self.play_notification_sound(NotificationSound::ModerationAction);
  }

  pub fn set_push_to_talk_active(&self, active: bool) {
    let release_delay_ms = self.voice.set_push_to_talk_active(active);

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
    self.voice.user_volume(user_id)
  }

  pub fn set_user_volume(&self, user_id: UserId, volume: i32) {
    self.voice.set_user_volume(user_id, volume);
  }

  pub fn user_normalization(&self, user_id: UserId) -> bool {
    self.voice.user_normalization(user_id)
  }

  pub fn set_user_normalization(&self, user_id: UserId, enabled: bool) {
    self.voice.set_user_normalization(user_id, enabled);
  }

  pub fn restart_audio_receiver(&self, user_id: UserId) -> bool {
    let restarted = self.voice.restart_audio_receiver(user_id);
    tracing::info!(
      target: "audio::decode",
      "[audio:decode] restart audio receiver requested: user={} restarted={} {}",
      user_id,
      restarted,
      self.connection_debug_context()
    );
    restarted
  }

  pub fn stream_volume(&self, user_id: UserId) -> i32 {
    self.voice.stream_volume(user_id)
  }

  pub fn set_stream_volume(&self, user_id: UserId, volume: i32) {
    self.voice.set_stream_volume(user_id, volume);
  }

  pub fn remember_muted_before_deafen(&self, muted: bool) {
    self.voice_state.remember_muted_before_deafen(muted);
  }

  pub fn take_muted_before_deafen(&self) -> Option<bool> {
    self.voice_state.take_muted_before_deafen()
  }

  pub fn mark_user_speaking(&self, user_id: UserId) {
    self.speaking.mark_user_speaking(self.clone(), user_id);
  }

  fn start_user_speaking_activity(&self, user_id: UserId) -> u64 {
    self.speaking.start_user_speaking_activity(self.clone(), user_id)
  }

  fn stop_user_speaking_activity(&self, user_id: UserId, token: u64) {
    self.speaking.stop_user_speaking_activity(self.clone(), user_id, token);
  }

  fn clear_user_speaking(&self, user_id: UserId) {
    self.speaking.clear_user_speaking(self.clone(), user_id);
  }

  fn set_user_speaking(&self, user_id: UserId, speaking: bool) {
    {
      let mut lobby = self.lobby.lock();
      for users in lobby.users_by_channel.values_mut() {
        if let Some(user) = users.iter_mut().find(|user| user.user_id == user_id) {
          user.speaking = speaking;
        }
      }
      lobby::sync_selected_users(&mut lobby);
    }
    self.bump_revision();
  }

  pub fn set_tofu_warning(&self, warning: TofuWarning) {
    self.connection.set_tofu_warning(warning);
  }

  pub fn clear_tofu_warning(&self) {
    self.connection.clear_tofu_warning();
  }

  pub fn tofu_warning(&self) -> Option<TofuWarning> {
    self.connection.tofu_warning()
  }

  pub fn lobby(&self) -> LobbyState {
    self.lobby.lock().clone()
  }

  pub fn revision(&self) -> Signal<u64> {
    self.revision.clone()
  }

  pub fn refresh_lobby(&self) {
    self.bump_revision();
  }

  fn bump_revision(&self) {
    self.revision.update(|revision| *revision = revision.wrapping_add(1));
  }

  pub fn select_channel(&self, channel_id: ChannelId) {
    if let Some(user_id) = self.info().map(|info| info.user_id) {
      self.speaking.forget_user(user_id);
    }
    {
      let mut lobby = self.lobby.lock();
      lobby::select_channel(&mut lobby, channel_id);
    }
    self.bump_revision();
  }

  pub fn leave_channel_locally(&self) {
    let local_user_id = self.info().map(|info| info.user_id);
    self.stop_voice();
    self.stop_video_broadcast();
    let effects = {
      let mut lobby = self.lobby.lock();
      lobby::leave_channel_locally(&mut lobby, local_user_id)
    };

    if let Some(previous_user_id) = effects.watching_change {
      self.finish_watching_user_change(previous_user_id, None);
    }
    if let Some(user_id) = effects.clear_video_cache_user {
      self.clear_video_cache_for_user(user_id);
    }

    if let Some(user_id) = effects.forget_speaking_user {
      self.speaking.forget_user(user_id);
    }

    self.bump_revision();
    if effects.left_voice {
      self.play_voice_leave_notification();
    }
  }

  pub fn select_text_channel(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock();
      lobby::select_text_channel(&mut lobby, channel_id);
    }
    self.bump_revision();
  }

  pub fn select_debug_chat(&self) {
    {
      let mut lobby = self.lobby.lock();
      lobby::select_debug_chat(&mut lobby);
    }
    self.bump_revision();
  }

  pub fn open_stream_browser(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock();
      lobby::open_stream_browser(&mut lobby, channel_id);
    }
    self.bump_revision();
  }

  pub fn close_stream_browser(&self) {
    {
      let mut lobby = self.lobby.lock();
      lobby::close_stream_browser(&mut lobby);
    }
    self.bump_revision();
  }

  pub fn begin_chat_history_request(&self, channel_id: ChannelId, before_id: u64) -> bool {
    let should_begin = {
      let mut lobby = self.lobby.lock();
      lobby::begin_chat_history_request(&mut lobby, channel_id, before_id)
    };

    if should_begin {
      self.bump_revision();
    }

    should_begin
  }

  pub fn finish_chat_history_request(&self, channel_id: ChannelId, has_more: bool) {
    {
      let mut lobby = self.lobby.lock();
      lobby::finish_chat_history_request(&mut lobby, channel_id, has_more);
    }
    self.bump_revision();
  }

  pub fn set_watching_user(&self, user_id: Option<UserId>) {
    self.streams.clear_pending_reconnect_watch();
    let (previous_user_id, changed, view_changed) = {
      let mut lobby = self.lobby.lock();
      let previous_text_channel_id = lobby.selected_text_channel_id;
      let previous_debug_chat_selected = lobby.debug_chat_selected;
      let previous_stream_browser_channel_id = lobby.stream_browser_channel_id;
      let (previous_user_id, changed) = lobby::set_watching_user(&mut lobby, user_id);
      (
        previous_user_id,
        changed,
        previous_text_channel_id != lobby.selected_text_channel_id
          || previous_debug_chat_selected != lobby.debug_chat_selected
          || previous_stream_browser_channel_id != lobby.stream_browser_channel_id,
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
    self.streams.has_pending_reconnect_watch()
  }

  pub async fn restore_pending_reconnect_watch(&self, settings: AppSettings, timeout: Duration) {
    self
      .streams
      .restore_pending_reconnect_watch(self.clone(), settings, timeout)
      .await;
  }

  fn reconnect_watch_target_available(&self, user_id: UserId) -> bool {
    let lobby = self.lobby.lock();
    lobby.screen_shares.iter().any(|share| share.sharer_user_id == user_id)
      && lobby::user_in_selected_voice_channel(&lobby, user_id)
  }

  fn finish_watching_user_change(&self, previous_user_id: Option<UserId>, user_id: Option<UserId>) {
    self.clear_stream_audio(previous_user_id);
    self.retain_video_cache(user_id);
  }

  fn watching_user_id(&self) -> Option<UserId> {
    self.lobby.lock().watching_user_id
  }

  fn video_decode_config_for_share(&self, user_id: UserId) -> Option<VideoDecodeConfig> {
    let lobby = self.lobby.lock();
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
      hardware_decoding: self.video_hardware_decoding.load(Ordering::Relaxed),
    })
  }

  fn retain_video_cache(&self, watched_user_id: Option<UserId>) {
    self.video_sink.retain_user(watched_user_id);
  }

  fn clear_video_cache_for_user(&self, user_id: UserId) {
    self.video_sink.clear_user(user_id);
  }

  fn set_video_error(&self, user_id: UserId, error: VideoStreamError) {
    self.video_sink.set_error(user_id, error);
  }

  fn clear_video_error(&self, user_id: UserId) {
    self.video_sink.clear_error(user_id);
  }

  pub fn start_voice(&self, settings: AppSettings, no_connected_server: &str) -> Result<(), String> {
    *self.voice_settings.lock() = settings.clone();
    let server = self.server().ok_or_else(|| no_connected_server.to_owned())?;
    let (muted, deafened) = self.local_voice_state().unwrap_or((false, false));
    tracing::info!(
      target: "voice",
      "[voice] starting local voice engine: muted={muted} deafened={deafened} {}",
      self.connection_debug_context()
    );
    let captures_voice = if muted || deafened {
      self.voice.ensure_stream_playback(settings, deafened)?;
      false
    } else {
      let on_local_voice = self.local_voice_callback();
      self
        .voice
        .start_capture(server, settings, muted, deafened, on_local_voice)?
    };
    tracing::info!(
      target: "voice",
      "[voice] local voice engine started: captures_voice={} {}",
      captures_voice,
      self.connection_debug_context()
    );
    Ok(())
  }

  pub fn ensure_voice_capture_started(&self, no_connected_server: &str) -> Result<(), String> {
    if self.voice.voice_active() {
      return Ok(());
    }

    let server = self.server().ok_or_else(|| no_connected_server.to_owned())?;
    let settings = self.voice_settings.lock().clone();
    let (muted, deafened) = self.local_voice_state().unwrap_or((false, false));
    if muted || deafened {
      return Ok(());
    }

    tracing::info!(
      target: "voice",
      "[voice] starting deferred local voice capture after unmute: {}",
      self.connection_debug_context()
    );
    let captures_voice = self
      .voice
      .start_capture(server, settings, muted, deafened, self.local_voice_callback())?;
    tracing::info!(
      target: "voice",
      "[voice] deferred local voice capture started: captures_voice={} {}",
      captures_voice,
      self.connection_debug_context()
    );
    Ok(())
  }

  pub fn ensure_stream_audio_playback(&self, settings: AppSettings) -> Result<(), String> {
    *self.voice_settings.lock() = settings.clone();
    if self.voice.has_engine() {
      return Ok(());
    }

    let (_, deafened) = self.local_voice_state().unwrap_or((false, false));
    tracing::info!(
      target: "voice",
      "[voice] starting stream audio playback engine: deafened={deafened} {}",
      self.connection_debug_context()
    );
    if self.voice.ensure_stream_playback(settings, deafened)? {
      tracing::info!(
        target: "voice",
        "[voice] stream audio playback engine started: {}",
        self.connection_debug_context()
      );
    }
    Ok(())
  }

  pub fn voice_active(&self) -> bool {
    self.voice.voice_active()
  }

  pub fn voice_engine_status(&self) -> (bool, bool) {
    self.voice.engine_status()
  }

  pub fn voice_audio_debug_counts(&self, user_id: UserId) -> (u64, u64, Option<std::time::SystemTime>) {
    let counts = self.voice.voice_audio_debug_counts(user_id);
    (counts.received, counts.queued, counts.last_played_packet_at)
  }

  pub fn stream_audio_debug_counts(&self, user_id: UserId) -> (u64, u64, Option<std::time::SystemTime>) {
    let counts = self.voice.stream_audio_debug_counts(user_id);
    (counts.received, counts.queued, counts.last_played_packet_at)
  }

  pub fn stop_voice(&self) {
    let stopped = self.voice.stop();
    if stopped {
      tracing::info!(
        target: "voice",
        "[voice] local voice engine stopped: {}",
        self.connection_debug_context()
      );
    }
  }

  pub fn start_video_broadcast(&self, config: VideoBroadcastConfig, no_connected_server: &str) -> Result<(), String> {
    let server = self.server().ok_or_else(|| no_connected_server.to_owned())?;
    let local_user_id = self.info().ok_or_else(|| no_connected_server.to_owned())?.user_id;
    let backend = self
      .streams
      .start_broadcast(server, config, self.local_video_loopback(local_user_id))
      .map_err(|error| {
        let error = error.to_string();
        tracing::error!(target: "video::encode", "[video:encode] VideoBroadcast::start failed: {error}");
        error
      })?;
    tracing::info!(target: "video::encode",
      "[video:encode] local broadcast backend selected: backend={}",
      video::native_video_backend_label(backend)
    );
    tracing::info!(target: "video::encode", "[video:encode] local broadcaster stored in session");
    Ok(())
  }

  pub fn stop_video_broadcast(&self) {
    let stopped = self.streams.stop_broadcast();
    if stopped {
      tracing::info!(target: "video::encode", "[video:encode] local broadcaster stopped");
    }
  }

  pub fn video_broadcast_active(&self) -> bool {
    self.streams.has_broadcast()
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

  fn local_intro_speaking_callback(&self) -> LocalSpeakingActivityCallback {
    let session = self.clone();
    let local_user_id = self.info().map(|info| info.user_id);
    let active_token = Arc::new(Mutex::new(None));

    Arc::new(move |active| {
      let Some(user_id) = local_user_id else {
        return;
      };
      if active {
        let token = session.start_user_speaking_activity(user_id);
        *active_token.lock() = Some(token);
      } else if let Some(token) = active_token.lock().take() {
        session.stop_user_speaking_activity(user_id, token);
      }
    })
  }

  pub async fn run_lobby_receiver(&self) {
    connection::run_lobby_receiver(self.clone()).await;
  }

  fn handle_voice_packet(&self, packet: crate::network::protocol::data::ForwardedVoicePacket) -> bool {
    if self.info().is_some_and(|info| info.user_id == packet.sender_id) {
      return false;
    }

    self.voice.handle_voice_packet(packet)
  }

  fn handle_video_control_packet(&self, control: VideoControl) {
    if let VideoControl::Pli { user_id } = control
      && self.streams.request_local_keyframe()
    {
      tracing::debug!(target: "video", "[video] local keyframe requested by viewer {user_id}");
    }
  }

  fn handle_stream_audio_packet(&self, packet: ForwardedStreamAudioPacket) {
    if self.info().is_some_and(|info| info.user_id == packet.sender_id) {
      return;
    }
    let watched_user_id = self.watching_user_id();
    self.voice.handle_stream_audio_packet(packet, watched_user_id);
  }

  fn clear_stream_audio(&self, user_id: Option<UserId>) {
    self.voice.clear_stream_audio(user_id);
  }

  fn handle_video_frame(&self, frame: DecodedVideoFrame) {
    let _span = profiler::span("video.render.handle_frame");
    #[cfg(target_os = "macos")]
    let prefer_cpu_frame = self.info().is_some_and(|info| info.user_id == frame.sender_id) && !frame.pixels.is_empty();
    #[cfg(not(target_os = "macos"))]
    let prefer_cpu_frame = false;
    self
      .video_sink
      .present_decoded(frame, self.watching_user_id(), prefer_cpu_frame);
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
    self.video_sink.shared_nv12_planes_surface_for_decode(
      surface_cache,
      user_id,
      width,
      height,
      y_shared_handle,
      uv_shared_handle,
    )
  }

  #[cfg(target_os = "windows")]
  fn dx12_video_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, u16, u16), VecDeque<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>>,
    user_id: UserId,
    width: u16,
    height: u16,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
    self
      .video_sink
      .dx12_surface_for_decode(surface_cache, user_id, width, height)
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
    self
      .video_sink
      .present_dx12_frame(sender_id, codec, width, height, surface);
  }

  fn take_video_pixel_buffer(&self, user_id: UserId, width: u16, height: u16) -> Option<Vec<u8>> {
    self.video_sink.take_pixel_buffer(user_id, width, height)
  }

  fn has_video_frame(&self, user_id: UserId, width: u16, height: u16) -> bool {
    self.video_sink.has_frame(user_id, width, height)
  }

  pub fn mark_lobby_error(&self, message: String) {
    if self.connection.shutdown_requested() {
      tracing::warn!(target: "network", "[network] ignoring network error during shutdown: {message}");
      return;
    }

    let mut watching_change = None;
    let reconnect_watch_user_id;
    {
      let mut lobby = self.lobby.lock();
      if lobby.disconnected {
        tracing::warn!(target: "network", "[network] lobby already disconnected; ignoring additional network error: {message}");
        if lobby.last_error.is_none() {
          lobby.last_error = Some(message);
          self.bump_revision();
        }
        return;
      }
      tracing::warn!(target: "network", "[network] marking lobby disconnected and closing transport: {message}");
      reconnect_watch_user_id = lobby.watching_user_id;
      lobby.receiver_running = false;
      lobby.disconnected = true;
      lobby.last_error = Some(message);
      lobby.connection_warning = None;
      lobby.stream_browser_channel_id = None;
      lobby.screen_shares.clear();
      let (previous_user_id, changed) = lobby::set_watching_user(&mut lobby, None);
      if changed {
        watching_change = Some(previous_user_id);
      }
    }
    self.stop_video_broadcast();
    self.streams.set_pending_reconnect_watch(reconnect_watch_user_id);
    if let Some(user_id) = reconnect_watch_user_id {
      tracing::info!(target: "video", "[video] saved watched stream target for reconnect: user={user_id}");
    }
    if let Some(previous_user_id) = watching_change {
      self.finish_watching_user_change(previous_user_id, None);
    }
    if let Some(server) = self.server() {
      server.disconnect();
    }
    self.bump_revision();
  }

  pub fn set_lobby_error_notice(&self, message: impl Into<String>) {
    let message = message.into();
    tracing::warn!(target: "lobby", "[lobby] notice: {message}");
    {
      let mut lobby = self.lobby.lock();
      lobby.last_error = Some(message);
    }
    self.bump_revision();
  }

  pub fn push_debug_chat_message(&self, message: impl Into<String>) {
    let message = message.into();
    tracing::warn!(target: "debug", "[debug-chat] {message}");
    {
      let mut lobby = self.lobby.lock();
      lobby::push_debug_chat_message(&mut lobby, message);
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
    let local_voice_state = self.voice_state.local_voice_state();
    let pending_keepalive_ping = if matches!(message, S2C::KeepalivePong) {
      self.connection.take_pending_keepalive_ping()
    } else {
      None
    };
    let context = lobby::ServerMessageContext {
      local_user_id,
      local_display_name,
      local_voice_state,
      pending_keepalive_ping,
    };
    let mut lobby = self.lobby.lock();
    let effects = lobby::apply_server_message(&mut lobby, message, context);
    drop(lobby);

    for user_id in effects.clear_video_cache_users {
      self.clear_video_cache_for_user(user_id);
    }
    if let Some(role) = effects.current_role_update {
      self.connection.update_current_role(local_user_id, role);
    }
    if let Some(previous_user_id) = effects.watching_change {
      self.finish_watching_user_change(previous_user_id, None);
    }
    if let Some(sound) = effects.notification_sound {
      self.play_notification_sound(sound);
    }
    if let Some(state) = effects.local_voice_update {
      self.voice_state.set_local_voice_state(state.0, state.1);
    }
    if let Some(user_id) = effects.clear_speaking_user {
      self.speaking.forget_user(user_id);
    }
    if effects.stop_local_voice {
      self.stop_voice();
      self.stop_video_broadcast();
      if let Some(user_id) = local_user_id {
        self.speaking.forget_user(user_id);
      }
    }
    self.bump_revision();
  }
}

impl connection::ConnectionSession for ServerSession {
  fn connected_server(&self) -> Option<Arc<Server>> {
    self.connection.server()
  }

  fn is_shutdown_requested(&self) -> bool {
    self.connection.shutdown_requested()
  }

  fn lobby_disconnected(&self) -> bool {
    self.lobby.lock().disconnected
  }

  fn try_begin_lobby_receiver(&self) -> bool {
    if !self.connection.try_begin_receiver() {
      return false;
    }
    {
      let mut lobby = self.lobby.lock();
      lobby.receiver_running = true;
      lobby.last_error = None;
    }
    true
  }

  fn set_video_receiver_stop(&self, stop: Option<Arc<AtomicBool>>) {
    self.connection.set_receiver_stop(stop);
  }

  fn finish_lobby_receiver(&self) {
    self.connection.finish_receiver();
    self.lobby.lock().receiver_running = false;
  }

  fn bump_connection_revision(&self) {
    ServerSession::bump_revision(self);
  }

  fn mark_connection_network_activity(&self) {
    self.connection.mark_network_activity();
  }

  fn network_idle_for(&self, now: Instant) -> Duration {
    self.connection.network_idle_for(now)
  }

  fn pending_keepalive_timed_out(&self, now: Instant, timeout: Duration) -> bool {
    self.connection.pending_keepalive_timed_out(now, timeout)
  }

  fn set_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String) {
    ServerSession::set_connection_warning(self, kind, message);
  }

  fn mark_lobby_error(&self, message: String) {
    ServerSession::mark_lobby_error(self, message);
  }

  fn apply_server_message(&self, message: S2C) {
    ServerSession::apply_server_message(self, message);
  }
}

impl speaking::SpeakingSession for ServerSession {
  fn set_user_speaking(&self, user_id: UserId, speaking: bool) {
    ServerSession::set_user_speaking(self, user_id, speaking);
  }
}

impl video_stream::StreamWatchSession for ServerSession {
  fn server(&self) -> Option<Arc<Server>> {
    ServerSession::server(self)
  }

  fn reconnect_watch_target_available(&self, user_id: UserId) -> bool {
    ServerSession::reconnect_watch_target_available(self, user_id)
  }

  fn set_watching_user(&self, user_id: Option<UserId>) {
    ServerSession::set_watching_user(self, user_id);
  }

  fn ensure_stream_audio_playback(&self, settings: AppSettings) -> Result<(), String> {
    ServerSession::ensure_stream_audio_playback(self, settings)
  }
}

impl video::VideoReceiverSession for ServerSession {
  fn mark_video_network_activity(&self) {
    ServerSession::mark_network_activity(self);
  }

  fn reset_video_packet_queue(&self) -> Arc<video::VideoPacketQueue> {
    ServerSession::reset_video_packet_queue(self)
  }

  fn handle_video_control_packet(&self, control: VideoControl) {
    ServerSession::handle_video_control_packet(self, control);
  }

  fn set_video_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String) {
    ServerSession::set_connection_warning(self, kind, message);
  }

  fn set_video_receiver_debug_snapshot(&self, snapshot: VideoReceiverDebugSnapshot) {
    ServerSession::set_video_receiver_debug_snapshot(self, snapshot);
  }

  fn watching_user_id(&self) -> Option<UserId> {
    ServerSession::watching_user_id(self)
  }

  fn video_decode_config_for_share(&self, user_id: UserId) -> Option<VideoDecodeConfig> {
    ServerSession::video_decode_config_for_share(self, user_id)
  }

  fn set_video_error(&self, user_id: UserId, error: VideoStreamError) {
    ServerSession::set_video_error(self, user_id, error);
  }

  fn clear_video_error(&self, user_id: UserId) {
    ServerSession::clear_video_error(self, user_id);
  }

  fn present_video_frame(&self, frame: DecodedVideoFrame) {
    ServerSession::handle_video_frame(self, frame);
  }

  fn take_video_pixel_buffer(&self, user_id: UserId, width: u16, height: u16) -> Option<Vec<u8>> {
    ServerSession::take_video_pixel_buffer(self, user_id, width, height)
  }

  fn has_video_frame(&self, user_id: UserId, width: u16, height: u16) -> bool {
    ServerSession::has_video_frame(self, user_id, width, height)
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
    ServerSession::shared_nv12_planes_video_surface_for_decode(
      self,
      surface_cache,
      user_id,
      width,
      height,
      y_shared_handle,
      uv_shared_handle,
    )
  }

  #[cfg(target_os = "windows")]
  fn dx12_video_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, u16, u16), VecDeque<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>>,
    user_id: UserId,
    width: u16,
    height: u16,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
    ServerSession::dx12_video_surface_for_decode(self, surface_cache, user_id, width, height)
  }

  #[cfg(target_os = "windows")]
  fn present_dx12_video_frame(
    &self,
    sender_id: UserId,
    codec: crate::network::protocol::VideoCodecId,
    width: u16,
    height: u16,
    surface: Arc<lurq::app::dx12_render::Dx12Nv12Surface>,
  ) {
    ServerSession::handle_dx12_video_frame(self, sender_id, codec, width, height, surface);
  }
}

impl voice_runtime::VoiceReceiverSession for ServerSession {
  fn connection_debug_context(&self) -> String {
    ServerSession::connection_debug_context(self)
  }

  fn mark_voice_network_activity(&self) {
    ServerSession::mark_network_activity(self);
  }

  fn handle_voice_packet(&self, packet: crate::network::protocol::data::ForwardedVoicePacket) -> bool {
    ServerSession::handle_voice_packet(self, packet)
  }

  fn mark_user_speaking(&self, user_id: UserId) {
    ServerSession::mark_user_speaking(self, user_id);
  }

  fn handle_stream_audio_packet(&self, packet: ForwardedStreamAudioPacket) {
    ServerSession::handle_stream_audio_packet(self, packet);
  }

  fn handle_video_control_packet(&self, control: VideoControl) {
    ServerSession::handle_video_control_packet(self, control);
  }

  fn set_voice_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String) {
    ServerSession::set_connection_warning(self, kind, message);
  }
}
