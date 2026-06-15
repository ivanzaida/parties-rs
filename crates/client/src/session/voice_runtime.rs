use std::{
  collections::{HashMap, HashSet},
  sync::Arc,
  time::{Duration, Instant, SystemTime},
};

use parking_lot::Mutex;

use super::LobbyConnectionWarningKind;
use crate::{
  network::{
    protocol::{
      UserId,
      data::{ForwardedStreamAudioPacket, ForwardedVoicePacket, VideoControl},
    },
    server::{ReceivedAudioPacket, Server, ServerError},
  },
  services::voice::{LocalVoiceCallback, VoiceEngine},
  storage::AppSettings,
};

const DEFAULT_USER_VOLUME: i32 = 100;
const VOICE_RECEIVE_GAP_LOG_AFTER: Duration = Duration::from_secs(3);
const VOICE_RECEIVE_GAP_WARN_AFTER: Duration = Duration::from_secs(10);
const VOICE_SENDER_SILENCE_WARN_AFTER: Duration = Duration::from_secs(15);
const VOICE_SENDER_SILENCE_WARN_REPEAT: Duration = Duration::from_secs(30);

struct VoiceSenderReceiveState {
  sequence: u16,
  last_at: Instant,
  last_silence_warn_at: Option<Instant>,
}

pub(super) trait VoiceReceiverSession: Clone + Send + Sync + 'static {
  fn connection_debug_context(&self) -> String;
  fn mark_voice_network_activity(&self);
  fn handle_voice_packet(&self, packet: ForwardedVoicePacket) -> bool;
  fn mark_user_speaking(&self, user_id: UserId);
  fn handle_stream_audio_packet(&self, packet: ForwardedStreamAudioPacket);
  fn handle_video_control_packet(&self, control: VideoControl);
  fn set_voice_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String);
}

