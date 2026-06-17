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

  let handled = handle_chat_command_navigation(&registry, &message_input, &selected_index, "Enter", "Enter");

  assert!(handled);
  assert_eq!(message_input.get_untracked(), "/audio-reset-all ");
}

#[test]
fn enter_does_not_autofill_fully_entered_command() {
  let registry = test_registry();
  let message_input = Signal::new("/audio-status".to_owned());
  let selected_index = Signal::new(0);

  let handled = handle_chat_command_navigation(&registry, &message_input, &selected_index, "Enter", "Enter");

  assert!(!handled);
  assert_eq!(message_input.get_untracked(), "/audio-status");
}

#[test]
fn tab_still_autofills_selected_command_suggestion() {
  let registry = test_registry();
  let message_input = Signal::new("/audio-s".to_owned());
  let selected_index = Signal::new(0);

  let handled = handle_chat_command_navigation(&registry, &message_input, &selected_index, "Tab", "Tab");

  assert!(handled);
  assert_eq!(message_input.get_untracked(), "/audio-status ");
}
