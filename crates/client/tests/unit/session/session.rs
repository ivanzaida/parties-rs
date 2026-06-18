use super::*;
use crate::network::protocol::{
  Role, S2C, ServerErrorCode, VideoCodecId,
  control::{
    ChannelInfo, ChannelList, ChannelUser, ChannelUserList, ChatHistoryResponse, ChatMessage, ScreenShareMetadata,
    ScreenShareStarted, TextChannelInfo, UserJoinedChannel, UserLeftChannel, UserRoleChanged, UserVoiceState,
  },
};

fn connected_info(role: Role) -> ConnectedServerInfo {
  ConnectedServerInfo {
    address: "example.test:7800".to_owned(),
    server_name: "Example".to_owned(),
    display_name: "local".to_owned(),
    user_id: 29,
    role,
    certificate_fingerprint: "aa:bb".to_owned(),
  }
}

fn connected_session() -> ServerSession {
  let session = ServerSession::default();
  let settings = AppSettings {
    notification_volume: 0,
    ..AppSettings::default()
  };
  session.set_notification_audio_settings(&settings);
  session
    .connection
    .set_connected_info_for_test(connected_info(Role::User));
  session
}

fn channel_info(id: ChannelId, name: &str) -> ChannelInfo {
  ChannelInfo {
    id,
    name: name.to_owned(),
    max_users: 0,
    sort_order: id,
    user_count: 0,
  }
}

fn channel_user(user_id: UserId, username: &str, role: Role) -> ChannelUser {
  ChannelUser {
    user_id,
    username: username.to_owned(),
    role,
    muted: false,
    deafened: false,
  }
}

fn text_channel_info(id: ChannelId, name: &str, sort_order: u32) -> TextChannelInfo {
  TextChannelInfo {
    id,
    name: name.to_owned(),
    sort_order,
  }
}

fn chat_message(id: u64, channel_id: ChannelId, sender_id: UserId, text: &str) -> ChatMessage {
  ChatMessage {
    id,
    channel_id,
    sender_id,
    sender_name: format!("user-{sender_id}"),
    timestamp: id,
    text: text.to_owned(),
    pinned: false,
    attachments: Vec::new(),
  }
}

fn screen_share_started(user_id: UserId) -> S2C {
  S2C::ScreenShareStarted(ScreenShareStarted {
    sharer_user_id: user_id,
    metadata: ScreenShareMetadata {
      codec: VideoCodecId::H264,
      width: 1920,
      height: 1080,
    },
  })
}

#[test]
fn test_lobby_update_snapshots_publish_monotonic_generations() {
  let session = connected_session();
  let mut updates = session.subscribe_lobby_updates();

  assert_eq!(updates.borrow().generation, 0);

  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));

  assert!(updates.has_changed().unwrap());
  let first = updates.borrow_and_update().clone();
  assert_eq!(first.generation, 1);
  assert_eq!(first.lobby.channels[0].id, 6);

  session.select_channel(6);

  assert!(updates.has_changed().unwrap());
  let second = updates.borrow_and_update().clone();
  assert!(second.generation > first.generation);
  assert_eq!(second.lobby.selected_channel_id, Some(6));
}

#[test]
fn test_connected_session_applies_channel_user_and_local_voice_state() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.set_local_voice_state(true, false);

  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::Moderator),
    ],
  }));

  let lobby = session.lobby();
  assert_eq!(lobby.selected_channel_id, Some(6));
  assert_eq!(lobby.users.len(), 2);
  let local = lobby.users.iter().find(|user| user.user_id == 29).unwrap();
  assert!(local.muted);
  assert!(!local.deafened);
  assert_eq!(lobby.channels[0].user_count, 2);
}

#[test]
fn test_connected_session_stream_stop_clears_watch_and_video_state() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::User),
    ],
  }));
  session.apply_server_message(screen_share_started(50));
  session.set_watching_user(Some(50));

  assert_eq!(session.lobby().watching_user_id, Some(50));

  session.apply_server_message(S2C::ScreenShareStopped { sharer_user_id: 50 });

  let lobby = session.lobby();
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.screen_shares.is_empty());
  assert_eq!(lobby.stream_browser_channel_id, Some(6));
}

