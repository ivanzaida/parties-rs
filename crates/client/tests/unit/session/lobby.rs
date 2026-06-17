use super::*;

fn test_context() -> ServerMessageContext {
  ServerMessageContext {
    local_user_id: Some(4),
    local_display_name: "local".to_owned(),
    local_voice_state: (false, false),
    pending_keepalive_ping: None,
  }
}

fn screen_share_metadata(width: u16, height: u16) -> crate::network::protocol::control::ScreenShareMetadata {
  crate::network::protocol::control::ScreenShareMetadata {
    codec: crate::network::protocol::VideoCodecId::H264,
    width,
    height,
  }
}

fn channel_info(id: ChannelId, sort_order: u32) -> crate::network::protocol::control::ChannelInfo {
  crate::network::protocol::control::ChannelInfo {
    id,
    name: format!("Voice {id}"),
    max_users: 0,
    sort_order,
    user_count: 0,
  }
}

fn text_channel_info(id: ChannelId, sort_order: u32) -> crate::network::protocol::control::TextChannelInfo {
  crate::network::protocol::control::TextChannelInfo {
    id,
    name: format!("Text {id}"),
    sort_order,
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
fn channel_list_preserves_existing_selection_and_syncs_users() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(2),
    stream_browser_channel_id: Some(2),
    users_by_channel: HashMap::from([(
      2,
      vec![LobbyUser {
        user_id: 7,
        username: "remote".to_owned(),
        role: Role::User,
        muted: false,
        deafened: false,
        speaking: true,
      }],
    )]),
    ..LobbyState::default()
  };

  apply_server_message(
    &mut lobby,
    S2C::ChannelList(crate::network::protocol::control::ChannelList {
      channels: vec![channel_info(2, 20), channel_info(1, 10)],
    }),
    test_context(),
  );

  assert!(lobby.channel_list_received);
  assert_eq!(
    lobby.channels.iter().map(|channel| channel.id).collect::<Vec<_>>(),
    vec![1, 2]
  );
  assert_eq!(lobby.selected_channel_id, Some(2));
  assert_eq!(lobby.stream_browser_channel_id, Some(2));
  assert_eq!(lobby.users.len(), 1);
  assert_eq!(lobby.users[0].user_id, 7);
}

#[test]
fn channel_list_prunes_removed_channels_and_clears_missing_selection() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(2),
    stream_browser_channel_id: Some(2),
    users: vec![LobbyUser {
      user_id: 7,
      username: "remote".to_owned(),
      role: Role::User,
      muted: false,
      deafened: false,
      speaking: true,
    }],
    users_by_channel: HashMap::from([
      (
        1,
        vec![LobbyUser {
          user_id: 4,
          username: "local".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: false,
        }],
      ),
      (
        2,
        vec![LobbyUser {
          user_id: 7,
          username: "remote".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: true,
        }],
      ),
    ]),
    ..LobbyState::default()
  };

  apply_server_message(
    &mut lobby,
    S2C::ChannelList(crate::network::protocol::control::ChannelList {
      channels: vec![channel_info(1, 10)],
    }),
    test_context(),
  );

  assert!(lobby.channel_list_received);
  assert_eq!(lobby.selected_channel_id, None);
  assert_eq!(lobby.stream_browser_channel_id, None);
  assert!(lobby.users.is_empty());
  assert!(lobby.users_by_channel.contains_key(&1));
  assert!(!lobby.users_by_channel.contains_key(&2));
}

