use super::*;

fn debug_registry() -> ChatCommandRegistry {
  ChatCommandRegistry::from_definitions([
    CommandDefinition::local_i18n(
      "/restart-audio-receiver",
      "lobby.text_channel.commands.description.restart_audio_receiver",
      "/restart-audio-receiver {userId:u32}",
    ),
    CommandDefinition::local_i18n(
      "/audio-status",
      "lobby.text_channel.commands.description.audio_status",
      "/audio-status",
    ),
  ])
}

#[test]
fn ignores_regular_chat_messages() {
  assert_eq!(debug_registry().parse("hello").unwrap(), None);
}

#[test]
fn empty_registry_exposes_no_commands() {
  let registry = ChatCommandRegistry::default();
  assert!(!registry.has_commands());
  assert!(registry.definitions().is_empty());
}

#[test]
fn parses_restart_audio_receiver() {
  assert_eq!(
    debug_registry().parse("/restart-audio-receiver 42").unwrap(),
    Some(ChatCommandInvocation {
      name: Arc::from("/restart-audio-receiver"),
      arguments: vec![Arc::from("42")],
      source: ChatCommandSource::Local,
    })
  );
}

#[test]
fn parses_registered_unimplemented_command_as_invocation() {
  assert_eq!(
    debug_registry().parse("/audio-status").unwrap(),
    Some(ChatCommandInvocation {
      name: Arc::from("/audio-status"),
      arguments: Vec::new(),
      source: ChatCommandSource::Local,
    })
  );
}

#[test]
fn restart_audio_receiver_requires_user_id() {
  assert_eq!(
    debug_registry().parse("/restart-audio-receiver").unwrap_err(),
    ChatCommandParseError::Usage {
      command: "/restart-audio-receiver".to_owned(),
      usage: "/restart-audio-receiver {userId:u32}".to_owned(),
    }
  );
}

#[test]
fn restart_audio_receiver_rejects_invalid_user_id_as_invalid_type() {
  assert_eq!(
    debug_registry().parse("/restart-audio-receiver abc").unwrap_err(),
    ChatCommandParseError::InvalidType {
      argument: "userId".to_owned(),
      value: "abc".to_owned(),
      expected: ChatCommandExpectedType::Number {
        min: "1".to_owned(),
        max: u32::MAX.to_string(),
      },
    }
  );
}

#[test]
fn restart_audio_receiver_rejects_zero_user_id_as_invalid_type() {
  assert_eq!(
    debug_registry().parse("/restart-audio-receiver 0").unwrap_err(),
    ChatCommandParseError::InvalidType {
      argument: "userId".to_owned(),
      value: "0".to_owned(),
      expected: ChatCommandExpectedType::Number {
        min: "1".to_owned(),
        max: u32::MAX.to_string(),
      },
    }
  );
}

#[test]
fn exposes_command_definitions() {
  let registry = debug_registry();
  let commands = registry.definitions();
  assert_eq!(commands.len(), 2);
  let first = commands.first().expect("restart command should be registered");
  assert_eq!(first.name.as_ref(), "/restart-audio-receiver");
  assert_eq!(
    first.description_key.as_ref(),
    "lobby.text_channel.commands.description.restart_audio_receiver"
  );
  assert!(first.description_is_i18n_key);
  assert_eq!(first.usage.as_ref(), "/restart-audio-receiver {userId:u32}");
  assert_eq!(first.source, ChatCommandSource::Local);
}

#[test]
fn server_advertised_commands_are_normalized_for_slash_input() {
  let registry = ChatCommandRegistry::from_definitions([CommandDefinition::server_advertised(
    "botping".to_owned(),
    "Ping the bot".to_owned(),
    "/botping [text]".to_owned(),
  )]);

  assert_eq!(
    registry.parse("/botping hello").unwrap(),
    Some(ChatCommandInvocation {
      name: Arc::from("/botping"),
      arguments: vec![Arc::from("hello")],
      source: ChatCommandSource::Server,
    })
  );
  let command = registry.definitions().first().unwrap();
  assert_eq!(command.name.as_ref(), "/botping");
  assert_eq!(command.description_key.as_ref(), "Ping the bot");
  assert!(!command.description_is_i18n_key);
  assert_eq!(command.source, ChatCommandSource::Server);
}

#[test]
fn detects_server_live_query_argument() {
  let registry = ChatCommandRegistry::from_definitions([CommandDefinition::server_advertised_with_inputs(
    "play".to_owned(),
    "Play music".to_owned(),
    "/play {query:string...}".to_owned(),
    vec![CommandInputDefinition {
      argument_name: Arc::from("query"),
      mode: crate::network::protocol::control::ChatCommandInputMode::LiveQuery,
      min_chars: 2,
      debounce_ms: 250,
      max_results: 8,
      placeholder: Arc::from("Search"),
    }],
  )]);

  let query = registry.live_query_for_input("/play black dog").unwrap();
  assert_eq!(query.command_name, "play");
  assert_eq!(query.argument_name, "query");
  assert_eq!(query.query, "black dog");
  assert_eq!(query.cursor_pos, "black dog".len() as u16);
}
