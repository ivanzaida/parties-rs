use std::sync::Arc;

use lurq::app::ctx::Ctx;

use crate::{
  network::protocol::ChannelId,
  session::{
    DEBUG_CHAT_CHANNEL_ID, LobbyTextChannel,
    chat_commands::{ChatCommandRegistry, CommandDefinition},
  },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatChannelKind {
  ServerText,
  Debug,
}

#[derive(Clone, Debug)]
pub(in crate::ui::lobby) struct ChatChannel {
  id: ChannelId,
  name: Arc<str>,
  topic: Arc<str>,
  icon: &'static str,
  kind: ChatChannelKind,
  command_registry: ChatCommandRegistry,
}

impl ChatChannel {
  pub(in crate::ui::lobby) fn server_text(
    ctx: &mut Ctx,
    channel: &LobbyTextChannel,
    command_registry: ChatCommandRegistry,
  ) -> Self {
    Self {
      id: channel.id,
      name: Arc::from(channel.name.as_str()),
      topic: ctx.t("lobby.text_channel.topic"),
      icon: "hash",
      kind: ChatChannelKind::ServerText,
      command_registry,
    }
  }

  pub(in crate::ui::lobby) fn debug(ctx: &mut Ctx) -> Self {
    Self {
      id: DEBUG_CHAT_CHANNEL_ID,
      name: ctx.t("lobby.debug_channels.chat"),
      topic: ctx.t("lobby.debug_channels.topic"),
      icon: "terminal",
      kind: ChatChannelKind::Debug,
      command_registry: ChatCommandRegistry::from_definitions([
        CommandDefinition::local_i18n(
          "/restart-audio-receiver",
          "lobby.text_channel.commands.description.restart_audio_receiver",
          "/restart-audio-receiver {userId:u32}",
        ),
        CommandDefinition::local_i18n(
          "/debug-user",
          "lobby.text_channel.commands.description.debug_user",
          "/debug-user {userId:u32}",
        ),
        CommandDefinition::local_i18n(
          "/debug-voice",
          "lobby.text_channel.commands.description.debug_voice",
          "/debug-voice {userId:u32}",
        ),
        CommandDefinition::local_i18n(
          "/debug-my-voice",
          "lobby.text_channel.commands.description.debug_my_voice",
          "/debug-my-voice",
        ),
        CommandDefinition::local_i18n(
          "/debug-stream",
          "lobby.text_channel.commands.description.debug_stream",
          "/debug-stream {userId:u32}",
        ),
        CommandDefinition::local_i18n(
          "/debug-my-stream",
          "lobby.text_channel.commands.description.debug_my_stream",
          "/debug-my-stream",
        ),
        CommandDefinition::local_i18n(
          "/debug-channel",
          "lobby.text_channel.commands.description.debug_channel",
          "/debug-channel",
        ),
        CommandDefinition::local_i18n(
          "/debug-audio-receivers",
          "lobby.text_channel.commands.description.debug_audio_receivers",
          "/debug-audio-receivers",
        ),
        CommandDefinition::local_i18n(
          "/debug-video-receivers",
          "lobby.text_channel.commands.description.debug_video_receivers",
          "/debug-video-receivers",
        ),
        CommandDefinition::local_i18n(
          "/video-status",
          "lobby.text_channel.commands.description.video_status",
          "/video-status",
        ),
        CommandDefinition::local_i18n(
          "/audio-status",
          "lobby.text_channel.commands.description.audio_status",
          "/audio-status",
        ),
        CommandDefinition::local_i18n(
          "/audio-reset-all",
          "lobby.text_channel.commands.description.audio_reset_all",
          "/audio-reset-all",
        ),
        CommandDefinition::local_i18n(
          "/audio-clear-queue",
          "lobby.text_channel.commands.description.audio_clear_queue",
          "/audio-clear-queue {userId:u32}",
        ),
      ]),
    }
  }

  pub(in crate::ui::lobby) fn id(&self) -> ChannelId {
    self.id
  }

  pub(in crate::ui::lobby) fn name(&self) -> &str {
    &self.name
  }

  pub(in crate::ui::lobby) fn topic(&self) -> &str {
    &self.topic
  }

  pub(in crate::ui::lobby) fn icon(&self) -> &'static str {
    self.icon
  }

  pub(in crate::ui::lobby) fn command_registry(&self) -> ChatCommandRegistry {
    self.command_registry.clone()
  }

  pub(in crate::ui::lobby) fn server_channel_id(&self) -> Option<ChannelId> {
    self.is_server_backed().then_some(self.id)
  }

  pub(in crate::ui::lobby) fn is_server_backed(&self) -> bool {
    self.kind == ChatChannelKind::ServerText
  }

  pub(in crate::ui::lobby) fn shows_text_tools(&self) -> bool {
    self.kind == ChatChannelKind::ServerText
  }

  pub(in crate::ui::lobby) fn empty_title_key(&self) -> &'static str {
    match self.kind {
      ChatChannelKind::ServerText => "lobby.text_channel.empty.title",
      ChatChannelKind::Debug => "lobby.debug_channels.empty.title",
    }
  }

  pub(in crate::ui::lobby) fn empty_description_key(&self) -> &'static str {
    match self.kind {
      ChatChannelKind::ServerText => "lobby.text_channel.empty.description",
      ChatChannelKind::Debug => "lobby.debug_channels.empty.description",
    }
  }
}
