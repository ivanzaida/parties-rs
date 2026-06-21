use server_plugin::{
  BotUser, ChannelId, ChatCommandInvocationRef, CommandQueryRequestRef, CommandQueryResponse, CommandQueryResult,
  HostHandle, PluginError, UserId,
  abi::LogLevel,
  plugin::{Context, Plugin},
};

use crate::{
  commands::command_definitions,
  config::BotConfig,
  player::PlaybackWorker,
  sources::{registry::SourceRegistry, soundcloud::SoundCloudTokenProvider},
};

#[derive(Default)]
pub(crate) struct MusicBot {
  host: Option<HostHandle>,
  bots: Vec<BotSlot>,
  next_bot_number: usize,
  config: Option<BotConfig>,
  sources: Option<SourceRegistry>,
}

struct BotSlot {
  bot: BotUser,
  voice_channel_id: Option<ChannelId>,
  worker: Option<PlaybackWorker>,
}

impl Plugin for MusicBot {
  fn init(&mut self, context: &mut Context<'_>) -> Result<(), PluginError> {
    let host = context.host();
    self.host = Some(host);
    self.next_bot_number = 1;

    let config = match BotConfig::from_context(context) {
      Ok(config) => config,
      Err(error) => {
        host.log(LogLevel::Warn, &format!("music-bot config error: {error}"))?;
        return Err(error);
      }
    };
    self.sources = match SoundCloudTokenProvider::new(config.soundcloud.clone()) {
      Ok(tokens) => Some(SourceRegistry::new(tokens)),
      Err(error) => {
        host.log(
          LogLevel::Warn,
          &format!("music-bot failed to initialize SoundCloud token provider: {error}"),
        )?;
        return Err(PluginError::HostCallFailed("soundcloud_token_provider"));
      }
    };
    self.config = Some(config);

    if let Err(error) = context.register_commands(&command_definitions()) {
      host.log(
        LogLevel::Warn,
        &format!("music-bot failed to register commands: {error}"),
      )?;
      return Err(error);
    }

    if let Err(error) = self.create_bot_slot(host) {
      host.log(LogLevel::Warn, &format!("music-bot failed to create bot user: {error}"))?;
      return Err(error);
    }

    if let Err(_error) = host.log(LogLevel::Info, "music-bot initialized") {
      // Logging is useful but should not prevent plugin startup.
    }
    Ok(())
  }

  fn on_chat_command(&mut self, invocation: ChatCommandInvocationRef<'_>) {
    let Some(host) = self.host else {
      return;
    };

    match invocation.command_name {
      "play" => self.handle_play(host, invocation),
      "stop" => self.handle_stop(host, invocation),
      "queue" => self.handle_queue(host, invocation),
      "nowplaying" => self.handle_nowplaying(host, invocation),
      "skip" => self.handle_skip(host, invocation),
      _ => {
        let message = format!("music-bot ignored unknown command: {}", invocation.command_name);
        host.log(LogLevel::Debug, &message).ok();
      }
    }
  }

  fn on_chat_command_query(&mut self, request: CommandQueryRequestRef<'_>) -> CommandQueryResponse {
    if request.command_name != "play" || request.argument_name != "query" {
      return CommandQueryResponse::no_results("");
    }

    let query = request.query.trim();
    if query.len() < 2 {
      return CommandQueryResponse::no_results("Type at least 2 characters.");
    }

    let Some(sources) = self.sources.as_ref() else {
      return CommandQueryResponse::plugin_error("Music bot is missing SoundCloud credentials.");
    };

    if sources.supports(query) {
      return CommandQueryResponse::ok(vec![CommandQueryResult {
        id: query.to_owned(),
        title: query.to_owned(),
        subtitle: "SoundCloud URL".to_owned(),
        value: query.to_owned(),
        kind: "soundcloud".to_owned(),
        duration_ms: 0,
        thumbnail_url: String::new(),
      }]);
    }

    match sources.search(query, 10) {
      Ok(requests) => CommandQueryResponse::ok(
        requests
          .into_iter()
          .map(|request| CommandQueryResult {
            id: request.provider_id.unwrap_or_else(|| request.url.clone()),
            title: request.loading_title,
            subtitle: request.url.clone(),
            value: request.url,
            kind: "soundcloud".to_owned(),
            duration_ms: request
              .duration_ms
              .and_then(|duration| u32::try_from(duration).ok())
              .unwrap_or(0),
            thumbnail_url: String::new(),
          })
          .collect(),
      ),
      Err(error) => CommandQueryResponse::plugin_error(error),
    }
  }