#[test]
fn chat_channel_list_prunes_removed_channel_state_and_selects_first_available() {
  let mut lobby = LobbyState {
    selected_text_channel_id: Some(3),
    unread_text_channel_ids: HashSet::from([2, 3]),
    chat_history_loading: HashSet::from([2, 3]),
    chat_history_has_more: HashMap::from([(2, true), (3, true)]),
    chat_messages_by_channel: HashMap::from([
      (
        2,
        vec![crate::network::protocol::control::ChatMessage {
          id: 10,
          channel_id: 2,
          sender_id: 7,
          sender_name: "remote".to_owned(),
          timestamp: 1,
          text: "kept".to_owned(),
          pinned: false,
          attachments: Vec::new(),
        }],
      ),
      (
        3,
        vec![crate::network::protocol::control::ChatMessage {
          id: 11,
          channel_id: 3,
          sender_id: 7,
          sender_name: "remote".to_owned(),
          timestamp: 1,
          text: "removed".to_owned(),
          pinned: false,
          attachments: Vec::new(),
        }],
      ),
    ]),
    ..LobbyState::default()
  };

  apply_server_message(
    &mut lobby,
    S2C::ChatChannelList {
      channels: vec![text_channel_info(2, 20), text_channel_info(1, 10)],
    },
    test_context(),
  );

  assert_eq!(
    lobby.text_channels.iter().map(|channel| channel.id).collect::<Vec<_>>(),
    vec![1, 2]
  );
  assert_eq!(lobby.selected_text_channel_id, Some(1));
  assert_eq!(lobby.unread_text_channel_ids, HashSet::from([2]));
  assert_eq!(lobby.chat_history_loading, HashSet::from([2]));
  assert_eq!(lobby.chat_history_has_more, HashMap::from([(2, true)]));
  assert!(lobby.chat_messages_by_channel.contains_key(&2));
  assert!(!lobby.chat_messages_by_channel.contains_key(&3));
}

#[test]
fn chat_channel_list_preserves_debug_chat_selection() {
  let mut lobby = LobbyState {
    selected_text_channel_id: Some(2),
    debug_chat_selected: true,
    ..LobbyState::default()
  };

  apply_server_message(
    &mut lobby,
    S2C::ChatChannelList {
      channels: vec![text_channel_info(1, 10), text_channel_info(2, 20)],
    },
    test_context(),
  );

  assert_eq!(lobby.selected_text_channel_id, None);
  assert!(lobby.debug_chat_selected);
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
fn local_leave_channel_emits_speaking_reset_effect() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    watching_user_id: Some(4),
    screen_shares: vec![
      LobbyScreenShare {
        sharer_user_id: 4,
        metadata: screen_share_metadata(1280, 720),
      },
      LobbyScreenShare {
        sharer_user_id: 7,
        metadata: screen_share_metadata(1920, 1080),
      },
    ],
    users_by_channel: HashMap::from([(
      1,
      vec![
        LobbyUser {
          user_id: 4,
          username: "local".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: true,
        },
        LobbyUser {
          user_id: 7,
          username: "remote".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: true,
        },
      ],
    )]),
    users: vec![LobbyUser {
      user_id: 4,
      username: "local".to_owned(),
      role: Role::User,
      muted: false,
      deafened: false,
      speaking: true,
    }],
    ..LobbyState::default()
  };

  let effects = leave_channel_locally(&mut lobby, Some(4));

  assert!(effects.left_voice);
  assert_eq!(effects.forget_speaking_user, Some(4));
  assert_eq!(effects.clear_video_cache_user, Some(4));
  assert_eq!(effects.watching_change, Some(Some(4)));
  assert_eq!(lobby.selected_channel_id, None);
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.users.is_empty());
  assert_eq!(lobby.screen_shares.len(), 1);
  assert_eq!(lobby.screen_shares[0].sharer_user_id, 7);
  assert_eq!(lobby.users_by_channel[&1].len(), 1);
  assert_eq!(lobby.users_by_channel[&1][0].user_id, 7);
}

#[test]
fn local_user_left_channel_message_clears_speaking_and_stops_voice() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    users_by_channel: HashMap::from([(
      1,
      vec![LobbyUser {
        user_id: 4,
        username: "local".to_owned(),
        role: Role::User,
        muted: false,
        deafened: false,
        speaking: true,
      }],
    )]),
    users: vec![LobbyUser {
      user_id: 4,
      username: "local".to_owned(),
      role: Role::User,
      muted: false,
      deafened: false,
      speaking: true,
    }],
    ..LobbyState::default()
  };

  let effects = apply_server_message(
    &mut lobby,
    S2C::UserLeftChannel(crate::network::protocol::control::UserLeftChannel {
      user_id: 4,
      channel_id: 1,
    }),
    test_context(),
  );

  assert!(effects.stop_local_voice);
  assert_eq!(effects.clear_speaking_user, Some(4));
  assert_eq!(lobby.selected_channel_id, None);
  assert!(lobby.users.is_empty());
  assert!(lobby.users_by_channel.values().all(Vec::is_empty));
}

