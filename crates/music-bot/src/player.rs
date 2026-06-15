use std::{
  sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
  },
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

use server_plugin::{BOT_VOICE_FRAME_DURATION_MS, BotUser, ChannelId, HostHandle};

use crate::{
  audio::{AudioFrames, VoiceEncoder},
  queue::{PlaybackSnapshot, PlayerState, QueuedTrack, Track},
  sources::registry::SourceRegistry,
};

const PACER_SPIN_THRESHOLD: Duration = Duration::from_millis(1);

enum PlayerCommand {
  Enqueue { track: Track, text_channel_id: ChannelId },
  Skip { text_channel_id: ChannelId },
  Stop { text_channel_id: ChannelId },
  Shutdown,
}

pub(crate) struct PlaybackWorker {
  tx: Sender<PlayerCommand>,
  state: Arc<Mutex<PlayerState>>,
  join_handle: Option<JoinHandle<()>>,
}

impl PlaybackWorker {
  pub(crate) fn spawn(host: HostHandle, bot: BotUser, sources: SourceRegistry) -> Self {
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(PlayerState::default()));
    let worker_state = Arc::clone(&state);
    let join_handle = thread::spawn(move || run_player(host, bot, sources, rx, worker_state));

    Self {
      tx,
      state,
      join_handle: Some(join_handle),
    }
  }

  pub(crate) fn enqueue(&self, track: Track, text_channel_id: ChannelId) {
    self.tx.send(PlayerCommand::Enqueue { track, text_channel_id }).ok();
  }

  pub(crate) fn skip(&self, text_channel_id: ChannelId) {
    self.tx.send(PlayerCommand::Skip { text_channel_id }).ok();
  }

  pub(crate) fn stop(&self, text_channel_id: ChannelId) {
    self.tx.send(PlayerCommand::Stop { text_channel_id }).ok();
  }

  pub(crate) fn snapshot(&self) -> PlaybackSnapshot {
    let state = self.state.lock().expect("playback state mutex poisoned");
    PlaybackSnapshot {
      current: state.current.as_ref().map(|queued| queued.track.title.clone()),
      queue: state.queue.iter().map(|queued| queued.track.title.clone()).collect(),
    }
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
  rx: Receiver<PlayerCommand>,
  state: Arc<Mutex<PlayerState>>,
) {
  let mut sequence = 0u16;
  while let NextTrack::Track(queued) = next_track(&host, &bot, &rx, &state) {
    match play_track(&host, &bot, &sources, &rx, &state, &mut sequence, queued) {
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
      PlayerCommand::Skip { text_channel_id } => {
        host.send_bot_chat(bot, text_channel_id, "Nothing to skip.").ok();
      }
      PlayerCommand::Stop { text_channel_id } => {
        clear_playback_state(state);
        host.send_bot_chat(bot, text_channel_id, "Nothing is playing.").ok();
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
  rx: &Receiver<PlayerCommand>,
  state: &Arc<Mutex<PlayerState>>,
  sequence: &mut u16,
  queued: QueuedTrack,
) -> PlaybackControl {
  let mut track = queued.track;
  let text_channel_id = queued.text_channel_id;
  host
    .send_bot_chat(bot, text_channel_id, &format!("Loading: {}", track.title))
    .ok();

  let mut frames = match AudioFrames::open(&mut track, sources) {
    Ok(frames) => frames,
    Err(error) => {
      host.send_bot_chat(bot, text_channel_id, &error).ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
  };
  update_current_track_title(state, &track.title);

  let response = format!("Now playing: {}", track.title);
  host.send_bot_chat(bot, text_channel_id, &response).ok();

  let mut encoder = match VoiceEncoder::new() {
    Ok(encoder) => encoder,
    Err(error) => {
      let response = format!("Cannot start audio encoder: {error}");
      host.send_bot_chat(bot, text_channel_id, &response).ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
  };

  let mut pacer = FramePacer::new();
  while let Some(frame) = match frames.next_frame() {
    Ok(frame) => frame,
    Err(error) => {
      let response = format!("Failed to read audio for {}: {error}", track.title);
      host.send_bot_chat(bot, text_channel_id, &response).ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
  } {
    match drain_commands_while_playing(host, bot, rx, state, &track) {
      CommandDrain::KeepPlaying => {}
      CommandDrain::EndCurrent => return PlaybackControl::Continue,
      CommandDrain::Shutdown => return PlaybackControl::Shutdown,
    }

    let opus_payload = match encoder.encode(&frame) {
      Ok(payload) => payload,
      Err(error) => {
        let response = format!("Failed to encode {}: {error}", track.title);
        host.send_bot_chat(bot, text_channel_id, &response).ok();
        clear_current_track(state);
        return PlaybackControl::Continue;
      }
    };

    pacer.wait_for_next_frame();

    if let Err(error) = host.send_bot_voice_packet(bot, *sequence, &opus_payload) {
      let response = format!("Failed to send audio for {}: {error}", track.title);
      host.send_bot_chat(bot, text_channel_id, &response).ok();
      clear_current_track(state);
      return PlaybackControl::Continue;
    }
    *sequence = sequence.wrapping_add(1);
  }

  clear_current_track(state);
  PlaybackControl::Continue
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
          format!("Queued: {}", track.title)
        };
        host.send_bot_chat(bot, text_channel_id, &response).ok();
      }
      PlayerCommand::Skip { text_channel_id } => {
        clear_current_track(state);
        let response = format!("Skipped: {}", current.title);
        host.send_bot_chat(bot, text_channel_id, &response).ok();
        return CommandDrain::EndCurrent;
      }
      PlayerCommand::Stop { text_channel_id } => {
        clear_playback_state(state);
        host
          .send_bot_chat(bot, text_channel_id, "Stopped playback and cleared the queue.")
          .ok();
        return CommandDrain::EndCurrent;
      }
      PlayerCommand::Shutdown => return CommandDrain::Shutdown,
    }
  }

  CommandDrain::KeepPlaying
}

fn set_current_track(state: &Arc<Mutex<PlayerState>>, queued: QueuedTrack) {
  state.lock().expect("playback state mutex poisoned").current = Some(queued);
}

fn update_current_track_title(state: &Arc<Mutex<PlayerState>>, title: &str) {
  if let Some(current) = state.lock().expect("playback state mutex poisoned").current.as_mut() {
    current.track.title = title.to_owned();
  }
}

fn clear_current_track(state: &Arc<Mutex<PlayerState>>) {
  state.lock().expect("playback state mutex poisoned").current = None;
}

fn clear_playback_state(state: &Arc<Mutex<PlayerState>>) {
  let mut state = state.lock().expect("playback state mutex poisoned");
  state.current = None;
  state.queue.clear();
}

fn take_next_queued_track(state: &Arc<Mutex<PlayerState>>) -> Option<QueuedTrack> {
  let mut state = state.lock().expect("playback state mutex poisoned");
  let queued = state.queue.pop_front()?;
  state.current = Some(queued.clone());
  Some(queued)
}

struct FramePacer {
  next_deadline: Instant,
  frame_duration: Duration,
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