#[test]
fn test_connected_session_role_change_updates_current_info() {
  let session = connected_session();

  session.apply_server_message(S2C::UserRoleChanged(UserRoleChanged {
    user_id: 29,
    role: Role::Admin,
  }));

  assert_eq!(session.info().map(|info| info.role), Some(Role::Admin));
}

#[test]
fn test_mark_lobby_error_clears_stream_state_and_preserves_reconnect_watch_target() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::User),
    ],
  }));
  session.apply_server_message(screen_share_started(50));
  session.set_watching_user(Some(50));

  session.mark_lobby_error("read: connection lost".to_owned());

  let lobby = session.lobby();
  assert!(lobby.disconnected);
  assert_eq!(lobby.last_error.as_deref(), Some("read: connection lost"));
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.screen_shares.is_empty());
  assert!(session.has_pending_reconnect_watch());
}

#[test]
fn test_non_reconnectable_server_error_disables_auto_reconnect_and_clears_streams() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::User),
    ],
  }));
  session.apply_server_message(screen_share_started(50));
  session.set_watching_user(Some(50));

  session.apply_server_message(S2C::ServerError {
    code: ServerErrorCode::Kicked,
    message: "kicked by admin".to_owned(),
  });

  let lobby = session.lobby();
  assert!(lobby.disconnected);
  assert!(lobby.auto_reconnect_disabled);
  assert_eq!(lobby.last_error.as_deref(), Some("kicked by admin"));
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.screen_shares.is_empty());
  assert!(!session.has_pending_reconnect_watch());
}

#[test]
fn test_remote_join_moves_user_between_channels_through_session_effects() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(1, "One"), channel_info(2, "Two")],
  }));
  session.select_channel(1);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 1,
    users: vec![channel_user(50, "remote", Role::User)],
  }));

  session.apply_server_message(S2C::UserJoinedChannel(UserJoinedChannel {
    user_id: 50,
    username: "remote".to_owned(),
    channel_id: 2,
    role: Role::Moderator,
  }));

  let lobby = session.lobby();
  assert!(lobby.users.is_empty());
  assert!(lobby.users_by_channel.get(&1).is_none_or(Vec::is_empty));
  assert_eq!(lobby.users_by_channel.get(&2).unwrap()[0].role, Role::Moderator);
}

#[test]
fn test_local_voice_state_message_updates_session_and_selected_user() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![channel_user(29, "local", Role::User)],
  }));

  session.apply_server_message(S2C::UserVoiceState(UserVoiceState {
    user_id: 29,
    muted: true,
    deafened: true,
  }));

  assert_eq!(session.local_voice_state(), Some((true, true)));
  let lobby = session.lobby();
  assert!(lobby.users[0].muted);
  assert!(lobby.users[0].deafened);
}

#[test]
fn test_generic_server_error_keeps_session_reconnectable() {
  let session = connected_session();

  session.apply_server_message(S2C::ServerError {
    code: ServerErrorCode::Generic,
    message: "temporary failure".to_owned(),
  });

  let lobby = session.lobby();
  assert!(!lobby.disconnected);
  assert!(!lobby.auto_reconnect_disabled);
  assert_eq!(lobby.last_error.as_deref(), Some("temporary failure"));
}

#[test]
fn test_watching_user_in_selected_channel_switches_main_view_to_stream_browser() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::User),
    ],
  }));
  {
    let mut lobby = session.lobby.lock();
    lobby.selected_text_channel_id = Some(2);
    lobby.debug_chat_selected = true;
  }

  session.set_watching_user(Some(50));

  let lobby = session.lobby();
  assert_eq!(lobby.watching_user_id, Some(50));
  assert_eq!(lobby.stream_browser_channel_id, Some(6));
  assert_eq!(lobby.selected_text_channel_id, None);
  assert!(!lobby.debug_chat_selected);
}

