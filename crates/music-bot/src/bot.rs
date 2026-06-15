use server_plugin::{
  BotUser, ChannelId, ChatCommandInvocationRef, HostHandle, PluginError,
  abi::LogLevel,
  plugin::{Context, Plugin},
};

use crate::{
  commands::command_definitions,
  config::BotConfig,
  player::PlaybackWorker,
  queue::Track,
  sources::{registry::SourceRegistry, soundcloud::SoundCloudTokenProvider},
};

#[derive(Default)]
pub(crate) struct MusicBot {
  host: Option<HostHandle>,
  bot: Option<BotUser>,
  config: Option<BotConfig>,
  sources: Option<SourceRegistry>,
  voice_channel_id: Option<ChannelId>,
  worker: Option<PlaybackWorker>,
}

impl Plugin for MusicBot {
  fn init(&mut self, context: &mut Context<'_>) -> Result<(), PluginError> {
    let host = context.host();
    self.host = Some(host);
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

    self.bot = match host.create_bot_user("music", "Music Bot") {
      Ok(bot) => Some(bot),
      Err(error) => {
        host.log(LogLevel::Warn, &format!("music-bot failed to create bot user: {error}"))?;
        return Err(error);
      }
    };

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

  fn shutdown(&mut self) {
    if let Some(worker) = self.worker.take() {
      worker.shutdown();
    }
    if let Some(sources) = self.sources.take() {
      sources.shutdown();
    }

    if let Some(host) = self.host {
      if let Some(bot) = self.bot.as_ref()
        && self.voice_channel_id.is_some()
      {
        host.leave_bot_voice(bot).ok();
      }

      if let Some(bot) = self.bot.take() {
        host.destroy_bot_user(&bot).ok();
      }
    } else {
      self.bot = None;
    }

    self.voice_channel_id = None;
    self.config = None;
    self.sources = None;
    self.host = None;
  }
}

impl MusicBot {
  fn handle_play(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    let Some(url_arg) = invocation.arg("url").filter(|arg| arg.present) else {
      self.send_reply(host, invocation.text_channel_id, "Usage: /play {url:string}");
      return;
    };
    let Some(sources) = self.sources.as_ref() else {
      self.send_reply(host, invocation.text_channel_id, "Music bot is not configured.");
      return;
    };

    let track = match Track::parse(url_arg.string_value, sources) {
      Ok(track) => track,
      Err(error) => {
        self.send_reply(host, invocation.text_channel_id, &error);
        return;
      }
    };

    let voice_channel_id = match host.user_voice_channel(invocation.user_id) {
      Ok(Some(channel_id)) => channel_id,
      Ok(None) => {
        self.send_reply(host, invocation.text_channel_id, "Join a voice channel first.");
        return;
      }
      Err(error) => {
        let message = format!("failed to resolve user voice channel: {error}");
        host.log(LogLevel::Warn, &message).ok();
        return;
      }
    };

    let Some(bot) = self.bot.as_ref() else {
      host.log(LogLevel::Warn, "music bot user is not initialized").ok();
      return;
    };

    if self.voice_channel_id != Some(voice_channel_id) {
      if let Err(error) = host.join_bot_voice(bot, voice_channel_id) {
        let message = format!("failed to join voice channel {voice_channel_id}: {error}");
        host.log(LogLevel::Warn, &message).ok();
        return;
      }
      self.voice_channel_id = Some(voice_channel_id);
    }

    if !self.ensure_worker(host) {
      return;
    }

    if let Some(worker) = self.worker.as_ref() {
      worker.enqueue(track, invocation.text_channel_id);
    }
  }

  fn handle_stop(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    if let Some(worker) = self.worker.as_ref() {
      worker.stop(invocation.text_channel_id);
    } else {
      self.send_reply(host, invocation.text_channel_id, "Nothing is playing.");
    }

    if let Some(bot) = self.bot.as_ref()
      && self.voice_channel_id.is_some()
    {
      host.leave_bot_voice(bot).ok();
    }

    self.voice_channel_id = None;
  }

  fn handle_queue(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    let message = self
      .worker
      .as_ref()
      .map(PlaybackWorker::snapshot)
      .map(|snapshot| snapshot.queue_message())
      .unwrap_or_else(|| "Queue is empty.".to_owned());

    self.send_reply(host, invocation.text_channel_id, &message);
  }

  fn handle_nowplaying(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    let message = self
      .worker
      .as_ref()
      .map(PlaybackWorker::snapshot)
      .map(|snapshot| snapshot.now_playing_message())
      .unwrap_or_else(|| "Nothing is playing yet.".to_owned());

    self.send_reply(host, invocation.text_channel_id, &message);
  }

  fn handle_skip(&mut self, host: HostHandle, invocation: ChatCommandInvocationRef<'_>) {
    if let Some(worker) = self.worker.as_ref() {
      worker.skip(invocation.text_channel_id);
    } else {
      self.send_reply(host, invocation.text_channel_id, "Nothing to skip.");
    }
  }

  fn ensure_bot(&mut self, host: HostHandle) -> Result<(), PluginError> {
    if self.bot.is_none() {
      self.bot = Some(host.create_bot_user("music", "Music Bot")?);
    }
    Ok(())
  }

  fn ensure_worker(&mut self, host: HostHandle) -> bool {
    if self.worker.is_none() {
      let Some(bot) = self.bot.as_ref() else {
        host.log(LogLevel::Warn, "music bot user is not initialized").ok();
        return false;
      };
      let Some(sources) = self.sources.as_ref() else {
        host.log(LogLevel::Warn, "music bot sources are not initialized").ok();
        return false;
      };
      self.worker = Some(PlaybackWorker::spawn(host, bot.clone(), sources.clone()));
    }
    true
  }

  fn send_reply(&mut self, host: HostHandle, text_channel_id: ChannelId, message: &str) {
    if let Err(error) = self.ensure_bot(host) {
      let log_message = format!("failed to create music bot user for reply: {error}; reply was: {message}");
      host.log(LogLevel::Warn, &log_message).ok();
      return;
    }

    if let Some(bot) = self.bot.as_ref()
      && let Err(error) = host.send_bot_chat(bot, text_channel_id, message)
    {
      let log_message = format!("failed to send music bot reply: {error}");
      host.log(LogLevel::Warn, &log_message).ok();
    }
  }
}