  fn shutdown(&mut self) {
    for slot in self.bots.iter_mut() {
      if let Some(worker) = slot.worker.take() {
        worker.shutdown();
      }
    }
    if let Some(sources) = self.sources.take() {
      sources.shutdown();
    }

    if let Some(host) = self.host {
      for slot in self.bots.drain(..) {
        if slot.voice_channel_id.is_some() {
          host.leave_bot_voice(&slot.bot).ok();
        }
        host.destroy_bot_user(&slot.bot).ok();
      }
    } else {
      self.bots.clear();
    }

    self.config = None;
    self.sources = None;
    self.host = None;
  }
}

impl MusicBot {
  fn handle_play(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    self.sync_voice_state(host);

    let Some(input) = invocation
      .arg("query")
      .filter(|arg| arg.present)
      .map(|arg| arg.string_value)
    else {
      self.send_reply(host, invocation.text_channel_id, "Usage: /play {query:string...}");
      return;
    };

    let Some(voice_channel_id) = self.requester_voice_channel(host, &invocation) else {
      return;
    };

    let Some(sources) = self.sources.as_ref() else {
      self.send_reply(
        host,
        invocation.text_channel_id,
        "Music bot is missing SoundCloud credentials.",
      );
      return;
    };
    let url = if sources.supports(input) {
      input.to_owned()
    } else {
      match sources.search(input, 1) {
        Ok(requests) => {
          let Some(request) = requests.into_iter().next() else {
            self.send_reply(host, invocation.text_channel_id, "No SoundCloud results found.");
            return;
          };
          request.url
        }
        Err(error) => {
          host
            .log(
              LogLevel::Warn,
              &format!("failed to search SoundCloud for /play: {error}"),
            )
            .ok();
          self.send_reply(host, invocation.text_channel_id, "SoundCloud search failed.");
          return;
        }
      }
    };

    let slot_index = match self.acquire_bot_for_voice(host, invocation.user_id, voice_channel_id) {
      Ok(slot_index) => slot_index,
      Err(error) => {
        host
          .log(
            LogLevel::Warn,
            &format!("failed to acquire music bot for voice channel: {error}"),
          )
          .ok();
        self.send_reply(
          host,
          invocation.text_channel_id,
          "Could not move a music bot into your voice channel.",
        );
        return;
      }
    };

    if !self.ensure_worker_for_slot(host, slot_index, voice_channel_id) {
      return;
    }

    let Some(worker) = self.bots.get(slot_index).and_then(|slot| slot.worker.as_ref()) else {
      self.send_reply(
        host,
        invocation.text_channel_id,
        "Playback worker is not available. Try /play again.",
      );
      return;
    };

    worker.resolve_and_enqueue(url, invocation.text_channel_id);
  }

  fn handle_stop(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    self.sync_voice_state(host);
    let Some(slot_index) = self.same_channel_bot_or_reply(host, &invocation) else {
      return;
    };

    if let Some(worker) = self.bots[slot_index].worker.as_ref() {
      worker.stop(invocation.text_channel_id);
    } else {
      self.send_reply(host, invocation.text_channel_id, "Nothing to stop.");
    }

    if let Some(worker) = self.bots[slot_index].worker.take() {
      worker.shutdown();
    }
    host.leave_bot_voice(&self.bots[slot_index].bot).ok();
    self.bots[slot_index].voice_channel_id = None;
  }

  fn handle_queue(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    self.sync_voice_state(host);
    let Some(slot_index) = self.same_channel_bot_or_reply(host, &invocation) else {
      return;
    };

    let message = self.bots[slot_index]
      .worker
      .as_ref()
      .map(PlaybackWorker::snapshot)
      .map(|snapshot| snapshot.queue_message())
      .unwrap_or_else(|| "Queue is empty.".to_owned());

    self.send_reply(host, invocation.text_channel_id, &message);
  }

  fn handle_nowplaying(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    self.sync_voice_state(host);
    let Some(slot_index) = self.same_channel_bot_or_reply(host, &invocation) else {
      return;
    };

    let message = self.bots[slot_index]
      .worker
      .as_ref()
      .map(PlaybackWorker::snapshot)
      .map(|snapshot| snapshot.now_playing_message())
      .unwrap_or_else(|| "Nothing is playing.".to_owned());

    self.send_reply(host, invocation.text_channel_id, &message);
  }