#[test]
fn test_watching_user_outside_selected_channel_preserves_current_text_view() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(1, "One"), channel_info(2, "Two")],
  }));
  session.select_channel(1);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 2,
    users: vec![channel_user(50, "remote", Role::User)],
  }));
  {
    let mut lobby = session.lobby.lock();
    lobby.selected_text_channel_id = Some(9);
    lobby.debug_chat_selected = false;
  }

  session.set_watching_user(Some(50));

  let lobby = session.lobby();
  assert_eq!(lobby.watching_user_id, Some(50));
  assert_eq!(lobby.stream_browser_channel_id, None);
  assert_eq!(lobby.selected_text_channel_id, Some(9));
}

#[test]
fn test_local_user_left_channel_message_clears_selected_voice_state() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::User),
    ],
  }));
  session.apply_server_message(screen_share_started(50));
  session.set_watching_user(Some(50));

  session.apply_server_message(S2C::UserLeftChannel(UserLeftChannel {
    user_id: 29,
    channel_id: 6,
  }));

  let lobby = session.lobby();
  assert_eq!(lobby.selected_channel_id, None);
  assert!(lobby.users.is_empty());
  assert_eq!(lobby.watching_user_id, None);
  assert!(lobby.screen_shares.iter().any(|share| share.sharer_user_id == 50));
}

#[test]
fn test_clear_resets_session_state_after_disconnect_flow() {
  let session = connected_session();
  session.set_tofu_warning(TofuWarning {
    address: "example.test:7800".to_owned(),
    server_name: "Example".to_owned(),
    user_id: 29,
    role: Role::User,
    saved_fingerprint: "old".to_owned(),
    received_fingerprint: "new".to_owned(),
    server_password: "secret".to_owned(),
    display_name: "local".to_owned(),
  });
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.apply_server_message(S2C::ChannelUserList(ChannelUserList {
    channel_id: 6,
    users: vec![
      channel_user(29, "local", Role::User),
      channel_user(50, "remote", Role::User),
    ],
  }));
  session.apply_server_message(screen_share_started(50));
  session.set_watching_user(Some(50));
  session.mark_lobby_error("transport closed".to_owned());
  assert!(session.has_pending_reconnect_watch());

  session.clear();

  assert!(session.info().is_none());
  assert!(session.tofu_warning().is_none());
  assert_eq!(session.lobby(), LobbyState::default());
  assert!(!session.has_pending_reconnect_watch());
  assert_eq!(session.local_voice_state(), None);
}

#[test]
fn test_remote_chat_message_marks_unread_until_text_channel_selected() {
  let session = connected_session();
  session.apply_server_message(S2C::ChatChannelList {
    channels: vec![
      text_channel_info(1, "general", 10),
      text_channel_info(2, "support", 20),
      text_channel_info(3, "random", 30),
    ],
  });

  session.apply_server_message(S2C::ChatMessage(chat_message(10, 2, 50, "hello")));
  session.apply_server_message(S2C::ChatMessage(chat_message(11, 1, 50, "selected")));
  session.apply_server_message(S2C::ChatMessage(chat_message(12, 3, 29, "local echo")));

  let lobby = session.lobby();
  assert_eq!(lobby.selected_text_channel_id, Some(1));
  assert!(lobby.unread_text_channel_ids.contains(&2));
  assert!(!lobby.unread_text_channel_ids.contains(&1));
  assert!(!lobby.unread_text_channel_ids.contains(&3));
  assert_eq!(lobby.chat_messages_by_channel.get(&2).unwrap()[0].text, "hello");

  session.select_text_channel(2);

  let lobby = session.lobby();
  assert_eq!(lobby.selected_text_channel_id, Some(2));
  assert!(!lobby.unread_text_channel_ids.contains(&2));
}

