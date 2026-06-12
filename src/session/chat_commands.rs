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

#[derive(Clone, Debug, Default)]
pub struct ChatCommandRegistry;

const RESTART_AUDIO_RECEIVER_USAGE: &str = "/restart-audio-receiver {userId:u32}";
const COMMAND_INFOS: [(&str, &str, &str); 50] = [
  (
    "/restart-audio-receiver",
    "Restart the local audio receiver state for a remote user.",
    RESTART_AUDIO_RECEIVER_USAGE,
  ),
  ("/audio-status", "Show local audio engine status.", "/audio-status"),
  (
    "/audio-reset-all",
    "Restart all local voice audio receivers.",
    "/audio-reset-all",
  ),
  (
    "/audio-clear-queue",
    "Clear queued voice audio for a user.",
    "/audio-clear-queue {userId:u32}",
  ),
  (
    "/audio-set-volume",
    "Set local playback volume for a user.",
    "/audio-set-volume {userId:u32} {volume:u8}",
  ),
  ("/audio-muted", "Show local mute and deafen state.", "/audio-muted"),
  (
    "/voice-join",
    "Join a voice channel by id.",
    "/voice-join {channelId:u32}",
  ),
  ("/voice-leave", "Leave the current voice channel.", "/voice-leave"),
  (
    "/voice-mute",
    "Mute a user through moderation controls.",
    "/voice-mute {userId:u32}",
  ),
  (
    "/voice-unmute",
    "Unmute a user through moderation controls.",
    "/voice-unmute {userId:u32}",
  ),
  (
    "/voice-deafen",
    "Deafen a user through moderation controls.",
    "/voice-deafen {userId:u32}",
  ),
  (
    "/voice-undeafen",
    "Undeafen a user through moderation controls.",
    "/voice-undeafen {userId:u32}",
  ),
  (
    "/voice-disconnect",
    "Disconnect a user from voice.",
    "/voice-disconnect {userId:u32}",
  ),
  (
    "/stream-watch",
    "Watch a user's active stream.",
    "/stream-watch {userId:u32}",
  ),
  ("/stream-stop", "Stop watching the current stream.", "/stream-stop"),
  (
    "/stream-codec",
    "Show stream codec metadata for a user.",
    "/stream-codec {userId:u32}",
  ),
  (
    "/stream-volume",
    "Set local stream audio volume.",
    "/stream-volume {userId:u32} {volume:u8}",
  ),
  ("/chat-pin", "Pin a chat message by id.", "/chat-pin {messageId:u64}"),
  (
    "/chat-unpin",
    "Unpin a chat message by id.",
    "/chat-unpin {messageId:u64}",
  ),
  (
    "/chat-delete",
    "Delete a chat message by id.",
    "/chat-delete {messageId:u64}",
  ),
  (
    "/chat-search",
    "Search messages in the current text channel.",
    "/chat-search {query:string}",
  ),
  (
    "/chat-history",
    "Request older chat history.",
    "/chat-history {beforeId:u64} {limit:u16}",
  ),
  ("/text-create", "Create a text channel.", "/text-create {name:string}"),
  ("/text-delete", "Delete a text channel.", "/text-delete {channelId:u32}"),
  (
    "/voice-create",
    "Create a voice channel.",
    "/voice-create {name:string} {maxUsers:u32}",
  ),
  (
    "/voice-delete",
    "Delete a voice channel.",
    "/voice-delete {channelId:u32}",
  ),
  (
    "/voice-rename",
    "Rename a voice channel.",
    "/voice-rename {channelId:u32} {name:string}",
  ),
  (
    "/role-set",
    "Set a user's server role.",
    "/role-set {userId:u32} {role:role}",
  ),
  ("/user-kick", "Kick a user from the server.", "/user-kick {userId:u32}"),
  (
    "/user-info",
    "Show cached lobby information for a user.",
    "/user-info {userId:u32}",
  ),
  ("/server-ping", "Show current server ping.", "/server-ping"),
  (
    "/server-reconnect",
    "Reconnect to the current server.",
    "/server-reconnect",
  ),
  ("/debug-lobby", "Show lobby debug state.", "/debug-lobby"),
  ("/debug-connection", "Show connection debug state.", "/debug-connection"),
  ("/debug-video", "Show video runtime debug state.", "/debug-video"),
  ("/debug-audio", "Show audio runtime debug state.", "/debug-audio"),
  ("/settings-audio", "Open audio settings.", "/settings-audio"),
  ("/settings-stream", "Open stream settings.", "/settings-stream"),
  ("/settings-identity", "Open identity settings.", "/settings-identity"),
  (
    "/settings-notifications",
    "Open notification settings.",
    "/settings-notifications",
  ),
  ("/help", "Show available commands.", "/help {command?:string}"),
  ("/me", "Send text with emphasis.", "/me {text:string}"),
  ("/shrug", "Send a shrug message.", "/shrug {text?:string}"),
  ("/tableflip", "Send a table flip message.", "/tableflip {text?:string}"),
  ("/unflip", "Send an unflip message.", "/unflip {text?:string}"),
  ("/roll", "Roll dice using NdM syntax.", "/roll {dice:string}"),
  ("/coin", "Flip a coin.", "/coin"),
  (
    "/remind",
    "Create a local reminder.",
    "/remind {duration:string} {text:string}",
  ),
  (
    "/copy-id",
    "Copy a user, channel, or message id.",
    "/copy-id {kind:choice} {id:u64}",
  ),
  ("/about", "Show application build information.", "/about"),
];