  fn handle_skip(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    self.sync_voice_state(host);
    let Some(slot_index) = self.same_channel_bot_or_reply(host, &invocation) else {
      return;
    };

    if let Some(worker) = self.bots[slot_index].worker.as_ref() {
      worker.skip(invocation.text_channel_id);
    } else {
      self.send_reply(host, invocation.text_channel_id, "Nothing to skip.");
    }
  }

  fn requester_voice_channel(
    &mut self,
    host: HostHandle,
    invocation: &ChatCommandInvocationRef<'_>,
  ) -> Option<ChannelId> {
    match host.user_voice_channel(invocation.user_id) {
      Ok(Some(channel_id)) => Some(channel_id),
      Ok(None) => {
        self.send_reply(host, invocation.text_channel_id, "Join a voice channel first.");
        None
      }
      Err(error) => {
        host
          .log(
            LogLevel::Warn,
            &format!("failed to resolve user voice channel: {error}"),
          )
          .ok();
        None
      }
    }
  }

  fn same_channel_bot_or_reply(
    &mut self,
    host: HostHandle,
    invocation: &ChatCommandInvocationRef<'_>,
  ) -> Option<usize> {
    let voice_channel_id = self.requester_voice_channel(host, invocation)?;
    if let Some(slot_index) = self.bot_index_in_voice(voice_channel_id) {
      return Some(slot_index);
    }

    self.send_reply(
      host,
      invocation.text_channel_id,
      "No music bot is active in your voice channel. Use /play first.",
    );
    None
  }

  fn acquire_bot_for_voice(
    &mut self,
    host: HostHandle,
    user_id: UserId,
    voice_channel_id: ChannelId,
  ) -> Result<usize, PluginError> {
    self.sync_voice_state(host);

    if let Some(slot_index) = self.bot_index_in_voice(voice_channel_id) {
      return Ok(slot_index);
    }

    if let Some(slot_index) = self.unjoined_bot_index() {
      self.join_slot_to_voice(host, slot_index, voice_channel_id)?;
      return Ok(slot_index);
    }

    if let Some(slot_index) = self.alone_bot_index(host, voice_channel_id) {
      self.move_slot_to_user_voice(host, slot_index, user_id, voice_channel_id)?;
      return Ok(slot_index);
    }

    let slot_index = self.create_bot_slot(host)?;
    self.join_slot_to_voice(host, slot_index, voice_channel_id)?;
    Ok(slot_index)
  }

  fn bot_index_in_voice(&self, voice_channel_id: ChannelId) -> Option<usize> {
    self
      .bots
      .iter()
      .position(|slot| slot.voice_channel_id == Some(voice_channel_id))
  }

  fn unjoined_bot_index(&self) -> Option<usize> {
    self.bots.iter().position(|slot| slot.voice_channel_id.is_none())
  }

  fn alone_bot_index(&self, host: HostHandle, target_voice_channel_id: ChannelId) -> Option<usize> {
    self.bots.iter().enumerate().find_map(|(index, slot)| {
      let voice_channel_id = slot.voice_channel_id?;
      (voice_channel_id != target_voice_channel_id && self.voice_channel_is_bot_alone(host, voice_channel_id))
        .then_some(index)
    })
  }

  fn voice_channel_is_bot_alone(&self, host: HostHandle, voice_channel_id: ChannelId) -> bool {
    match host.get_voice_channel_info(voice_channel_id) {
      Ok(info) => info.user_count <= 1,
      Err(error) => {
        host
          .log(
            LogLevel::Warn,
            &format!("failed to read voice channel {voice_channel_id} occupancy: {error}"),
          )
          .ok();
        false
      }
    }
  }

  fn join_slot_to_voice(
    &mut self,
    host: HostHandle,
    slot_index: usize,
    voice_channel_id: ChannelId,
  ) -> Result<(), PluginError> {
    host.join_bot_voice(&self.bots[slot_index].bot, voice_channel_id)?;
    self.bots[slot_index].voice_channel_id = Some(voice_channel_id);
    Ok(())
  }

  fn move_slot_to_user_voice(
    &mut self,
    host: HostHandle,
    slot_index: usize,
    user_id: UserId,
    voice_channel_id: ChannelId,
  ) -> Result<(), PluginError> {
    if let Some(worker) = self.bots[slot_index].worker.take() {
      worker.shutdown();
    }
    host.move_bot_to_user_voice(&self.bots[slot_index].bot, user_id)?;
    self.bots[slot_index].voice_channel_id = Some(voice_channel_id);
    Ok(())
  }

