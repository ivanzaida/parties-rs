use super::*;

fn test_registry() -> ChatCommandRegistry {
  ChatCommandRegistry::from_definitions([
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
  ])
}

#[test]
fn enter_autofills_selected_command_suggestion() {
  let registry = test_registry();
  let message_input = Signal::new("/audio-r".to_owned());
  let selected_index = Signal::new(0);

  let handled = handle_chat_command_navigation(&registry, &message_input, &selected_index, None, 0, "Enter", "Enter");

  assert!(handled);
  assert_eq!(message_input.get_untracked(), "/audio-reset-all ");
}

#[test]
fn enter_does_not_autofill_fully_entered_command() {
  let registry = test_registry();
  let message_input = Signal::new("/audio-status".to_owned());
  let selected_index = Signal::new(0);

  let handled = handle_chat_command_navigation(&registry, &message_input, &selected_index, None, 0, "Enter", "Enter");

  assert!(!handled);
  assert_eq!(message_input.get_untracked(), "/audio-status");
}

#[test]
fn tab_still_autofills_selected_command_suggestion() {
  let registry = test_registry();
  let message_input = Signal::new("/audio-s".to_owned());
  let selected_index = Signal::new(0);

  let handled = handle_chat_command_navigation(&registry, &message_input, &selected_index, None, 0, "Tab", "Tab");

  assert!(handled);
  assert_eq!(message_input.get_untracked(), "/audio-status ");
}

#[test]
fn tab_does_not_fill_live_query_result() {
  let registry = ChatCommandRegistry::from_definitions([CommandDefinition::server_advertised_with_inputs(
    "play".to_owned(),
    "Play music".to_owned(),
    "/play {query:string...}".to_owned(),
    vec![crate::session::chat_commands::CommandInputDefinition {
      argument_name: std::sync::Arc::from("query"),
      mode: crate::network::protocol::control::ChatCommandInputMode::LiveQuery,
      min_chars: 2,
      debounce_ms: 400,
      max_results: 10,
      placeholder: std::sync::Arc::from("Search SoundCloud"),
    }],
  )]);
  let response = ChatCommandQueryResponse {
    request_id: 1,
    command_name: "play".to_owned(),
    argument_name: "query".to_owned(),
    status: ChatCommandQueryStatus::Ok,
    message: String::new(),
    results: vec![ChatCommandQueryResult {
      id: "track-1".to_owned(),
      title: "Track".to_owned(),
      subtitle: "Artist".to_owned(),
      value: "https://soundcloud.com/artist/track".to_owned(),
      kind: "track".to_owned(),
      duration_ms: 0,
      thumbnail_url: String::new(),
    }],
  };
  let message_input = Signal::new("/play tr".to_owned());
  let selected_index = Signal::new(0);

  let handled = handle_chat_command_navigation(
    &registry,
    &message_input,
    &selected_index,
    Some(&response),
    1,
    "Tab",
    "Tab",
  );

  assert!(handled);
  assert_eq!(message_input.get_untracked(), "/play tr");
}