pub(super) async fn run_voice_activity_receiver<S>(session: S, server: Arc<Server>)
where
  S: VoiceReceiverSession,
{
  tracing::info!(
    target: "voice",
    "[voice] voice receiver started: {}",
    session.connection_debug_context()
  );
  let started_at = Instant::now();
  let mut voice_packets = 0_u64;
  let mut stream_packets = 0_u64;
  let mut video_controls = 0_u64;
  let mut malformed_packets = 0_u64;
  let mut last_packet = "none";
  let mut last_voice_sender = None;
  let mut last_voice_sequence = None;
  let mut last_voice_by_sender = HashMap::<UserId, VoiceSenderReceiveState>::new();

  let stop_reason = loop {
    match server.recv_audio().await {
      Ok(ReceivedAudioPacket::Voice(packet)) => {
        session.mark_voice_network_activity();
        let now = Instant::now();
        warn_stale_voice_senders(
          &session,
          &mut last_voice_by_sender,
          now,
          voice_packets,
          Some(packet.sender_id),
        );
        voice_packets = voice_packets.saturating_add(1);
        last_packet = "voice";
        last_voice_sender = Some(packet.sender_id);
        last_voice_sequence = Some(packet.sequence);
        if let Some(previous) = last_voice_by_sender.insert(
          packet.sender_id,
          VoiceSenderReceiveState {
            sequence: packet.sequence,
            last_at: now,
            last_silence_warn_at: None,
          },
        ) {
          let sequence_delta = packet.sequence.wrapping_sub(previous.sequence);
          let gap = now.duration_since(previous.last_at);
          if sequence_delta != 1 {
            tracing::warn!(
              target: "voice",
              "[voice] voice packet sequence gap from user {}: previous_sequence={} current_sequence={} delta={} gap_ms={} total_voice_packets={} {}",
              packet.sender_id,
              previous.sequence,
              packet.sequence,
              sequence_delta,
              gap.as_millis(),
              voice_packets,
              session.connection_debug_context()
            );
          } else if gap >= VOICE_RECEIVE_GAP_WARN_AFTER {
            tracing::warn!(
              target: "voice",
              "[voice] voice packets resumed from user {} after long silence: gap_ms={} previous_sequence={} current_sequence={} delta={} total_voice_packets={} note=\"delta=1 means no packets appear lost; sender likely stopped transmitting or server stopped forwarding\" {}",
              packet.sender_id,
              gap.as_millis(),
              previous.sequence,
              packet.sequence,
              sequence_delta,
              voice_packets,
              session.connection_debug_context()
            );
          } else if gap >= VOICE_RECEIVE_GAP_LOG_AFTER {
            tracing::info!(
              target: "voice",
              "[voice] voice packets resumed from user {} after {}ms: sequence={} total_voice_packets={} note=\"delta=1; no packets appear lost\" {}",
              packet.sender_id,
              gap.as_millis(),
              packet.sequence,
              voice_packets,
              session.connection_debug_context()
            );
          }
        }
        if voice_packets == 1 {
          tracing::info!(
            target: "voice",
            "[voice] first voice packet received: sender={} sequence={} bytes={} {}",
            packet.sender_id,
            packet.sequence,
            packet.opus.len(),
            session.connection_debug_context()
          );
        }
        let speaking = session.handle_voice_packet(packet.clone());
        if speaking {
          session.mark_user_speaking(packet.sender_id);
        }
      }
      Ok(ReceivedAudioPacket::Stream(packet)) => {
        session.mark_voice_network_activity();
        warn_stale_voice_senders(&session, &mut last_voice_by_sender, Instant::now(), voice_packets, None);
        stream_packets = stream_packets.saturating_add(1);
        last_packet = "stream_audio";
        if stream_packets == 1 {
          tracing::info!(
            target: "voice",
            "[voice] first stream audio packet received: sender={} bytes={} {}",
            packet.sender_id,
            packet.opus.len(),
            session.connection_debug_context()
          );
        }
        session.handle_stream_audio_packet(packet);
      }
      Ok(ReceivedAudioPacket::VideoControl(control)) => {
        session.mark_voice_network_activity();
        warn_stale_voice_senders(&session, &mut last_voice_by_sender, Instant::now(), voice_packets, None);
        video_controls = video_controls.saturating_add(1);
        last_packet = "video_control";
        session.handle_video_control_packet(control);
      }
      Err(ServerError::Protocol(error)) => {
        malformed_packets = malformed_packets.saturating_add(1);
        tracing::warn!(
          target: "voice",
          "[voice] ignored malformed audio packet #{}: {error}; {}",
          malformed_packets,
          session.connection_debug_context()
        );
        continue;
      }
      Err(error) => {
        let error = error.to_string();
        let stop_reason = format!("transport error: {error}");
        tracing::warn!(
          target: "voice",
          "[voice] voice receiver transport error; waiting for keepalive/control to confirm disconnect: {error}; {}",
          session.connection_debug_context()
        );
        session.set_voice_connection_warning(
          LobbyConnectionWarningKind::VoiceReceiverStopped,
          format!("Voice receiver stopped: {error}"),
        );
        break stop_reason;
      }
    }
  };

  tracing::warn!(
    target: "voice",
    "[voice] voice receiver stopped: reason='{stop_reason}' uptime_ms={} voice_packets={} stream_packets={} video_controls={} malformed_packets={} last_packet={} last_voice_sender={last_voice_sender:?} last_voice_sequence={last_voice_sequence:?} {}",
    started_at.elapsed().as_millis(),
    voice_packets,
    stream_packets,
    video_controls,
    malformed_packets,
    last_packet,
    session.connection_debug_context()
  );
}

fn warn_stale_voice_senders<S>(
  session: &S,
  last_voice_by_sender: &mut HashMap<UserId, VoiceSenderReceiveState>,
  now: Instant,
  voice_packets: u64,
  skip_sender: Option<UserId>,
) where
  S: VoiceReceiverSession,
{
  for (sender_id, state) in last_voice_by_sender.iter_mut() {
    if Some(*sender_id) == skip_sender {
      continue;
    }
    let silence = now.duration_since(state.last_at);
    if silence < VOICE_SENDER_SILENCE_WARN_AFTER {
      continue;
    }
    if state
      .last_silence_warn_at
      .is_some_and(|last_warn| now.duration_since(last_warn) < VOICE_SENDER_SILENCE_WARN_REPEAT)
    {
      continue;
    }
    state.last_silence_warn_at = Some(now);
    tracing::warn!(
      target: "voice",
      "[voice] no voice packets from user {} for {}ms while audio receiver is still active: last_sequence={} total_voice_packets={} note=\"could be silence/VAD, sender-side capture suppression, or server forwarding gap; wait for resume/sequence-gap log to distinguish packet loss\" {}",
      sender_id,
      silence.as_millis(),
      state.sequence,
      voice_packets,
      session.connection_debug_context()
    );
  }
}

