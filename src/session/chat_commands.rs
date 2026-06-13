use crate::network::protocol::UserId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatCommand {
  RestartAudioReceiver { user_id: UserId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInfo {
  pub name: String,
  pub description: String,
  pub usage: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandDefinition {
  pub name: &'static str,
  pub description_key: &'static str,
  pub usage: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatCommandParseError {
  Empty,
  UnterminatedQuotedArgument,
  Usage { command: String, usage: String },
  InvalidUserId,
  UserIdMustBeGreaterThanZero,
  NotImplemented { command: String },
  Unknown { command: String },
}

#[derive(Clone, Debug, Default)]
pub struct ChatCommandRegistry;

const RESTART_AUDIO_RECEIVER_USAGE: &str = "/restart-audio-receiver {userId:u32}";
const COMMAND_DEFINITIONS: [CommandDefinition; 50] = [
  CommandDefinition {
    name: "/restart-audio-receiver",
    description_key: "lobby.text_channel.commands.description.restart_audio_receiver",
    usage: RESTART_AUDIO_RECEIVER_USAGE,
  },
  CommandDefinition {
    name: "/audio-status",
    description_key: "lobby.text_channel.commands.description.audio_status",
    usage: "/audio-status",
  },
  CommandDefinition {
    name: "/audio-reset-all",
    description_key: "lobby.text_channel.commands.description.audio_reset_all",
    usage: "/audio-reset-all",
  },
  CommandDefinition {
    name: "/audio-clear-queue",
    description_key: "lobby.text_channel.commands.description.audio_clear_queue",
    usage: "/audio-clear-queue {userId:u32}",
  },
  CommandDefinition {
    name: "/audio-set-volume",
    description_key: "lobby.text_channel.commands.description.audio_set_volume",
    usage: "/audio-set-volume {userId:u32} {volume:u8}",
  },
  CommandDefinition {
    name: "/audio-muted",
    description_key: "lobby.text_channel.commands.description.audio_muted",
    usage: "/audio-muted",
  },
  CommandDefinition {
    name: "/voice-join",
    description_key: "lobby.text_channel.commands.description.voice_join",
    usage: "/voice-join {channelId:u32}",
  },
  CommandDefinition {
    name: "/voice-leave",
    description_key: "lobby.text_channel.commands.description.voice_leave",
    usage: "/voice-leave",
  },
  CommandDefinition {
    name: "/voice-mute",
    description_key: "lobby.text_channel.commands.description.voice_mute",
    usage: "/voice-mute {userId:u32}",
  },
  CommandDefinition {
    name: "/voice-unmute",
    description_key: "lobby.text_channel.commands.description.voice_unmute",
    usage: "/voice-unmute {userId:u32}",
  },
  CommandDefinition {
    name: "/voice-deafen",
    description_key: "lobby.text_channel.commands.description.voice_deafen",
    usage: "/voice-deafen {userId:u32}",
  },
  CommandDefinition {
    name: "/voice-undeafen",
    description_key: "lobby.text_channel.commands.description.voice_undeafen",
    usage: "/voice-undeafen {userId:u32}",
  },
  CommandDefinition {
    name: "/voice-disconnect",
    description_key: "lobby.text_channel.commands.description.voice_disconnect",
    usage: "/voice-disconnect {userId:u32}",
  },
  CommandDefinition {
    name: "/stream-watch",
    description_key: "lobby.text_channel.commands.description.stream_watch",
    usage: "/stream-watch {userId:u32}",
  },
  CommandDefinition {
    name: "/stream-stop",
    description_key: "lobby.text_channel.commands.description.stream_stop",
    usage: "/stream-stop",
  },
  CommandDefinition {
    name: "/stream-codec",
    description_key: "lobby.text_channel.commands.description.stream_codec",
    usage: "/stream-codec {userId:u32}",
  },
  CommandDefinition {
    name: "/stream-volume",
    description_key: "lobby.text_channel.commands.description.stream_volume",
    usage: "/stream-volume {userId:u32} {volume:u8}",
  },
  CommandDefinition {
    name: "/chat-pin",
    description_key: "lobby.text_channel.commands.description.chat_pin",
    usage: "/chat-pin {messageId:u64}",
  },
  CommandDefinition {
    name: "/chat-unpin",
    description_key: "lobby.text_channel.commands.description.chat_unpin",
    usage: "/chat-unpin {messageId:u64}",
  },
  CommandDefinition {
    name: "/chat-delete",
    description_key: "lobby.text_channel.commands.description.chat_delete",
    usage: "/chat-delete {messageId:u64}",
  },
  CommandDefinition {
    name: "/chat-search",
    description_key: "lobby.text_channel.commands.description.chat_search",
    usage: "/chat-search {query:string}",
  },
  CommandDefinition {
    name: "/chat-history",
    description_key: "lobby.text_channel.commands.description.chat_history",
    usage: "/chat-history {beforeId:u64} {limit:u16}",
  },
  CommandDefinition {
    name: "/text-create",
    description_key: "lobby.text_channel.commands.description.text_create",
    usage: "/text-create {name:string}",
  },
  CommandDefinition {
    name: "/text-delete",
    description_key: "lobby.text_channel.commands.description.text_delete",
    usage: "/text-delete {channelId:u32}",
  },
  CommandDefinition {
    name: "/voice-create",
    description_key: "lobby.text_channel.commands.description.voice_create",
    usage: "/voice-create {name:string} {maxUsers:u32}",
  },
  CommandDefinition {
    name: "/voice-delete",
    description_key: "lobby.text_channel.commands.description.voice_delete",
    usage: "/voice-delete {channelId:u32}",
  },
  CommandDefinition {
    name: "/voice-rename",
    description_key: "lobby.text_channel.commands.description.voice_rename",
    usage: "/voice-rename {channelId:u32} {name:string}",
  },
  CommandDefinition {
    name: "/role-set",
    description_key: "lobby.text_channel.commands.description.role_set",
    usage: "/role-set {userId:u32} {role:role}",
  },
  CommandDefinition {
    name: "/user-kick",
    description_key: "lobby.text_channel.commands.description.user_kick",
    usage: "/user-kick {userId:u32}",
  },
  CommandDefinition {
    name: "/user-info",
    description_key: "lobby.text_channel.commands.description.user_info",
    usage: "/user-info {userId:u32}",
  },
  CommandDefinition {
    name: "/server-ping",
    description_key: "lobby.text_channel.commands.description.server_ping",
    usage: "/server-ping",
  },
  CommandDefinition {
    name: "/server-reconnect",
    description_key: "lobby.text_channel.commands.description.server_reconnect",
    usage: "/server-reconnect",
  },
  CommandDefinition {
    name: "/debug-lobby",
    description_key: "lobby.text_channel.commands.description.debug_lobby",
    usage: "/debug-lobby",
  },
  CommandDefinition {
    name: "/debug-connection",
    description_key: "lobby.text_channel.commands.description.debug_connection",
    usage: "/debug-connection",
  },
  CommandDefinition {
    name: "/debug-video",
    description_key: "lobby.text_channel.commands.description.debug_video",
    usage: "/debug-video",
  },
  CommandDefinition {
    name: "/debug-audio",
    description_key: "lobby.text_channel.commands.description.debug_audio",
    usage: "/debug-audio",
  },
  CommandDefinition {
    name: "/settings-audio",
    description_key: "lobby.text_channel.commands.description.settings_audio",
    usage: "/settings-audio",
  },
  CommandDefinition {
    name: "/settings-stream",
    description_key: "lobby.text_channel.commands.description.settings_stream",
    usage: "/settings-stream",
  },
  CommandDefinition {
    name: "/settings-identity",
    description_key: "lobby.text_channel.commands.description.settings_identity",
    usage: "/settings-identity",
  },
  CommandDefinition {
    name: "/settings-notifications",
    description_key: "lobby.text_channel.commands.description.settings_notifications",
    usage: "/settings-notifications",
  },
  CommandDefinition {
    name: "/help",
    description_key: "lobby.text_channel.commands.description.help",
    usage: "/help {command?:string}",
  },
  CommandDefinition {
    name: "/me",
    description_key: "lobby.text_channel.commands.description.me",
    usage: "/me {text:string}",
  },
  CommandDefinition {
    name: "/shrug",
    description_key: "lobby.text_channel.commands.description.shrug",
    usage: "/shrug {text?:string}",
  },
  CommandDefinition {
    name: "/tableflip",
    description_key: "lobby.text_channel.commands.description.tableflip",
    usage: "/tableflip {text?:string}",
  },
  CommandDefinition {
    name: "/unflip",
    description_key: "lobby.text_channel.commands.description.unflip",
    usage: "/unflip {text?:string}",
  },
  CommandDefinition {
    name: "/roll",
    description_key: "lobby.text_channel.commands.description.roll",
    usage: "/roll {dice:string}",
  },
  CommandDefinition {
    name: "/coin",
    description_key: "lobby.text_channel.commands.description.coin",
    usage: "/coin",
  },
  CommandDefinition {
    name: "/remind",
    description_key: "lobby.text_channel.commands.description.remind",
    usage: "/remind {duration:string} {text:string}",
  },
  CommandDefinition {
    name: "/copy-id",
    description_key: "lobby.text_channel.commands.description.copy_id",
    usage: "/copy-id {kind:choice} {id:u64}",
  },
  CommandDefinition {
    name: "/about",
    description_key: "lobby.text_channel.commands.description.about",
    usage: "/about",
  },
];

impl ChatCommandRegistry {
  pub fn new() -> Self {
    Self
  }

  pub fn definitions(&self) -> &'static [CommandDefinition] {
    &COMMAND_DEFINITIONS
  }

  pub fn parse(&self, input: &str) -> Result<Option<ChatCommand>, ChatCommandParseError> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
      return Ok(None);
    }

    let mut tokens = command_tokens(trimmed)?;
    if tokens.is_empty() {
      return Err(ChatCommandParseError::Empty);
    }

    let command = tokens.remove(0);
    match command.as_str() {
      "/restart-audio-receiver" => {
        if tokens.len() != 1 {
          return Err(ChatCommandParseError::Usage {
            usage: self.usage_for(&command),
            command,
          });
        }
        let user_id = tokens[0]
          .parse::<UserId>()
          .map_err(|_| ChatCommandParseError::InvalidUserId)?;
        if user_id == 0 {
          return Err(ChatCommandParseError::UserIdMustBeGreaterThanZero);
        }
        Ok(Some(ChatCommand::RestartAudioReceiver { user_id }))
      }
      _ if self.definitions().iter().any(|info| info.name == command) => {
        Err(ChatCommandParseError::NotImplemented { command })
      }
      _ => Err(ChatCommandParseError::Unknown { command }),
    }
  }

  fn usage_for(&self, name: &str) -> String {
    self
      .definitions()
      .into_iter()
      .find(|definition| definition.name == name)
      .map(|definition| definition.usage.to_owned())
      .unwrap_or_else(|| name.to_owned())
  }
}

fn command_tokens(input: &str) -> Result<Vec<String>, ChatCommandParseError> {
  let mut tokens = Vec::new();
  let mut current = String::new();
  let mut in_quotes = false;
  let mut escaped = false;

  for ch in input.chars() {
    if escaped {
      current.push(ch);
      escaped = false;
      continue;
    }

    match ch {
      '\\' if in_quotes => escaped = true,
      '"' => in_quotes = !in_quotes,
      ch if ch.is_whitespace() && !in_quotes => {
        if !current.is_empty() {
          tokens.push(std::mem::take(&mut current));
        }
      }
      _ => current.push(ch),
    }
  }

  if escaped {
    current.push('\\');
  }
  if in_quotes {
    return Err(ChatCommandParseError::UnterminatedQuotedArgument);
  }
  if !current.is_empty() {
    tokens.push(current);
  }

  Ok(tokens)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ignores_regular_chat_messages() {
    assert_eq!(ChatCommandRegistry::new().parse("hello").unwrap(), None);
  }

  #[test]
  fn parses_restart_audio_receiver() {
    assert_eq!(
      ChatCommandRegistry::new().parse("/restart-audio-receiver 42").unwrap(),
      Some(ChatCommand::RestartAudioReceiver { user_id: 42 })
    );
  }

  #[test]
  fn restart_audio_receiver_requires_user_id() {
    assert_eq!(
      ChatCommandRegistry::new().parse("/restart-audio-receiver").unwrap_err(),
      ChatCommandParseError::Usage {
        command: "/restart-audio-receiver".to_owned(),
        usage: "/restart-audio-receiver {userId:u32}".to_owned(),
      }
    );
  }

  #[test]
  fn restart_audio_receiver_rejects_invalid_user_id() {
    assert_eq!(
      ChatCommandRegistry::new()
        .parse("/restart-audio-receiver abc")
        .unwrap_err(),
      ChatCommandParseError::InvalidUserId
    );
  }

  #[test]
  fn exposes_command_definitions() {
    let commands = ChatCommandRegistry::new().definitions();
    assert_eq!(commands.len(), 50);
    assert_eq!(
      commands.first(),
      Some(&CommandDefinition {
        name: "/restart-audio-receiver",
        description_key: "lobby.text_channel.commands.description.restart_audio_receiver",
        usage: "/restart-audio-receiver {userId:u32}",
      })
    );
  }
}
