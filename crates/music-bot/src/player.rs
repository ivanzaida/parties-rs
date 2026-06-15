use std::{
  sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
  },
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

use server_plugin::{BOT_VOICE_FRAME_DURATION_MS, BotUser, ChannelId, HostHandle, abi::LogLevel};

use crate::{
  audio::{AudioFrames, VoiceEncoder},
  queue::{PlaybackSnapshot, PlayerState, QueuedTrack, Track},
  sources::registry::SourceRegistry,
};

const PACER_SPIN_THRESHOLD: Duration = Duration::from_millis(1);
const EMPTY_VOICE_TIMEOUT: Duration = Duration::from_secs(20);
const EMPTY_VOICE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

enum PlayerCommand {
  Enqueue {
    track: Track,
    text_channel_id: ChannelId,
  },
  EnqueueMany {
    tracks: Vec<Track>,
    text_channel_id: ChannelId,
  },
  Skip {
    text_channel_id: ChannelId,
  },
  Stop {
    text_channel_id: ChannelId,
  },
  Shutdown,
}

pub(crate) struct PlaybackWorker {
  tx: Sender<PlayerCommand>,
  state: Arc<Mutex<PlayerState>>,
  join_handle: Option<JoinHandle<()>>,
}

impl PlaybackWorker {
  pub(crate) fn spawn(host: HostHandle, bot: BotUser, sources: SourceRegistry, voice_channel_id: ChannelId) -> Self {
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(PlayerState::default()));
    let worker_state = Arc::clone(&state);
    let join_handle = thread::spawn(move || run_player(host, bot, sources, voice_channel_id, rx, worker_state));

    Self {
      tx,
      state,
      join_handle: Some(join_handle),
    }
  }

  pub(crate) fn enqueue(&self, track: Track, text_channel_id: ChannelId) {
    self.tx.send(PlayerCommand::Enqueue { track, text_channel_id }).ok();
  }

  pub(crate) fn enqueue_many(&self, tracks: Vec<Track>, text_channel_id: ChannelId) {
    self
      .tx
      .send(PlayerCommand::EnqueueMany {
        tracks,
        text_channel_id,
      })
      .ok();
  }

  pub(crate) fn skip(&self, text_channel_id: ChannelId) {
    self.tx.send(PlayerCommand::Skip { text_channel_id }).ok();
  }

  pub(crate) fn stop(&self, text_channel_id: ChannelId) {
    self.tx.send(PlayerCommand::Stop { text_channel_id }).ok();
  }

  pub(crate) fn snapshot(&self) -> PlaybackSnapshot {
    let state = self.state.lock().expect("playback state mutex poisoned");
    let current_elapsed_ms = state.current_started_at.map(|started_at| {
      let elapsed_ms = Instant::now().saturating_duration_since(started_at).as_millis();
      u64::try_from(elapsed_ms).unwrap_or(u64::MAX)
    });
    PlaybackSnapshot {
      current: state.current.as_ref().map(|queued| queued.track.summary()),
      current_elapsed_ms,
      queue: state.queue.iter().map(|queued| queued.track.summary()).collect(),
    }
  }

  pub(crate) fn is_finished(&self) -> bool {
    self
      .join_handle
      .as_ref()
      .is_some_and(|join_handle| join_handle.is_finished())
  }

  pub(crate) fn shutdown(mut self) {
    self.tx.send(PlayerCommand::Shutdown).ok();
    if let Some(join_handle) = self.join_handle.take() {
      join_handle.join().ok();
    }
  }
}

impl Drop for PlaybackWorker {
  fn drop(&mut self) {
    self.tx.send(PlayerCommand::Shutdown).ok();
    if let Some(join_handle) = self.join_handle.take() {
      join_handle.join().ok();
    }
  }
}

enum PlaybackControl {
  Continue,
  Shutdown,
}

enum CommandDrain {
  KeepPlaying,
  EndCurrent,
  Shutdown,
}