pub(super) struct VoiceRuntime {
  engine: Mutex<Option<VoiceEngine>>,
  voice_audio_counts: Mutex<HashMap<UserId, u64>>,
  voice_audio_queued_counts: Mutex<HashMap<UserId, u64>>,
  voice_audio_last_played_packet_at: Mutex<HashMap<UserId, SystemTime>>,
  stream_audio_counts: Mutex<HashMap<UserId, u64>>,
  stream_audio_queued_counts: Mutex<HashMap<UserId, u64>>,
  stream_audio_last_played_packet_at: Mutex<HashMap<UserId, SystemTime>>,
  user_volumes: Mutex<HashMap<UserId, i32>>,
  stream_volumes: Mutex<HashMap<UserId, i32>>,
  normalized_users: Mutex<HashSet<UserId>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceAudioDebugCounts {
  pub received: u64,
  pub queued: u64,
  pub last_played_packet_at: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamAudioDebugCounts {
  pub received: u64,
  pub queued: u64,
  pub last_played_packet_at: Option<SystemTime>,
}

impl VoiceRuntime {
  pub(super) fn new() -> Self {
    Self {
      engine: Mutex::new(None),
      voice_audio_counts: Mutex::new(HashMap::new()),
      voice_audio_queued_counts: Mutex::new(HashMap::new()),
      voice_audio_last_played_packet_at: Mutex::new(HashMap::new()),
      stream_audio_counts: Mutex::new(HashMap::new()),
      stream_audio_queued_counts: Mutex::new(HashMap::new()),
      stream_audio_last_played_packet_at: Mutex::new(HashMap::new()),
      user_volumes: Mutex::new(HashMap::new()),
      stream_volumes: Mutex::new(HashMap::new()),
      normalized_users: Mutex::new(HashSet::new()),
    }
  }

  pub(super) fn clear_counts(&self) {
    self.voice_audio_counts.lock().clear();
    self.voice_audio_queued_counts.lock().clear();
    self.voice_audio_last_played_packet_at.lock().clear();
    self.stream_audio_counts.lock().clear();
    self.stream_audio_queued_counts.lock().clear();
    self.stream_audio_last_played_packet_at.lock().clear();
  }

  pub(super) fn clear_volumes(&self) {
    self.user_volumes.lock().clear();
    self.stream_volumes.lock().clear();
    self.normalized_users.lock().clear();
  }

  pub(super) fn engine_status(&self) -> (bool, bool) {
    let engine = self.engine.lock();
    (
      engine.is_some(),
      engine.as_ref().is_some_and(VoiceEngine::captures_voice),
    )
  }

  pub(super) fn voice_audio_debug_counts(&self, user_id: UserId) -> VoiceAudioDebugCounts {
    VoiceAudioDebugCounts {
      received: self.voice_audio_counts.lock().get(&user_id).copied().unwrap_or(0),
      queued: self
        .voice_audio_queued_counts
        .lock()
        .get(&user_id)
        .copied()
        .unwrap_or(0),
      last_played_packet_at: self.voice_audio_last_played_packet_at.lock().get(&user_id).copied(),
    }
  }

  pub(super) fn stream_audio_debug_counts(&self, user_id: UserId) -> StreamAudioDebugCounts {
    StreamAudioDebugCounts {
      received: self.stream_audio_counts.lock().get(&user_id).copied().unwrap_or(0),
      queued: self
        .stream_audio_queued_counts
        .lock()
        .get(&user_id)
        .copied()
        .unwrap_or(0),
      last_played_packet_at: self.stream_audio_last_played_packet_at.lock().get(&user_id).copied(),
    }
  }

  pub(super) fn has_engine(&self) -> bool {
    self.engine.lock().is_some()
  }

  pub(super) fn start_capture(
    &self,
    server: Arc<Server>,
    settings: AppSettings,
    muted: bool,
    deafened: bool,
    on_local_voice: LocalVoiceCallback,
  ) -> Result<bool, String> {
    let mut engine =
      VoiceEngine::start(server, settings, muted, deafened, on_local_voice).map_err(|error| error.to_string())?;
    self.apply_stored_audio_preferences(&mut engine);
    let captures_voice = engine.captures_voice();
    *self.engine.lock() = Some(engine);
    Ok(captures_voice)
  }

  pub(super) fn ensure_stream_playback(&self, settings: AppSettings, deafened: bool) -> Result<bool, String> {
    if self.engine.lock().is_some() {
      return Ok(false);
    }

    let mut engine = VoiceEngine::start_playback(settings, deafened).map_err(|error| error.to_string())?;
    self.apply_stored_audio_preferences(&mut engine);
    *self.engine.lock() = Some(engine);
    Ok(true)
  }

  pub(super) fn voice_active(&self) -> bool {
    self.engine.lock().as_ref().is_some_and(VoiceEngine::captures_voice)
  }

  pub(super) fn stop(&self) -> bool {
    self.engine.lock().take().is_some()
  }

  pub(super) fn set_voice_state(&self, muted: bool, deafened: bool) {
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_voice_state(muted, deafened);
    }
  }

  pub(super) fn set_voice_activation_threshold(&self, value: i32) {
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_voice_activation_threshold(value);
    }
  }