#[test]
fn remote_user_left_selected_channel_clears_watch_and_speaking_effects() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    watching_user_id: Some(7),
    screen_shares: vec![LobbyScreenShare {
      sharer_user_id: 7,
      metadata: screen_share_metadata(1280, 720),
    }],
    users_by_channel: HashMap::from([(
      1,
      vec![
        LobbyUser {
          user_id: 4,
          username: "local".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: false,
        },
        LobbyUser {
          user_id: 7,
          username: "remote".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: true,
        },
      ],
    )]),
    ..LobbyState::default()
  };
  sync_selected_users(&mut lobby);

  let effects = apply_server_message(
    &mut lobby,
    S2C::UserLeftChannel(crate::network::protocol::control::UserLeftChannel {
      user_id: 7,
      channel_id: 1,
    }),
    test_context(),
  );

  assert!(!effects.stop_local_voice);
  assert_eq!(effects.clear_speaking_user, Some(7));
  assert_eq!(effects.clear_video_cache_users, vec![7]);
  assert_eq!(effects.notification_sound, Some(NotificationSound::VoiceLeave));
  assert_eq!(effects.watching_change, Some(Some(7)));
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.screen_shares.is_empty());
  assert_eq!(lobby.users.len(), 1);
  assert_eq!(lobby.users[0].user_id, 4);
}

#[test]
fn watched_stream_stop_clears_watch_and_video_cache() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    watching_user_id: Some(7),
    screen_shares: vec![LobbyScreenShare {
      sharer_user_id: 7,
      metadata: screen_share_metadata(1280, 720),
    }],
    users_by_channel: HashMap::from([(
      1,
      vec![
        LobbyUser {
          user_id: 4,
          username: "local".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: false,
        },
        LobbyUser {
          user_id: 7,
          username: "remote".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: false,
        },
      ],
    )]),
    ..LobbyState::default()
  };

  let effects = apply_server_message(
    &mut lobby,
    S2C::ScreenShareStopped { sharer_user_id: 7 },
    test_context(),
  );

  assert_eq!(effects.clear_video_cache_users, vec![7]);
  assert_eq!(effects.watching_change, Some(Some(7)));
  assert_eq!(effects.notification_sound, Some(NotificationSound::StreamEnded));
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.screen_shares.is_empty());
}

#[test]
fn unwatched_stream_stop_clears_cache_without_watch_change() {
  let mut lobby = LobbyState {
    watching_user_id: Some(8),
    screen_shares: vec![
      LobbyScreenShare {
        sharer_user_id: 7,
        metadata: screen_share_metadata(1280, 720),
      },
      LobbyScreenShare {
        sharer_user_id: 8,
        metadata: screen_share_metadata(1920, 1080),
      },
    ],
    ..LobbyState::default()
  };

  let effects = apply_server_message(
    &mut lobby,
    S2C::ScreenShareStopped { sharer_user_id: 7 },
    test_context(),
  );

  assert_eq!(effects.clear_video_cache_users, vec![7]);
  assert_eq!(effects.watching_change, None);
  assert_eq!(effects.notification_sound, None);
  assert_eq!(lobby.watching_user_id, Some(8));
  assert_eq!(lobby.screen_shares.len(), 1);
  assert_eq!(lobby.screen_shares[0].sharer_user_id, 8);
}

#[test]
fn duplicate_screen_share_started_updates_existing_metadata() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    screen_shares: vec![LobbyScreenShare {
      sharer_user_id: 7,
      metadata: screen_share_metadata(1280, 720),
    }],
    users_by_channel: HashMap::from([(
      1,
      vec![LobbyUser {
        user_id: 7,
        username: "remote".to_owned(),
        role: Role::User,
        muted: false,
        deafened: false,
        speaking: false,
      }],
    )]),
    ..LobbyState::default()
  };

  let effects = apply_server_message(
    &mut lobby,
    S2C::ScreenShareStarted(crate::network::protocol::control::ScreenShareStarted {
      sharer_user_id: 7,
      metadata: screen_share_metadata(1920, 1080),
    }),
    test_context(),
  );

  assert_eq!(effects.notification_sound, Some(NotificationSound::StreamStarted));
  assert_eq!(lobby.screen_shares.len(), 1);
  assert_eq!(lobby.screen_shares[0].sharer_user_id, 7);
  assert_eq!(lobby.screen_shares[0].metadata, screen_share_metadata(1920, 1080));
}

