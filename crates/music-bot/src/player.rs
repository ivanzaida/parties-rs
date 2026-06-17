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
  queue::{PlaybackSnapshot, PlayerState, QueuedTrack, Track, playlist_queue_message},
  sources::registry::SourceRegistry,
};

const PACER_SPIN_THRESHOLD: Duration = Duration::from_millis(1);
#[cfg(not(test))]
const EMPTY_VOICE_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(test)]
const EMPTY_VOICE_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const EMPTY_VOICE_CHECK_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(test)]
const EMPTY_VOICE_CHECK_INTERVAL: Duration = Duration::from_millis(5);

enum PlayerCommand {
  ResolveAndEnqueue {
    input: String,
    text_channel_id: ChannelId,
  },
  EnqueueResolved {
    tracks: Result<Vec<Track>, String>,
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
    let resolver_tx = tx.clone();
    let join_handle =
      thread::spawn(move || run_player(host, bot, sources, voice_channel_id, rx, worker_state, resolver_tx));

    Self {
      tx,
      state,
      join_handle: Some(join_handle),
    }
  }

  pub(crate) fn resolve_and_enqueue(&self, input: String, text_channel_id: ChannelId) {
    self
      .tx
      .send(PlayerCommand::ResolveAndEnqueue { input, text_channel_id })
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
  tx: Sender<PlayerCommand>,
) {
  let mut sequence = 0u16;
  let mut resolver_threads = ResolverThreads::new(tx);
  let mut idle_voice = IdleVoiceMonitor::new();
  let mut last_text_channel_id = None;
  while let NextTrack::Track(queued) = next_track(
    &host,
    &bot,
    &sources,
    voice_channel_id,
    &rx,
    &state,
    &mut resolver_threads,
    &mut idle_voice,
    &mut last_text_channel_id,
  ) {
    match play_track(
      &host,
      &bot,
      &sources,
      voice_channel_id,
      &rx,
      &state,
      &mut resolver_threads,
      &mut idle_voice,
      &mut last_text_channel_id,
      &mut sequence,
      queued,
    ) {
      PlaybackControl::Continue => {}
      PlaybackControl::Shutdown => break,
    }
  }
  resolver_threads.join_all();
}

enum NextTrack {
  Track(QueuedTrack),
  Shutdown,
}

fn next_track(
  host: &HostHandle,
  bot: &BotUser,
  sources: &SourceRegistry,
  voice_channel_id: ChannelId,
  rx: &Receiver<PlayerCommand>,
  state: &Arc<Mutex<PlayerState>>,
  resolver_threads: &mut ResolverThreads,
  idle_voice: &mut IdleVoiceMonitor,
  last_text_channel_id: &mut Option<ChannelId>,
) -> NextTrack {
  if let Some(track) = take_next_queued_track(state) {
    return NextTrack::Track(track);
  }

  loop {
    if !bot_is_still_in_expected_voice(host, bot, voice_channel_id) {
      stop_after_voice_disconnect(host, bot, state, *last_text_channel_id);
      return NextTrack::Shutdown;
    }
    if idle_voice.channel_has_been_empty_too_long(host, voice_channel_id) {
      stop_after_empty_voice_channel(host, bot, state, *last_text_channel_id);
      return NextTrack::Shutdown;
    }

    let command = match rx.recv_timeout(EMPTY_VOICE_CHECK_INTERVAL) {
      Ok(command) => command,
      Err(mpsc::RecvTimeoutError::Timeout) => continue,
      Err(mpsc::RecvTimeoutError::Disconnected) => return NextTrack::Shutdown,
    };

    resolver_threads.reap_finished();
    match command {
      PlayerCommand::ResolveAndEnqueue { input, text_channel_id } => {
        *last_text_channel_id = Some(text_channel_id);
        resolver_threads.resolve(input, text_channel_id, sources.clone());
      }
      PlayerCommand::EnqueueResolved {
        tracks,
        text_channel_id,
      } => {
        *last_text_channel_id = Some(text_channel_id);
        if let Some(queued) = enqueue_resolved_from_idle(host, bot, state, tracks, text_channel_id) {
          return NextTrack::Track(queued);
        }
      }
      PlayerCommand::Skip { text_channel_id } => {
        *last_text_channel_id = Some(text_channel_id);
        host.send_bot_chat(bot, text_channel_id, "Nothing to skip.").ok();
      }
      PlayerCommand::Stop { text_channel_id } => {
        *last_text_channel_id = Some(text_channel_id);
        clear_playback_state(state);
        host.send_bot_chat(bot, text_channel_id, "Nothing to stop.").ok();
      }
      PlayerCommand::Shutdown => return NextTrack::Shutdown,
    }
  }
}

fn play_track(
  host: &HostHandle,
  bot: &BotUser,
  sources: &SourceRegistry,
  voice_channel_id: ChannelId,
  rx: &Receiver<PlayerCommand>,
  state: &Arc<Mutex<PlayerState>>,
  resolver_threads: &mut ResolverThreads,
  idle_voice: &mut IdleVoiceMonitor,
  last_text_channel_id: &mut Option<ChannelId>,
  sequence: &mut u16,
  queued: QueuedTrack,
) -> PlaybackControl {
  let mut track = queued.track;
  let text_channel_id = queued.text_channel_id;
  *last_text_channel_id = Some(text_channel_id);

  if !bot_is_still_in_expected_voice(host, bot, voice_channel_id) {
    stop_after_voice_disconnect(host, bot, state, Some(text_channel_id));
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
    match drain_commands_while_playing(host, bot, rx, state, sources, resolver_threads, &track) {
      CommandDrain::KeepPlaying => {}
      CommandDrain::EndCurrent => return PlaybackControl::Continue,
      CommandDrain::Shutdown => return PlaybackControl::Shutdown,
    }

    if !bot_is_still_in_expected_voice(host, bot, voice_channel_id) {
      stop_after_voice_disconnect(host, bot, state, Some(text_channel_id));
      return PlaybackControl::Shutdown;
    }

    if idle_voice.channel_has_been_empty_too_long(host, voice_channel_id) {
      stop_after_empty_voice_channel(host, bot, state, Some(text_channel_id));
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
  text_channel_id: Option<ChannelId>,
) {
  clear_playback_state(state);
  if let Some(text_channel_id) = text_channel_id {
    host
      .send_bot_chat(
        bot,
        text_channel_id,
        "Removed from voice. Playback stopped and queue cleared.",
      )
      .ok();
  }
}

fn stop_after_empty_voice_channel(
  host: &HostHandle,
  bot: &BotUser,
  state: &Arc<Mutex<PlayerState>>,
  text_channel_id: Option<ChannelId>,
) {
  clear_playback_state(state);
  host.leave_bot_voice(bot).ok();
  if let Some(text_channel_id) = text_channel_id {
    host
      .send_bot_chat(
        bot,
        text_channel_id,
        "Voice channel was empty for 20 seconds. Playback stopped and queue cleared.",
      )
      .ok();
  }
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
  sources: &SourceRegistry,
  resolver_threads: &mut ResolverThreads,
  current: &Track,
) -> CommandDrain {
  while let Ok(command) = rx.try_recv() {
    resolver_threads.reap_finished();
    match command {
      PlayerCommand::ResolveAndEnqueue { input, text_channel_id } => {
        resolver_threads.resolve(input, text_channel_id, sources.clone());
      }
      PlayerCommand::EnqueueResolved {
        tracks,
        text_channel_id,
      } => {
        enqueue_resolved_while_playing(host, bot, state, tracks, text_channel_id);
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

fn enqueue_resolved_from_idle(
  host: &HostHandle,
  bot: &BotUser,
  state: &Arc<Mutex<PlayerState>>,
  tracks: Result<Vec<Track>, String>,
  text_channel_id: ChannelId,
) -> Option<QueuedTrack> {
  let tracks = match tracks {
    Ok(tracks) => tracks,
    Err(error) => {
      host
        .send_bot_chat(bot, text_channel_id, &source_error_message(&error))
        .ok();
      return None;
    }
  };

  if tracks.is_empty() {
    host
      .send_bot_chat(bot, text_channel_id, "SoundCloud URL did not contain queueable tracks.")
      .ok();
    return None;
  }

  if tracks.len() > 1 {
    host
      .send_bot_chat(bot, text_channel_id, &playlist_queue_message(&tracks))
      .ok();
  }

  enqueue_many_from_idle(state, tracks, text_channel_id)
}

fn enqueue_resolved_while_playing(
  host: &HostHandle,
  bot: &BotUser,
  state: &Arc<Mutex<PlayerState>>,
  tracks: Result<Vec<Track>, String>,
  text_channel_id: ChannelId,
) {
  let tracks = match tracks {
    Ok(tracks) => tracks,
    Err(error) => {
      host
        .send_bot_chat(bot, text_channel_id, &source_error_message(&error))
        .ok();
      return;
    }
  };

  match tracks.len() {
    0 => {
      host
        .send_bot_chat(bot, text_channel_id, "SoundCloud URL did not contain queueable tracks.")
        .ok();
    }
    1 => {
      let track = tracks.into_iter().next().expect("one resolved track");
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
    _ => {
      let response = playlist_queue_message(&tracks);
      let mut state = state.lock().expect("playback state mutex poisoned");
      state
        .queue
        .extend(tracks.into_iter().map(|track| QueuedTrack { track, text_channel_id }));
      drop(state);
      host.send_bot_chat(bot, text_channel_id, &response).ok();
    }
  }
}

fn source_error_message(error: &str) -> String {
  if error.contains("Only SoundCloud URLs are supported") {
    "Only SoundCloud URLs are supported right now.".to_owned()
  } else if error.contains("playlist")
    || error.contains("track")
    || error.contains("SoundCloud")
    || error.contains("private")
    || error.contains("deleted")
  {
    error.to_owned()
  } else {
    "Could not read that SoundCloud URL.".to_owned()
  }
}

struct ResolverThreads {
  tx: Sender<PlayerCommand>,
  handles: Vec<JoinHandle<()>>,
}

impl ResolverThreads {
  fn new(tx: Sender<PlayerCommand>) -> Self {
    Self {
      tx,
      handles: Vec::new(),
    }
  }

  fn resolve(&mut self, input: String, text_channel_id: ChannelId, sources: SourceRegistry) {
    let tx = self.tx.clone();
    self.handles.push(thread::spawn(move || {
      let tracks = Track::parse_many(&input, &sources);
      tx.send(PlayerCommand::EnqueueResolved {
        tracks,
        text_channel_id,
      })
      .ok();
    }));
  }

  fn reap_finished(&mut self) {
    let mut index = 0;
    while index < self.handles.len() {
      if self.handles[index].is_finished() {
        self.handles.swap_remove(index).join().ok();
      } else {
        index += 1;
      }
    }
  }

  fn join_all(mut self) {
    for handle in self.handles.drain(..) {
      handle.join().ok();
    }
  }
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

#[cfg(test)]
mod tests {
  use std::{
    collections::HashMap,
    ffi::{CStr, c_void},
    os::raw::c_char,
    sync::{
      Arc, Mutex,
      mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
  };

  use server_plugin::{HostRef, MessageId, abi};

  use super::*;
  use crate::sources::{
    model::{ResolvedAudio, SourceKind, SourceRequest},
    registry::TestSourceBackend,
  };

  #[test]
  fn resolve_command_does_not_block_playback_command_drain() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let sources = SourceRegistry::new_for_tests(Arc::new(BlockingParseBackend {
      started: Mutex::new(Some(started_tx)),
      release: Mutex::new(release_rx),
    }));
    let (tx, rx) = mpsc::channel();
    let mut resolver_threads = ResolverThreads::new(tx.clone());
    let state = Arc::new(Mutex::new(PlayerState::default()));
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let bot = host.create_bot_user("music", "Music Bot").unwrap();

    tx.send(PlayerCommand::ResolveAndEnqueue {
      input: "https://soundcloud.com/artist/new-track".to_owned(),
      text_channel_id: 99,
    })
    .unwrap();

    let started_at = Instant::now();
    let drain = drain_commands_while_playing(&host, &bot, &rx, &state, &sources, &mut resolver_threads, &track(0));

    assert!(matches!(drain, CommandDrain::KeepPlaying));
    assert!(started_at.elapsed() < Duration::from_millis(200));
    assert!(state.lock().expect("state mutex poisoned").queue.is_empty());

    started_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    release_tx.send(()).unwrap();
    let command = rx.recv_timeout(Duration::from_millis(500)).unwrap();
    match command {
      PlayerCommand::EnqueueResolved {
        tracks,
        text_channel_id,
      } => {
        assert_eq!(text_channel_id, 99);
        assert_eq!(tracks.unwrap().len(), 1);
      }
      _ => panic!("resolver should return resolved enqueue command"),
    }
    resolver_threads.join_all();
  }

  #[test]
  fn resolved_tracks_are_queued_while_playing() {
    let sources = SourceRegistry::new_for_tests(Arc::new(ImmediateBackend));
    let (tx, rx) = mpsc::channel();
    let mut resolver_threads = ResolverThreads::new(tx.clone());
    let state = Arc::new(Mutex::new(PlayerState::default()));
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let bot = host.create_bot_user("music", "Music Bot").unwrap();

    tx.send(PlayerCommand::EnqueueResolved {
      tracks: Ok(vec![track(1)]),
      text_channel_id: 99,
    })
    .unwrap();

    let drain = drain_commands_while_playing(&host, &bot, &rx, &state, &sources, &mut resolver_threads, &track(0));

    assert!(matches!(drain, CommandDrain::KeepPlaying));
    let state = state.lock().expect("state mutex poisoned");
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue[0].track.title, "track 1");
    drop(state);
    assert_eq!(fake.chats.len(), 1);
    assert!(fake.chats[0].2.starts_with("Queued #1: "));
  }

  #[test]
  fn multiple_bot_resolvers_keep_parsed_queues_separate() {
    let sources_a = SourceRegistry::new_for_tests(Arc::new(IndexedBackend {
      first_index: 10,
      count: 2,
    }));
    let sources_b = SourceRegistry::new_for_tests(Arc::new(IndexedBackend {
      first_index: 20,
      count: 3,
    }));
    let (tx_a, rx_a) = mpsc::channel();
    let (tx_b, rx_b) = mpsc::channel();
    let mut resolver_threads_a = ResolverThreads::new(tx_a.clone());
    let mut resolver_threads_b = ResolverThreads::new(tx_b.clone());
    let state_a = Arc::new(Mutex::new(PlayerState::default()));
    let state_b = Arc::new(Mutex::new(PlayerState::default()));
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let bot_a = host.create_bot_user("music", "Music Bot").unwrap();
    let bot_b = host.create_bot_user("music-2", "Music Bot 2").unwrap();

    tx_a
      .send(PlayerCommand::ResolveAndEnqueue {
        input: "https://soundcloud.com/artist/list-a".to_owned(),
        text_channel_id: 101,
      })
      .unwrap();
    tx_b
      .send(PlayerCommand::ResolveAndEnqueue {
        input: "https://soundcloud.com/artist/list-b".to_owned(),
        text_channel_id: 202,
      })
      .unwrap();

    drain_commands_while_playing(
      &host,
      &bot_a,
      &rx_a,
      &state_a,
      &sources_a,
      &mut resolver_threads_a,
      &track(0),
    );
    drain_commands_while_playing(
      &host,
      &bot_b,
      &rx_b,
      &state_b,
      &sources_b,
      &mut resolver_threads_b,
      &track(0),
    );

    drain_until_queue_len(&host, &bot_a, &rx_a, &state_a, &sources_a, &mut resolver_threads_a, 2);
    drain_until_queue_len(&host, &bot_b, &rx_b, &state_b, &sources_b, &mut resolver_threads_b, 3);

    assert_eq!(queue_titles(&state_a), vec!["track 10", "track 11"]);
    assert_eq!(queue_titles(&state_b), vec!["track 20", "track 21", "track 22"]);
    assert!(
      fake
        .chats
        .iter()
        .any(|chat| chat.0 == 1 && chat.1 == 101 && chat.2.starts_with("Added 2 tracks:"))
    );
    assert!(
      fake
        .chats
        .iter()
        .any(|chat| chat.0 == 2 && chat.1 == 202 && chat.2.starts_with("Added 3 tracks:"))
    );

    resolver_threads_a.join_all();
    resolver_threads_b.join_all();
  }

  #[test]
  fn idle_worker_leaves_empty_voice_channel() {
    let sources = SourceRegistry::new_for_tests(Arc::new(ImmediateBackend));
    let (tx, rx) = mpsc::channel();
    let mut resolver_threads = ResolverThreads::new(tx);
    let state = Arc::new(Mutex::new(PlayerState::default()));
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let bot = host.create_bot_user("music", "Music Bot").unwrap();
    fake.set_bot_voice(1, 42);
    fake.set_channel_user_count(42, 1);
    let mut idle_voice = IdleVoiceMonitor::new();
    let mut last_text_channel_id = Some(99);

    let next = next_track(
      &host,
      &bot,
      &sources,
      42,
      &rx,
      &state,
      &mut resolver_threads,
      &mut idle_voice,
      &mut last_text_channel_id,
    );

    assert!(matches!(next, NextTrack::Shutdown));
    assert_eq!(fake.bot_voice(1), None);
    assert_eq!(fake.leaves, vec![1]);
    assert_eq!(
      fake.chats,
      vec![(
        1,
        99,
        "Voice channel was empty for 20 seconds. Playback stopped and queue cleared.".to_owned()
      )]
    );
    resolver_threads.join_all();
  }

  struct BlockingParseBackend {
    started: Mutex<Option<Sender<()>>>,
    release: Mutex<Receiver<()>>,
  }

  impl TestSourceBackend for BlockingParseBackend {
    fn parse(&self, input: &str) -> Result<SourceRequest, String> {
      self.parse_many(input).map(|mut requests| requests.remove(0))
    }

    fn parse_many(&self, _input: &str) -> Result<Vec<SourceRequest>, String> {
      if let Some(started) = self.started.lock().expect("started mutex poisoned").take() {
        started.send(()).ok();
      }
      self.release.lock().expect("release mutex poisoned").recv().unwrap();
      Ok(vec![source_request(10)])
    }

    fn resolve(&self, _request: &SourceRequest) -> Result<ResolvedAudio, String> {
      Err("not used".to_owned())
    }
  }

  struct ImmediateBackend;

  impl TestSourceBackend for ImmediateBackend {
    fn parse(&self, input: &str) -> Result<SourceRequest, String> {
      self.parse_many(input).map(|mut requests| requests.remove(0))
    }

    fn parse_many(&self, _input: &str) -> Result<Vec<SourceRequest>, String> {
      Ok(vec![source_request(1)])
    }

    fn resolve(&self, _request: &SourceRequest) -> Result<ResolvedAudio, String> {
      Err("not used".to_owned())
    }
  }

  struct IndexedBackend {
    first_index: usize,
    count: usize,
  }

  impl TestSourceBackend for IndexedBackend {
    fn parse(&self, input: &str) -> Result<SourceRequest, String> {
      self.parse_many(input).map(|mut requests| requests.remove(0))
    }

    fn parse_many(&self, _input: &str) -> Result<Vec<SourceRequest>, String> {
      Ok(
        (0..self.count)
          .map(|offset| source_request(self.first_index + offset))
          .collect(),
      )
    }

    fn resolve(&self, _request: &SourceRequest) -> Result<ResolvedAudio, String> {
      Err("not used".to_owned())
    }
  }

  fn drain_until_queue_len(
    host: &HostHandle,
    bot: &BotUser,
    rx: &Receiver<PlayerCommand>,
    state: &Arc<Mutex<PlayerState>>,
    sources: &SourceRegistry,
    resolver_threads: &mut ResolverThreads,
    expected_len: usize,
  ) {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
      drain_commands_while_playing(host, bot, rx, state, sources, resolver_threads, &track(0));
      if state.lock().expect("state mutex poisoned").queue.len() == expected_len {
        return;
      }
      thread::sleep(Duration::from_millis(5));
    }

    panic!("queue did not reach expected length {expected_len}");
  }

  fn queue_titles(state: &Arc<Mutex<PlayerState>>) -> Vec<String> {
    state
      .lock()
      .expect("state mutex poisoned")
      .queue
      .iter()
      .map(|queued| queued.track.title.clone())
      .collect()
  }

  fn track(index: usize) -> Track {
    Track {
      title: format!("track {index}"),
      duration_ms: Some(180_000),
      source: source_request(index),
    }
  }

  fn source_request(index: usize) -> SourceRequest {
    SourceRequest {
      kind: SourceKind::SoundCloud,
      url: format!("https://soundcloud.com/artist/track-{index}"),
      provider_id: Some(index.to_string()),
      duration_ms: Some(180_000),
      loading_title: format!("track {index}"),
    }
  }

  #[derive(Default)]
  struct FakeHost {
    next_bot_id: usize,
    chats: Vec<(usize, ChannelId, String)>,
    bot_voice_channels: HashMap<usize, ChannelId>,
    channel_user_counts: HashMap<ChannelId, u32>,
    leaves: Vec<usize>,
  }

  impl FakeHost {
    fn host_handle(&mut self) -> HostHandle {
      let mut host = abi::Host::empty();
      host.context = (self as *mut Self).cast();
      host.create_bot_user = Some(fake_create_bot_user);
      host.send_bot_chat = Some(fake_send_bot_chat);
      host.leave_bot_voice = Some(fake_leave_bot_voice);
      host.get_voice_channel_info = Some(fake_get_voice_channel_info);
      host.bot_voice_channel = Some(fake_bot_voice_channel);
      unsafe { HostRef::from_raw(&host).unwrap().to_handle() }
    }

    fn set_bot_voice(&mut self, bot_id: usize, channel_id: ChannelId) {
      self.bot_voice_channels.insert(bot_id, channel_id);
    }

    fn bot_voice(&self, bot_id: usize) -> Option<ChannelId> {
      self.bot_voice_channels.get(&bot_id).copied()
    }

    fn set_channel_user_count(&mut self, channel_id: ChannelId, user_count: u32) {
      self.channel_user_counts.insert(channel_id, user_count);
    }
  }

  unsafe extern "C" fn fake_create_bot_user(
    context: *mut c_void,
    _key: *const c_char,
    _display_name: *const c_char,
    out_bot: *mut abi::BotHandle,
    out_user_id: *mut u32,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    fake.next_bot_id += 1;
    unsafe {
      *out_bot = fake.next_bot_id as abi::BotHandle;
      *out_user_id = 100 + fake.next_bot_id as u32;
    }
    true
  }

  unsafe extern "C" fn fake_send_bot_chat(
    context: *mut c_void,
    bot: abi::BotHandle,
    text_channel_id: ChannelId,
    text: *const c_char,
    out_message_id: *mut MessageId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned();
    fake.chats.push((bot as usize, text_channel_id, text));
    unsafe {
      *out_message_id = fake.chats.len() as MessageId;
    }
    true
  }

  unsafe extern "C" fn fake_leave_bot_voice(context: *mut c_void, bot: abi::BotHandle) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    let bot_id = bot as usize;
    fake.bot_voice_channels.remove(&bot_id);
    fake.leaves.push(bot_id);
    true
  }

  unsafe extern "C" fn fake_get_voice_channel_info(
    context: *mut c_void,
    channel_id: ChannelId,
    out_info: *mut abi::ChannelInfo,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    let mut info = abi::ChannelInfo::default();
    info.channel_id = channel_id;
    info.user_count = fake.channel_user_counts.get(&channel_id).copied().unwrap_or(0);
    unsafe {
      *out_info = info;
    }
    true
  }

  unsafe extern "C" fn fake_bot_voice_channel(
    context: *mut c_void,
    bot: abi::BotHandle,
    out_voice_channel_id: *mut ChannelId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    unsafe {
      *out_voice_channel_id = fake.bot_voice_channels.get(&(bot as usize)).copied().unwrap_or(0);
    }
    true
  }
}