  fn ensure_worker_for_slot(&mut self, host: HostHandle, slot_index: usize, voice_channel_id: ChannelId) -> bool {
    if self.bots[slot_index].worker.is_some() {
      return true;
    }

    let Some(sources) = self.sources.as_ref() else {
      host.log(LogLevel::Warn, "music bot sources are not initialized").ok();
      return false;
    };
    let bot = self.bots[slot_index].bot.clone();
    self.bots[slot_index].worker = Some(PlaybackWorker::spawn(host, bot, sources.clone(), voice_channel_id));
    true
  }

  fn create_bot_slot(&mut self, host: HostHandle) -> Result<usize, PluginError> {
    let bot_number = self.next_bot_number.max(1);
    let (key, display_name) = bot_identity(bot_number);
    let bot = host.create_bot_user(&key, &display_name)?;
    self.next_bot_number = bot_number + 1;
    self.bots.push(BotSlot {
      bot,
      voice_channel_id: None,
      worker: None,
    });
    Ok(self.bots.len() - 1)
  }

  fn sync_voice_state(&mut self, host: HostHandle) {
    for slot in self.bots.iter_mut() {
      if slot.worker.as_ref().is_some_and(PlaybackWorker::is_finished)
        && let Some(worker) = slot.worker.take()
      {
        worker.shutdown();
      }

      let actual_voice_channel_id = match host.bot_voice_channel(&slot.bot) {
        Ok(channel_id) => channel_id,
        Err(error) => {
          host
            .log(
              LogLevel::Warn,
              &format!("failed to sync music bot voice state: {error}"),
            )
            .ok();
          continue;
        }
      };

      if actual_voice_channel_id != slot.voice_channel_id {
        if let Some(worker) = slot.worker.take() {
          worker.shutdown();
        }
        slot.voice_channel_id = actual_voice_channel_id;
      }
    }
  }

  fn send_reply(&mut self, host: HostHandle, text_channel_id: ChannelId, message: &str) {
    if self.bots.is_empty()
      && let Err(error) = self.create_bot_slot(host)
    {
      let log_message = format!("failed to create music bot user for reply: {error}; reply was: {message}");
      host.log(LogLevel::Warn, &log_message).ok();
      return;
    }

    if let Some(bot) = self.bots.first().map(|slot| &slot.bot)
      && let Err(error) = host.send_bot_chat(bot, text_channel_id, message)
    {
      let log_message = format!("failed to send music bot reply: {error}");
      host.log(LogLevel::Warn, &log_message).ok();
    }
  }
}

fn bot_identity(bot_number: usize) -> (String, String) {
  if bot_number == 1 {
    ("music".to_owned(), "Music Bot".to_owned())
  } else {
    (format!("music-{bot_number}"), format!("Music Bot {bot_number}"))
  }
}

#[cfg(test)]
mod tests {
  use std::{collections::HashMap, ffi::CStr, os::raw::c_char};

  use server_plugin::{HostRef, MessageId, abi};

  use super::*;
  use crate::{
    queue::{Track, playlist_queue_message},
    sources::model::{SourceKind, SourceRequest},
  };

  #[test]
  fn bot_identity_uses_stable_primary_and_numbered_extra_bots() {
    assert_eq!(bot_identity(1), ("music".to_owned(), "Music Bot".to_owned()));
    assert_eq!(bot_identity(2), ("music-2".to_owned(), "Music Bot 2".to_owned()));
  }

  #[test]
  fn playlist_queue_message_limits_to_five_tracks() {
    let tracks = (0..10).map(track).collect::<Vec<_>>();

    let message = playlist_queue_message(&tracks);

    assert!(message.starts_with("Added 10 tracks:"));
    assert!(message.contains("1) "));
    assert!(message.contains("track 0"));
    assert!(message.contains("track 4"));
    assert!(!message.contains("track 5"));
    assert!(message.contains("... 5 more"));
  }

  #[test]
  fn acquire_bot_reuses_bot_already_in_requester_voice() {
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let mut music_bot = MusicBot::default();
    let slot = music_bot.create_bot_slot(host).unwrap();
    fake.set_bot_voice(1, 10);
    music_bot.bots[slot].voice_channel_id = Some(10);

    let acquired = music_bot.acquire_bot_for_voice(host, 7, 10).unwrap();

    assert_eq!(acquired, slot);
    assert_eq!(fake.created_bot_count, 1);
    assert!(fake.joins.is_empty());
    assert!(fake.moves.is_empty());
  }