#[test]
fn test_text_and_debug_chat_selection_clear_stream_browser_without_leaving_voice() {
  let session = connected_session();
  session.apply_server_message(S2C::ChannelList(ChannelList {
    channels: vec![channel_info(6, "General")],
  }));
  session.select_channel(6);
  session.open_stream_browser(6);

  session.select_text_channel(2);

  let lobby = session.lobby();
  assert_eq!(lobby.selected_channel_id, Some(6));
  assert_eq!(lobby.selected_text_channel_id, Some(2));
  assert_eq!(lobby.stream_browser_channel_id, None);
  assert!(!lobby.debug_chat_selected);

  session.open_stream_browser(6);
  session.select_debug_chat();

  let lobby = session.lobby();
  assert_eq!(lobby.selected_channel_id, Some(6));
  assert_eq!(lobby.selected_text_channel_id, None);
  assert_eq!(lobby.stream_browser_channel_id, None);
  assert!(lobby.debug_chat_selected);
}

#[test]
fn test_debug_chat_messages_are_local_ordered_and_isolated_from_server_channels() {
  let session = connected_session();
  session.apply_server_message(S2C::ChatChannelList {
    channels: vec![text_channel_info(1, "general", 10)],
  });
  session.push_debug_chat_message("first");
  session.push_debug_chat_message("second");

  let lobby = session.lobby();
  assert_eq!(lobby.debug_chat_messages.len(), 2);
  assert_eq!(lobby.debug_chat_messages[0].id, 1);
  assert_eq!(lobby.debug_chat_messages[0].text, "first");
  assert_eq!(lobby.debug_chat_messages[1].id, 2);
  assert_eq!(lobby.debug_chat_messages[1].text, "second");
  assert!(lobby.chat_messages_by_channel.is_empty());
}

#[test]
fn test_chat_history_request_lifecycle_blocks_duplicate_and_finished_pages() {
  let session = connected_session();

  assert!(session.begin_chat_history_request(1, 0));
  assert!(!session.begin_chat_history_request(1, 0));
  assert!(session.lobby().chat_history_loading.contains(&1));

  session.finish_chat_history_request(1, false);

  let lobby = session.lobby();
  assert!(!lobby.chat_history_loading.contains(&1));
  assert_eq!(lobby.chat_history_has_more.get(&1), Some(&false));
  drop(lobby);

  assert!(!session.begin_chat_history_request(1, 0));
}

#[test]
fn test_chat_history_response_finishes_loading_and_merges_older_messages() {
  let session = connected_session();
  session.apply_server_message(S2C::ChatMessage(chat_message(20, 1, 50, "newer")));
  assert!(session.begin_chat_history_request(1, 20));

  session.apply_server_message(S2C::ChatHistoryResp(ChatHistoryResponse {
    channel_id: 1,
    has_more: false,
    messages: vec![chat_message(10, 1, 50, "older")],
  }));

  let lobby = session.lobby();
  assert!(!lobby.chat_history_loading.contains(&1));
  assert_eq!(lobby.chat_history_has_more.get(&1), Some(&false));
  assert_eq!(
    lobby
      .chat_messages_by_channel
      .get(&1)
      .unwrap()
      .iter()
      .map(|message| message.id)
      .collect::<Vec<_>>(),
    vec![10, 20]
  );
}

#[test]
fn test_chat_channel_list_prunes_removed_channel_state_through_session() {
  let session = connected_session();
  session.apply_server_message(S2C::ChatChannelList {
    channels: vec![text_channel_info(1, "general", 10), text_channel_info(2, "random", 20)],
  });
  session.apply_server_message(S2C::ChatMessage(chat_message(10, 2, 50, "unread")));
  assert!(session.begin_chat_history_request(2, 10));
  session.finish_chat_history_request(2, true);

  session.apply_server_message(S2C::ChatChannelList {
    channels: vec![text_channel_info(1, "general", 10)],
  });

  let lobby = session.lobby();
  assert_eq!(lobby.selected_text_channel_id, Some(1));
  assert!(!lobby.chat_messages_by_channel.contains_key(&2));
  assert!(!lobby.unread_text_channel_ids.contains(&2));
  assert!(!lobby.chat_history_loading.contains(&2));
  assert!(!lobby.chat_history_has_more.contains_key(&2));
}
