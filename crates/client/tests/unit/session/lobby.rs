use super::*;

fn test_context() -> ServerMessageContext {
  ServerMessageContext {
    local_user_id: Some(4),
    local_display_name: "local".to_owned(),
    local_voice_state: (false, false),
    pending_keepalive_ping: None,
  }
}

#[test]
fn mention_detection_matches_at_display_name() {
  assert!(message_mentions_display_name("hey @Lurk", "lurk"));
}

#[test]
fn mention_detection_matches_display_name_token() {
  assert!(message_mentions_display_name("thanks Lurk!", "lurk"));
}

#[test]
fn mention_detection_does_not_match_partial_words() {
  assert!(!message_mentions_display_name("the lurking issue", "lurk"));
}

#[test]
fn chat_command_list_updates_server_command_registry() {
  let mut lobby = LobbyState::default();

  apply_server_message(
    &mut lobby,
    S2C::ChatCommandList(crate::network::protocol::control::ChatCommandList {
      commands: vec![crate::network::protocol::control::ChatCommandInfo {
        name: "botping".to_owned(),
        description: "Ping the bot".to_owned(),
        usage: "/botping [text]".to_owned(),
      }],
    }),
    test_context(),
  );

  let definitions = lobby.chat_command_registry.definitions();
  assert_eq!(definitions.len(), 1);
  assert_eq!(definitions[0].name.as_ref(), "/botping");
  assert_eq!(definitions[0].description_key.as_ref(), "Ping the bot");
  assert!(!definitions[0].description_is_i18n_key);
  assert_eq!(
    lobby.chat_command_registry.parse("/botping hello").unwrap(),
    Some(super::super::chat_commands::ChatCommandInvocation {
      name: "/botping".into(),
      arguments: vec!["hello".into()],
      source: super::super::chat_commands::ChatCommandSource::Server,
    })
  );
}

#[test]
fn joining_voice_channel_preserves_current_text_view() {
  let mut lobby = LobbyState {
    selected_text_channel_id: Some(10),
    ..LobbyState::default()
  };

  select_channel(&mut lobby, 1);

  assert_eq!(lobby.selected_channel_id, Some(1));
  assert_eq!(lobby.selected_text_channel_id, Some(10));
  assert!(!lobby.debug_chat_selected);
  assert_eq!(lobby.stream_browser_channel_id, None);
}

#[test]
fn watching_stream_in_joined_voice_channel_opens_voice_view() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    selected_text_channel_id: Some(10),
    users_by_channel: HashMap::from([(
      1,
      vec![LobbyUser {
        user_id: 4,
        username: "streamer".to_owned(),
        role: Role::User,
        muted: false,
        deafened: false,
        speaking: false,
      }],
    )]),
    ..LobbyState::default()
  };

  set_watching_user(&mut lobby, Some(4));

  assert_eq!(lobby.watching_user_id, Some(4));
  assert_eq!(lobby.stream_browser_channel_id, Some(1));
  assert_eq!(lobby.selected_text_channel_id, None);
  assert!(!lobby.debug_chat_selected);
}

#[test]
fn watching_stream_outside_joined_voice_channel_preserves_current_text_view() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    selected_text_channel_id: Some(10),
    users_by_channel: HashMap::from([(
      2,
      vec![LobbyUser {
        user_id: 4,
        username: "streamer".to_owned(),
        role: Role::User,
        muted: false,
        deafened: false,
        speaking: false,
      }],
    )]),
    ..LobbyState::default()
  };

  set_watching_user(&mut lobby, Some(4));

  assert_eq!(lobby.watching_user_id, Some(4));
  assert_eq!(lobby.stream_browser_channel_id, None);
  assert_eq!(lobby.selected_text_channel_id, Some(10));
}

#[test]
fn kicked_server_error_marks_disconnect_without_auto_reconnect() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    receiver_running: true,
    watching_user_id: Some(7),
    ..LobbyState::default()
  };
  let effects = apply_server_message(
    &mut lobby,
    S2C::ServerError {
      code: ServerErrorCode::Kicked,
      message: "kicked by admin".to_owned(),
    },
    test_context(),
  );

  assert!(lobby.disconnected);
  assert!(lobby.auto_reconnect_disabled);
  assert!(!lobby.receiver_running);
  assert_eq!(lobby.last_error.as_deref(), Some("kicked by admin"));
  assert_eq!(lobby.watching_user_id, None);
  assert!(effects.stop_local_voice);
  assert_eq!(effects.notification_sound, Some(NotificationSound::UserKicked));
  assert_eq!(effects.watching_change, Some(Some(7)));
}