  #[test]
  fn acquire_bot_joins_idle_bot_to_requester_voice() {
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let mut music_bot = MusicBot::default();
    let slot = music_bot.create_bot_slot(host).unwrap();

    let acquired = music_bot.acquire_bot_for_voice(host, 7, 10).unwrap();

    assert_eq!(acquired, slot);
    assert_eq!(fake.bot_voice(1), Some(10));
    assert_eq!(fake.joins, vec![(1, 10)]);
    assert!(fake.moves.is_empty());
  }

  #[test]
  fn acquire_bot_moves_bot_that_is_alone_elsewhere() {
    let mut fake = FakeHost::default();
    fake.set_user_voice(7, 10);
    fake.set_channel_user_count(20, 1);
    let host = fake.host_handle();
    let mut music_bot = MusicBot::default();
    let slot = music_bot.create_bot_slot(host).unwrap();
    fake.set_bot_voice(1, 20);
    music_bot.bots[slot].voice_channel_id = Some(20);

    let acquired = music_bot.acquire_bot_for_voice(host, 7, 10).unwrap();

    assert_eq!(acquired, slot);
    assert_eq!(fake.bot_voice(1), Some(10));
    assert_eq!(fake.moves, vec![(1, 7)]);
    assert_eq!(fake.created_bot_count, 1);
  }

  #[test]
  fn acquire_bot_creates_new_bot_when_existing_bot_is_with_users() {
    let mut fake = FakeHost::default();
    fake.set_channel_user_count(20, 2);
    let host = fake.host_handle();
    let mut music_bot = MusicBot::default();
    let slot = music_bot.create_bot_slot(host).unwrap();
    fake.set_bot_voice(1, 20);
    music_bot.bots[slot].voice_channel_id = Some(20);

    let acquired = music_bot.acquire_bot_for_voice(host, 7, 10).unwrap();

    assert_eq!(acquired, 1);
    assert_eq!(music_bot.bots.len(), 2);
    assert_eq!(fake.created_bot_count, 2);
    assert_eq!(fake.bot_voice(2), Some(10));
    assert_eq!(fake.joins, vec![(2, 10)]);
    assert!(fake.moves.is_empty());
  }

  #[test]
  fn same_channel_commands_reject_when_user_has_no_voice_channel() {
    let mut fake = FakeHost::default();
    let host = fake.host_handle();
    let mut music_bot = MusicBot::default();
    music_bot.create_bot_slot(host).unwrap();
    let invocation = invocation(7, 99);

    assert!(music_bot.same_channel_bot_or_reply(host, &invocation).is_none());

    assert_eq!(fake.chats, vec![(1, 99, "Join a voice channel first.".to_owned())]);
  }

  #[test]
  fn same_channel_commands_reject_when_no_bot_is_in_user_voice_channel() {
    let mut fake = FakeHost::default();
    fake.set_user_voice(7, 10);
    let host = fake.host_handle();
    let mut music_bot = MusicBot::default();
    let slot = music_bot.create_bot_slot(host).unwrap();
    fake.set_bot_voice(1, 20);
    music_bot.bots[slot].voice_channel_id = Some(20);
    let invocation = invocation(7, 99);

    assert!(music_bot.same_channel_bot_or_reply(host, &invocation).is_none());

    assert_eq!(
      fake.chats,
      vec![(
        1,
        99,
        "No music bot is active in your voice channel. Use /play first.".to_owned()
      )]
    );
  }

  fn track(index: usize) -> Track {
    Track {
      title: format!("track {index}"),
      duration_ms: Some(180_000),
      source: SourceRequest {
        kind: SourceKind::SoundCloud,
        url: format!("https://soundcloud.com/artist/track-{index}"),
        provider_id: Some(index.to_string()),
        duration_ms: Some(180_000),
        loading_title: format!("track {index}"),
      },
    }
  }