fn run_player(
  host: HostHandle,
  bot: BotUser,
  sources: SourceRegistry,
  voice_channel_id: ChannelId,
  rx: Receiver<PlayerCommand>,
  state: Arc<Mutex<PlayerState>>,
) {
  let mut sequence = 0u16;
  while let NextTrack::Track(queued) = next_track(&host, &bot, &rx, &state) {
    match play_track(
      &host,
      &bot,
      &sources,
      voice_channel_id,
      &rx,
      &state,
      &mut sequence,
      queued,
    ) {
      PlaybackControl::Continue => {}
      PlaybackControl::Shutdown => break,
    }
  }
}

enum NextTrack {
  Track(QueuedTrack),
  Shutdown,
}

fn next_track(
  host: &HostHandle,
  bot: &BotUser,
  rx: &Receiver<PlayerCommand>,
  state: &Arc<Mutex<PlayerState>>,
) -> NextTrack {
  if let Some(track) = take_next_queued_track(state) {
    return NextTrack::Track(track);
  }

  while let Ok(command) = rx.recv() {
    match command {
      PlayerCommand::Enqueue { track, text_channel_id } => {
        let queued = QueuedTrack { track, text_channel_id };
        set_current_track(state, queued.clone());
        return NextTrack::Track(queued);
      }
      PlayerCommand::EnqueueMany {
        tracks,
        text_channel_id,
      } => {
        if let Some(queued) = enqueue_many_from_idle(state, tracks, text_channel_id) {
          return NextTrack::Track(queued);
        }
      }
      PlayerCommand::Skip { text_channel_id } => {
        host.send_bot_chat(bot, text_channel_id, "Nothing to skip.").ok();
      }
      PlayerCommand::Stop { text_channel_id } => {
        clear_playback_state(state);
        host.send_bot_chat(bot, text_channel_id, "Nothing to stop.").ok();
      }
      PlayerCommand::Shutdown => return NextTrack::Shutdown,
    }
  }

  NextTrack::Shutdown
}

