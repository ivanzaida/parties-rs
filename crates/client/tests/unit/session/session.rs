use super::*;
use crate::network::protocol::{
  Role, S2C, ServerErrorCode, VideoCodecId,
  control::{
    ChannelInfo, ChannelList, ChannelUser, ChannelUserList, ScreenShareMetadata, ScreenShareStarted, UserJoinedChannel,
    UserRoleChanged,
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