#[test]
fn remote_joined_other_channel_moves_user_out_of_selected_channel() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    users_by_channel: HashMap::from([
      (
        1,
        vec![LobbyUser {
          user_id: 7,
          username: "remote".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: true,
        }],
      ),
      (2, Vec::new()),
    ]),
    ..LobbyState::default()
  };
  sync_selected_users(&mut lobby);

  let effects = apply_server_message(
    &mut lobby,
    S2C::UserJoinedChannel(crate::network::protocol::control::UserJoinedChannel {
      user_id: 7,
      username: "remote".to_owned(),
      channel_id: 2,
      role: Role::User,
    }),
    test_context(),
  );

  assert_eq!(effects.notification_sound, Some(NotificationSound::VoiceLeave));
  assert!(lobby.users.is_empty());
  assert!(lobby.users_by_channel[&1].is_empty());
  assert_eq!(lobby.users_by_channel[&2].len(), 1);
  assert_eq!(lobby.users_by_channel[&2][0].user_id, 7);
  assert!(!lobby.users_by_channel[&2][0].speaking);
}

#[test]
fn channel_user_list_moves_user_between_cached_channels() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(2),
    users_by_channel: HashMap::from([
      (
        1,
        vec![LobbyUser {
          user_id: 7,
          username: "remote".to_owned(),
          role: Role::User,
          muted: false,
          deafened: false,
          speaking: true,
        }],
      ),
      (2, Vec::new()),
    ]),
    ..LobbyState::default()
  };

  apply_server_message(
    &mut lobby,
    S2C::ChannelUserList(crate::network::protocol::control::ChannelUserList {
      channel_id: 2,
      users: vec![crate::network::protocol::control::ChannelUser {
        user_id: 7,
        username: "remote".to_owned(),
        role: Role::User,
        muted: false,
        deafened: false,
      }],
    }),
    test_context(),
  );

  assert!(lobby.users_by_channel[&1].is_empty());
  assert_eq!(lobby.users_by_channel[&2].len(), 1);
  assert_eq!(lobby.users.len(), 1);
  assert_eq!(lobby.users[0].user_id, 7);
  assert!(!lobby.users[0].speaking);
}

#[test]
fn keepalive_pong_clears_overdue_warning_and_records_ping() {
  let mut lobby = LobbyState {
    connection_warning: Some(LobbyConnectionWarning {
      kind: LobbyConnectionWarningKind::KeepalivePongOverdue,
      message: "No pong for 8s, but traffic is still arriving.".to_owned(),
    }),
    ..LobbyState::default()
  };
  let mut context = test_context();
  context.pending_keepalive_ping = Some(Instant::now() - std::time::Duration::from_millis(25));

  apply_server_message(&mut lobby, S2C::KeepalivePong, context);

  assert!(lobby.keepalive_ok);
  assert_eq!(lobby.connection_warning, None);
  assert!(lobby.ping_ms.is_some_and(|ping_ms| ping_ms >= 20));
}

#[test]
fn keepalive_pong_preserves_non_keepalive_warning() {
  let mut lobby = LobbyState {
    connection_warning: Some(LobbyConnectionWarning {
      kind: LobbyConnectionWarningKind::VoiceReceiverStopped,
      message: "voice stopped".to_owned(),
    }),
    ..LobbyState::default()
  };

  apply_server_message(&mut lobby, S2C::KeepalivePong, test_context());

  assert!(lobby.keepalive_ok);
  assert_eq!(
    lobby.connection_warning.as_ref().map(|warning| &warning.kind),
    Some(&LobbyConnectionWarningKind::VoiceReceiverStopped)
  );
}

#[test]
fn kicked_server_error_marks_disconnect_without_auto_reconnect() {
  let mut lobby = LobbyState {
    selected_channel_id: Some(1),
    stream_browser_channel_id: Some(1),
    receiver_running: true,
    watching_user_id: Some(7),
    screen_shares: vec![LobbyScreenShare {
      sharer_user_id: 7,
      metadata: screen_share_metadata(1280, 720),
    }],
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
  assert_eq!(lobby.stream_browser_channel_id, None);
  assert!(lobby.screen_shares.is_empty());
  assert!(effects.stop_local_voice);
  assert_eq!(effects.notification_sound, Some(NotificationSound::UserKicked));
  assert_eq!(effects.watching_change, Some(Some(7)));
}