fn play_track(
  host: &HostHandle,
  bot: &BotUser,
  sources: &SourceRegistry,
  voice_channel_id: ChannelId,
  rx: &Receiver<PlayerCommand>,
  state: &Arc<Mutex<PlayerState>>,
  sequence: &mut u16,
  queued: QueuedTrack,
) -> PlaybackControl {
  let mut track = queued.track;
  let text_channel_id = queued.text_channel_id;

  if !bot_is_still_in_expected_voice(host, bot, voice_channel_id) {
    stop_after_voice_disconnect(host, bot, state, text_channel_id);
    return PlaybackControl::Shutdown;
  }

  let mut frames = match AudioFrames::open(&mut track, sources) {
    Ok(frames) => frames,
    Err(error) => {
      host
        .send_bot_chat(
          bot,
          text_channel_id,
          &format!(
            "Could not play {}: {}",
            track.markdown_link(),
            playback_error_detail(&error)
          ),
        )
        .ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
  };
  update_current_track_title(state, &track.title);

  let mut encoder = match VoiceEncoder::new() {
    Ok(encoder) => encoder,
    Err(error) => {
      host
        .send_bot_chat(bot, text_channel_id, "Playback stopped: audio encoder failed.")
        .ok();
      host
        .log(LogLevel::Warn, &format!("music-bot audio encoder failed: {error}"))
        .ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
  };

  mark_current_track_started(state);

  let response = format!("Playing: {}", track.markdown_link());
  host.send_bot_chat(bot, text_channel_id, &response).ok();

  let mut pacer = FramePacer::new();
  let mut idle_voice = IdleVoiceMonitor::new();
  while let Some(frame) = match frames.next_frame() {
    Ok(frame) => frame,
    Err(error) => {
      host
        .send_bot_chat(
          bot,
          text_channel_id,
          &format!(
            "Playback stopped for {}: {}",
            track.markdown_link(),
            playback_error_detail(&error)
          ),
        )
        .ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
  } {
    match drain_commands_while_playing(host, bot, rx, state, &track) {
      CommandDrain::KeepPlaying => {}
      CommandDrain::EndCurrent => return PlaybackControl::Continue,
      CommandDrain::Shutdown => return PlaybackControl::Shutdown,
    }

    if !bot_is_still_in_expected_voice(host, bot, voice_channel_id) {
      stop_after_voice_disconnect(host, bot, state, text_channel_id);
      return PlaybackControl::Shutdown;
    }

    if idle_voice.channel_has_been_empty_too_long(host, voice_channel_id) {
      stop_after_empty_voice_channel(host, bot, state, text_channel_id);
      return PlaybackControl::Shutdown;
    }

    let opus_payload = match encoder.encode(&frame) {
      Ok(payload) => payload,
      Err(error) => {
        host
          .send_bot_chat(bot, text_channel_id, "Playback stopped: audio encoder failed.")
          .ok();
        host
          .log(LogLevel::Warn, &format!("music-bot audio encoder failed: {error}"))
          .ok();
        clear_current_track(state);
        return PlaybackControl::Continue;
      }
    };

    pacer.wait_for_next_frame();

    if let Err(error) = host.send_bot_voice_packet(bot, *sequence, &opus_payload) {
      host
        .send_bot_chat(bot, text_channel_id, "Playback stopped: could not send voice audio.")
        .ok();
      host
        .log(LogLevel::Warn, &format!("music-bot voice packet failed: {error}"))
        .ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
    *sequence = sequence.wrapping_add(1);
  }

  clear_current_track(state);
  PlaybackControl::Continue
}

fn bot_is_still_in_expected_voice(host: &HostHandle, bot: &BotUser, expected: ChannelId) -> bool {
  matches!(host.bot_voice_channel(bot), Ok(Some(channel_id)) if channel_id == expected)
}

fn stop_after_voice_disconnect(
  host: &HostHandle,
  bot: &BotUser,
  state: &Arc<Mutex<PlayerState>>,
  text_channel_id: ChannelId,
) {
  clear_playback_state(state);
  host
    .send_bot_chat(
      bot,
      text_channel_id,
      "Removed from voice. Playback stopped and queue cleared.",
    )
    .ok();
}

fn stop_after_empty_voice_channel(
  host: &HostHandle,
  bot: &BotUser,
  state: &Arc<Mutex<PlayerState>>,
  text_channel_id: ChannelId,
) {
  clear_playback_state(state);
  host.leave_bot_voice(bot).ok();
  host
    .send_bot_chat(
      bot,
      text_channel_id,
      "Voice channel was empty for 20 seconds. Playback stopped and queue cleared.",
    )
    .ok();
}

fn playback_error_detail(error: &str) -> &'static str {
  if error.contains("HTTP status") {
    "provider rejected the audio stream"
  } else if error.contains("decode") {
    "audio could not be decoded"
  } else if error.contains("read") {
    "audio stream ended unexpectedly"
  } else if error.contains("probe") || error.contains("open") {
    "audio stream could not be opened"
  } else {
    "audio stream failed"
  }
}

fn drain_commands_while_playing(
  host: &HostHandle,
  bot: &BotUser,
  rx: &Receiver<PlayerCommand>,
  state: &Arc<Mutex<PlayerState>>,
  current: &Track,
) -> CommandDrain {
  while let Ok(command) = rx.try_recv() {
    match command {
      PlayerCommand::Enqueue { track, text_channel_id } => {
        let response = {
          let mut state = state.lock().expect("playback state mutex poisoned");
          state.queue.push_back(QueuedTrack {
            track: track.clone(),
            text_channel_id,
          });
          let position = state.queue.len();
          format!("Queued #{}: {}", position, track.markdown_link_with_duration())
        };
        host.send_bot_chat(bot, text_channel_id, &response).ok();
      }
      PlayerCommand::EnqueueMany {
        tracks,
        text_channel_id,
      } => {
        let mut state = state.lock().expect("playback state mutex poisoned");
        state
          .queue
          .extend(tracks.into_iter().map(|track| QueuedTrack { track, text_channel_id }));
      }
      PlayerCommand::Skip { text_channel_id } => {
        clear_current_track(state);
        let response = format!("Skipped: {}", current.markdown_link());
        host.send_bot_chat(bot, text_channel_id, &response).ok();
        return CommandDrain::EndCurrent;
      }
      PlayerCommand::Stop { text_channel_id } => {
        clear_playback_state(state);
        host.send_bot_chat(bot, text_channel_id, "Stopped. Queue cleared.").ok();
        return CommandDrain::EndCurrent;
      }
      PlayerCommand::Shutdown => return CommandDrain::Shutdown,
    }
  }

  CommandDrain::KeepPlaying
}

fn set_current_track(state: &Arc<Mutex<PlayerState>>, queued: QueuedTrack) {
  let mut state = state.lock().expect("playback state mutex poisoned");
  state.current = Some(queued);
  state.current_started_at = None;
}

fn enqueue_many_from_idle(
  state: &Arc<Mutex<PlayerState>>,
  tracks: Vec<Track>,
  text_channel_id: ChannelId,
) -> Option<QueuedTrack> {
  let mut tracks = tracks.into_iter();
  let queued = QueuedTrack {
    track: tracks.next()?,
    text_channel_id,
  };
  let mut state = state.lock().expect("playback state mutex poisoned");
  state.current = Some(queued.clone());
  state.current_started_at = None;
  state
    .queue
    .extend(tracks.map(|track| QueuedTrack { track, text_channel_id }));
  Some(queued)
}

fn update_current_track_title(state: &Arc<Mutex<PlayerState>>, title: &str) {
  if let Some(current) = state.lock().expect("playback state mutex poisoned").current.as_mut() {
    current.track.title = title.to_owned();
  }
}

fn clear_current_track(state: &Arc<Mutex<PlayerState>>) {
  let mut state = state.lock().expect("playback state mutex poisoned");
  state.current = None;
  state.current_started_at = None;
}

fn clear_playback_state(state: &Arc<Mutex<PlayerState>>) {
  let mut state = state.lock().expect("playback state mutex poisoned");
  state.current = None;
  state.current_started_at = None;
  state.queue.clear();
}

fn take_next_queued_track(state: &Arc<Mutex<PlayerState>>) -> Option<QueuedTrack> {
  let mut state = state.lock().expect("playback state mutex poisoned");
  let queued = state.queue.pop_front()?;
  state.current = Some(queued.clone());
  state.current_started_at = None;
  Some(queued)
}

fn mark_current_track_started(state: &Arc<Mutex<PlayerState>>) {
  state.lock().expect("playback state mutex poisoned").current_started_at = Some(Instant::now());
}

struct FramePacer {
  next_deadline: Instant,
  frame_duration: Duration,
}

struct IdleVoiceMonitor {
  next_check_at: Instant,
  empty_since: Option<Instant>,
}

impl IdleVoiceMonitor {
  fn new() -> Self {
    Self {
      next_check_at: Instant::now(),
      empty_since: None,
    }
  }

  fn channel_has_been_empty_too_long(&mut self, host: &HostHandle, voice_channel_id: ChannelId) -> bool {
    let now = Instant::now();
    if now < self.next_check_at {
      return false;
    }
    self.next_check_at = now + EMPTY_VOICE_CHECK_INTERVAL;

    match host.get_voice_channel_info(voice_channel_id) {
      Ok(info) if info.user_count <= 1 => {
        let empty_since = self.empty_since.get_or_insert(now);
        now.saturating_duration_since(*empty_since) >= EMPTY_VOICE_TIMEOUT
      }
      Ok(_) => {
        self.empty_since = None;
        false
      }
      Err(_) => false,
    }
  }
}

impl FramePacer {
  fn new() -> Self {
    Self {
      next_deadline: Instant::now(),
      frame_duration: Duration::from_millis(u64::from(BOT_VOICE_FRAME_DURATION_MS)),
    }
  }

  fn wait_for_next_frame(&mut self) {
    let deadline = self.next_deadline;
    sleep_until(deadline);

    let now = Instant::now();
    self.next_deadline = deadline + self.frame_duration;
    if now.saturating_duration_since(deadline) > self.frame_duration {
      self.next_deadline = now + self.frame_duration;
    }
  }
}

fn sleep_until(deadline: Instant) {
  loop {
    let now = Instant::now();
    let Some(remaining) = deadline.checked_duration_since(now) else {
      return;
    };

    if remaining > PACER_SPIN_THRESHOLD {
      thread::sleep(remaining - PACER_SPIN_THRESHOLD);
    } else {
      thread::yield_now();
    }
  }
}