  fn invocation(user_id: UserId, text_channel_id: ChannelId) -> ChatCommandInvocationRef<'static> {
    ChatCommandInvocationRef {
      session_id: 1,
      user_id,
      text_channel_id,
      caller_role: 3,
      command_name: "queue",
      args: "",
      raw_text: "/queue",
      parsed_args: Vec::new(),
    }
  }

  #[derive(Default)]
  struct FakeHost {
    next_bot_id: usize,
    created_bot_count: usize,
    user_voice_channels: HashMap<UserId, ChannelId>,
    bot_voice_channels: HashMap<usize, ChannelId>,
    channel_user_counts: HashMap<ChannelId, u32>,
    joins: Vec<(usize, ChannelId)>,
    moves: Vec<(usize, UserId)>,
    chats: Vec<(usize, ChannelId, String)>,
  }

  impl FakeHost {
    fn host_handle(&mut self) -> HostHandle {
      let mut host = abi::Host::empty();
      host.context = (self as *mut Self).cast();
      host.log = Some(fake_log);
      host.create_bot_user = Some(fake_create_bot_user);
      host.send_bot_chat = Some(fake_send_bot_chat);
      host.join_bot_voice = Some(fake_join_bot_voice);
      host.leave_bot_voice = Some(fake_leave_bot_voice);
      host.user_voice_channel = Some(fake_user_voice_channel);
      host.get_voice_channel_info = Some(fake_get_voice_channel_info);
      host.bot_voice_channel = Some(fake_bot_voice_channel);
      host.move_bot_to_user_voice = Some(fake_move_bot_to_user_voice);
      unsafe { HostRef::from_raw(&host).unwrap().to_handle() }
    }

    fn set_user_voice(&mut self, user_id: UserId, channel_id: ChannelId) {
      self.user_voice_channels.insert(user_id, channel_id);
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

  unsafe extern "C" fn fake_log(_context: *mut std::ffi::c_void, _level: u8, _message: *const c_char) {}

  unsafe extern "C" fn fake_create_bot_user(
    context: *mut std::ffi::c_void,
    _key: *const c_char,
    _display_name: *const c_char,
    out_bot: *mut abi::BotHandle,
    out_user_id: *mut UserId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    fake.next_bot_id += 1;
    fake.created_bot_count += 1;
    unsafe {
      *out_bot = fake.next_bot_id as abi::BotHandle;
      *out_user_id = 100 + fake.next_bot_id as UserId;
    }
    true
  }

  unsafe extern "C" fn fake_send_bot_chat(
    context: *mut std::ffi::c_void,
    bot: abi::BotHandle,
    text_channel_id: ChannelId,
    text: *const c_char,
    out_message_id: *mut MessageId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned();
    fake.chats.push((bot_id(bot), text_channel_id, text));
    unsafe {
      *out_message_id = fake.chats.len() as MessageId;
    }
    true
  }

  unsafe extern "C" fn fake_join_bot_voice(
    context: *mut std::ffi::c_void,
    bot: abi::BotHandle,
    voice_channel_id: ChannelId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    let bot_id = bot_id(bot);
    fake.bot_voice_channels.insert(bot_id, voice_channel_id);
    fake.joins.push((bot_id, voice_channel_id));
    true
  }

  unsafe extern "C" fn fake_leave_bot_voice(context: *mut std::ffi::c_void, bot: abi::BotHandle) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    fake.bot_voice_channels.remove(&bot_id(bot));
    true
  }

  unsafe extern "C" fn fake_user_voice_channel(
    context: *mut std::ffi::c_void,
    user_id: UserId,
    out_voice_channel_id: *mut ChannelId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    unsafe {
      *out_voice_channel_id = fake.user_voice_channels.get(&user_id).copied().unwrap_or(0);
    }
    true
  }

  unsafe extern "C" fn fake_get_voice_channel_info(
    context: *mut std::ffi::c_void,
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
    context: *mut std::ffi::c_void,
    bot: abi::BotHandle,
    out_voice_channel_id: *mut ChannelId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    unsafe {
      *out_voice_channel_id = fake.bot_voice_channels.get(&bot_id(bot)).copied().unwrap_or(0);
    }
    true
  }

  unsafe extern "C" fn fake_move_bot_to_user_voice(
    context: *mut std::ffi::c_void,
    bot: abi::BotHandle,
    user_id: UserId,
  ) -> bool {
    let fake = unsafe { &mut *(context as *mut FakeHost) };
    let Some(channel_id) = fake.user_voice_channels.get(&user_id).copied() else {
      return false;
    };
    let bot_id = bot_id(bot);
    fake.bot_voice_channels.insert(bot_id, channel_id);
    fake.moves.push((bot_id, user_id));
    true
  }

  fn bot_id(bot: abi::BotHandle) -> usize {
    bot as usize
  }
}