impl ChatCommandRegistry {
  pub fn new() -> Self {
    Self
  }

  pub fn commands(&self) -> Vec<CommandInfo> {
    COMMAND_INFOS
      .iter()
      .map(|(name, description, usage)| CommandInfo {
        name: (*name).to_owned(),
        description: (*description).to_owned(),
        usage: (*usage).to_owned(),
      })
      .collect()
  }

  pub fn parse(&self, input: &str) -> Result<Option<ChatCommand>, String> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
      return Ok(None);
    }

    let mut tokens = command_tokens(trimmed)?;
    if tokens.is_empty() {
      return Err("Command is empty.".to_owned());
    }

    let command = tokens.remove(0);
    match command.as_str() {
      "/restart-audio-receiver" => {
        if tokens.len() != 1 {
          return Err(format!("Usage: {}", self.usage_for(&command)));
        }
        let user_id = tokens[0]
          .parse::<UserId>()
          .map_err(|_| "userId must be a positive numeric user id.".to_owned())?;
        if user_id == 0 {
          return Err("userId must be greater than zero.".to_owned());
        }
        Ok(Some(ChatCommand::RestartAudioReceiver { user_id }))
      }
      _ if self.commands().iter().any(|info| info.name == command) => Err(format!(
        "{command} is registered for UI preview but is not implemented yet."
      )),
      _ => Err(format!("Unknown command: {command}")),
    }
  }

  fn usage_for(&self, name: &str) -> String {
    self
      .commands()
      .into_iter()
      .find(|command| command.name == name)
      .map(|command| command.usage)
      .unwrap_or_else(|| name.to_owned())
  }
}

fn command_tokens(input: &str) -> Result<Vec<String>, String> {
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
    return Err("Command contains an unterminated quoted argument.".to_owned());
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
      "Usage: /restart-audio-receiver {userId:u32}"
    );
  }

  #[test]
  fn restart_audio_receiver_rejects_invalid_user_id() {
    assert_eq!(
      ChatCommandRegistry::new()
        .parse("/restart-audio-receiver abc")
        .unwrap_err(),
      "userId must be a positive numeric user id."
    );
  }

  #[test]
  fn exposes_command_info() {
    let commands = ChatCommandRegistry::new().commands();
    assert_eq!(commands.len(), 50);
    assert_eq!(
      commands.first(),
      Some(&CommandInfo {
        name: "/restart-audio-receiver".to_owned(),
        description: "Restart the local audio receiver state for a remote user.".to_owned(),
        usage: "/restart-audio-receiver {userId:u32}".to_owned(),
      })
    );
  }
}