  pub(super) fn set_voice_normalization(&self, value: bool) {
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_voice_normalization(value);
    }
  }

  pub(super) fn set_voice_normalization_target_level(&self, value: i32) {
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_voice_normalization_target_level(value);
    }
  }

  pub(super) fn user_normalization(&self, user_id: UserId) -> bool {
    self.normalized_users.lock().contains(&user_id)
  }

  pub(super) fn set_user_normalization(&self, user_id: UserId, enabled: bool) -> bool {
    {
      let mut normalized_users = self.normalized_users.lock();
      if enabled {
        normalized_users.insert(user_id);
      } else {
        normalized_users.remove(&user_id);
      }
    }
    if let Some(engine) = self.engine.lock().as_mut() {
      engine.set_user_normalization(user_id, enabled);
    }
    enabled
  }

  pub(super) fn set_push_to_talk_release_delay_ms(&self, value: i32) {
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_push_to_talk_release_delay_ms(value);
    }
  }

  pub(super) fn set_push_to_talk_active(&self, active: bool) -> u64 {
    let engine = self.engine.lock();
    if let Some(engine) = engine.as_ref() {
      engine.set_push_to_talk_active(active);
      engine.push_to_talk_release_delay_ms()
    } else {
      0
    }
  }

  pub(super) fn user_volume(&self, user_id: UserId) -> i32 {
    self
      .user_volumes
      .lock()
      .get(&user_id)
      .copied()
      .unwrap_or(DEFAULT_USER_VOLUME)
  }

  pub(super) fn set_user_volume(&self, user_id: UserId, volume: i32) -> i32 {
    let volume = volume.clamp(0, 100);
    {
      let mut user_volumes = self.user_volumes.lock();
      if volume == DEFAULT_USER_VOLUME {
        user_volumes.remove(&user_id);
      } else {
        user_volumes.insert(user_id, volume);
      }
    }
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_user_volume(user_id, volume);
    }
    volume
  }

  pub(super) fn restart_audio_receiver(&self, user_id: UserId) -> bool {
    self.voice_audio_counts.lock().remove(&user_id);
    self.voice_audio_queued_counts.lock().remove(&user_id);
    self.voice_audio_last_played_packet_at.lock().remove(&user_id);
    self
      .engine
      .lock()
      .as_mut()
      .is_some_and(|engine| engine.restart_audio_receiver(user_id))
  }

  pub(super) fn stream_volume(&self, user_id: UserId) -> i32 {
    self
      .stream_volumes
      .lock()
      .get(&user_id)
      .copied()
      .unwrap_or(DEFAULT_USER_VOLUME)
  }

  pub(super) fn set_stream_volume(&self, user_id: UserId, volume: i32) -> i32 {
    let volume = volume.clamp(0, 100);
    {
      let mut stream_volumes = self.stream_volumes.lock();
      if volume == DEFAULT_USER_VOLUME {
        stream_volumes.remove(&user_id);
      } else {
        stream_volumes.insert(user_id, volume);
      }
    }
    if let Some(engine) = self.engine.lock().as_ref() {
      engine.set_stream_volume(user_id, volume);
    }
    volume
  }

  pub(super) fn handle_voice_packet(&self, packet: ForwardedVoicePacket) -> bool {
    let sender_id = packet.sender_id;
    let sequence = packet.sequence;
    let packet_len = packet.opus.len();
    let received_count = {
      let mut counts = self.voice_audio_counts.lock();
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

    let status = self.engine.lock().as_mut().map(|engine| engine.push_packet(packet));
    if status.is_some_and(|status| status.queued) {
      let mut counts = self.voice_audio_queued_counts.lock();
      increment_counter(&mut counts, sender_id);
      self
        .voice_audio_last_played_packet_at
        .lock()
        .insert(sender_id, SystemTime::now());
    }
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

  pub(super) fn handle_stream_audio_packet(&self, packet: ForwardedStreamAudioPacket, watched_user_id: Option<UserId>) {
    let sender_id = packet.sender_id;
    let received_count = {
      let mut counts = self.stream_audio_counts.lock();
      increment_counter(&mut counts, sender_id)
    };
    if should_log_audio_count(received_count) {
      tracing::debug!(target: "audio::decode",
        "[audio:decode] received stream audio #{received_count} from user {}: watched={watched_user_id:?} bytes={}",
        sender_id,
        packet.opus.len()
      );
    }
    if watched_user_id != Some(sender_id) {
      return;
    }

    let queued = self
      .engine
      .lock()
      .as_mut()
      .is_some_and(|engine| engine.push_stream_audio_packet(packet));
    if queued {
      let mut counts = self.stream_audio_queued_counts.lock();
      increment_counter(&mut counts, sender_id);
      self
        .stream_audio_last_played_packet_at
        .lock()
        .insert(sender_id, SystemTime::now());
    }
    if should_log_audio_count(received_count) {
      tracing::debug!(target: "audio::decode",
        "[audio:decode] stream audio {} for watched user {}",
        if queued { "queued" } else { "dropped" },
        watched_user_id.unwrap_or_default()
      );
    }
  }

  pub(super) fn clear_stream_audio(&self, user_id: Option<UserId>) {
    let engine = self.engine.lock();
    let Some(engine) = engine.as_ref() else {
      return;
    };

    if let Some(user_id) = user_id {
      self.stream_audio_queued_counts.lock().remove(&user_id);
      self.stream_audio_last_played_packet_at.lock().remove(&user_id);
      engine.clear_stream_audio(user_id);
    } else {
      self.stream_audio_queued_counts.lock().clear();
      self.stream_audio_last_played_packet_at.lock().clear();
      engine.clear_all_stream_audio();
    }
  }

  fn apply_stored_audio_preferences(&self, engine: &mut VoiceEngine) {
    let user_volumes = self.user_volumes.lock();
    for (user_id, volume) in user_volumes.iter() {
      engine.set_user_volume(*user_id, *volume);
    }
    drop(user_volumes);

    let stream_volumes = self.stream_volumes.lock();
    for (user_id, volume) in stream_volumes.iter() {
      engine.set_stream_volume(*user_id, *volume);
    }
    drop(stream_volumes);

    let normalized_users = self.normalized_users.lock();
    for user_id in normalized_users.iter() {
      engine.set_user_normalization(*user_id, true);
    }
  }
}

impl Default for VoiceRuntime {
  fn default() -> Self {
    Self::new()
  }
}

fn increment_counter(counters: &mut HashMap<UserId, u64>, user_id: UserId) -> u64 {
  let counter = counters.entry(user_id).or_insert(0);
  *counter += 1;
  *counter
}

fn should_log_audio_count(count: u64) -> bool {
  count == 1 || count % 100 == 0
}
