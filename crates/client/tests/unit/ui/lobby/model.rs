use std::collections::HashMap;

use super::{
  chat_pane_model, floating_stream_preview_model, lobby_shell_model, main_body_model, main_top_bar_model,
  rail_bottom_model, rail_channels_model, rail_header_model, stream_browser_model, stream_watching_model,
};
use crate::{
  network::protocol::{
    ChannelId, Role, UserId, VideoCodecId,
    control::{ChatMessage, ScreenShareMetadata},
  },
  session::{
    ConnectedServerInfo, LobbyChannel, LobbyConnectionWarning, LobbyConnectionWarningKind, LobbyScreenShare,
    LobbyState, LobbyTextChannel, LobbyUser,
  },
};

fn info(user_id: UserId) -> ConnectedServerInfo {
  ConnectedServerInfo {
    address: "127.0.0.1:5000".to_owned(),
    server_name: "Parties".to_owned(),
    display_name: "Local Display".to_owned(),
    user_id,
    role: Role::Moderator,
    certificate_fingerprint: "fingerprint".to_owned(),
  }
}

fn voice_channel(id: ChannelId, name: &str) -> LobbyChannel {
  LobbyChannel {
    id,
    name: name.to_owned(),
    max_users: 0,
    sort_order: id,
    user_count: 1,
  }
}

fn text_channel(id: ChannelId, name: &str) -> LobbyTextChannel {
  LobbyTextChannel {
    id,
    name: name.to_owned(),
    sort_order: id,
  }
}

fn user(user_id: UserId, username: &str, muted: bool, deafened: bool) -> LobbyUser {
  LobbyUser {
    user_id,
    username: username.to_owned(),
    role: Role::User,
    muted,
    deafened,
    speaking: false,
  }
}

fn share(sharer_user_id: UserId) -> LobbyScreenShare {
  LobbyScreenShare {
    sharer_user_id,
    metadata: ScreenShareMetadata {
      codec: VideoCodecId::H264,
      width: 1920,
      height: 1080,
    },
  }
}

fn chat_message(id: u64, channel_id: ChannelId, sender_id: UserId) -> ChatMessage {
  ChatMessage {
    id,
    channel_id,
    sender_id,
    sender_name: format!("user-{sender_id}"),
    timestamp: id,
    text: format!("message {id}"),
    pinned: false,
    attachments: Vec::new(),
  }
}

#[test]
fn rail_section_models_collect_rail_only_state() {
  let local = user(7, "local", true, false);
  let remote = user(8, "remote", false, false);
  let lobby = LobbyState {
    channels: vec![voice_channel(10, "General"), voice_channel(20, "Away")],
    selected_channel_id: Some(10),
    text_channels: vec![text_channel(30, "chat"), text_channel(40, "logs")],
    selected_text_channel_id: Some(30),
    unread_text_channel_ids: [40].into_iter().collect(),
    users: vec![remote.clone()],
    users_by_channel: HashMap::from([(10, vec![local.clone()])]),
    screen_shares: vec![share(7)],
    debug_chat_selected: true,
    disconnected: false,
    ping_ms: Some(42),
    connection_warning: Some(LobbyConnectionWarning {
      kind: LobbyConnectionWarningKind::VoiceReceiverStopped,
      message: "voice stopped".to_owned(),
    }),
    ..LobbyState::default()
  };

  let info = info(7);
  let header = rail_header_model(&info, &lobby);
  let channels = rail_channels_model(&info, &lobby);
  let bottom = rail_bottom_model(&info, &lobby);

  assert_eq!(header.server_name, "Parties");
  assert_eq!(header.display_name, "Local Display");
  assert_eq!(header.user_id, 7);
  assert_eq!(header.role, Role::Moderator);
  assert_eq!(header.local_user_name.as_deref(), Some("local"));
  assert_eq!(channels.text_channels.len(), 2);
  assert!(channels.text_channels[0].selected);
  assert!(channels.text_channels[1].unread);
  assert_eq!(channels.voice_channels.len(), 2);
  assert!(channels.voice_channels[0].selected);
  assert!(channels.voice_channels[0].users[0].local);
  assert!(channels.voice_channels[0].users[0].streaming);
  assert!(channels.debug_chat_selected);
  assert_eq!(
    bottom.selected_voice_channel.as_ref().map(|channel| channel.id),
    Some(10)
  );
  assert_eq!(
    bottom
      .connection_warning
      .as_ref()
      .map(|warning| warning.message.as_str()),
    Some("voice stopped")
  );
  assert_eq!(bottom.ping_ms, Some(42));
  assert_eq!(bottom.local_user_name.as_deref(), Some("local"));
  assert_eq!(bottom.local_voice_state, (true, false));
  assert!(bottom.local_user_in_voice);
  assert!(bottom.local_streaming);
}

#[test]
fn stream_watching_model_collects_current_channel_streams_and_error() {
  let lobby = LobbyState {
    users_by_channel: HashMap::from([(
      10,
      vec![user(7, "local", false, false), user(8, "remote", false, false)],
    )]),
    screen_shares: vec![share(7), share(8), share(9)],
    watching_user_id: Some(8),
    last_error: Some("stream warning".to_owned()),
    ..LobbyState::default()
  };

  let model = stream_watching_model(&lobby, 10).expect("watching model");

  assert_eq!(model.stream.share.sharer_user_id, 8);
  assert_eq!(
    model.stream.user.as_ref().map(|user| user.username.as_str()),
    Some("remote")
  );
  assert_eq!(
    model
      .streams
      .iter()
      .map(|stream| stream.share.sharer_user_id)
      .collect::<Vec<_>>(),
    vec![7, 8]
  );
  assert_eq!(model.error.as_deref(), Some("stream warning"));
}

#[test]
fn floating_stream_preview_model_hides_when_main_pane_shows_watched_stream() {
  let lobby = LobbyState {
    channels: vec![voice_channel(10, "General")],
    stream_browser_channel_id: Some(10),
    users_by_channel: HashMap::from([(10, vec![user(7, "local", false, false)])]),
    screen_shares: vec![share(7)],
    watching_user_id: Some(7),
    ..LobbyState::default()
  };

  assert!(floating_stream_preview_model(&lobby).is_none());

  let lobby = LobbyState {
    selected_text_channel_id: Some(30),
    ..lobby
  };

  let preview = floating_stream_preview_model(&lobby).expect("floating preview");
  assert_eq!(preview.channel.id, 10);
  assert_eq!(preview.stream.share.sharer_user_id, 7);
}

#[test]
fn chat_pane_model_collects_messages_and_paging_state() {
  let lobby = LobbyState {
    chat_messages_by_channel: HashMap::from([(30, vec![chat_message(5, 30, 8), chat_message(6, 30, 7)])]),
    chat_history_has_more: HashMap::from([(30, true)]),
    last_error: Some("chat warning".to_owned()),
    ..LobbyState::default()
  };

  let model = chat_pane_model(&info(7), &lobby, 30, true);

  assert_eq!(model.local_user_id, 7);
  assert_eq!(model.messages.len(), 2);
  assert_eq!(model.messages[0].id, 5);
  assert!(!model.initial_history_loading);
  assert!(model.can_page);
  assert_eq!(model.error.as_deref(), Some("chat warning"));
}

#[test]
fn chat_pane_model_tracks_initial_history_loading_without_messages() {
  let lobby = LobbyState {
    chat_history_loading: [30].into_iter().collect(),
    chat_history_has_more: HashMap::from([(30, true)]),
    ..LobbyState::default()
  };

  let model = chat_pane_model(&info(7), &lobby, 30, true);

  assert!(model.messages.is_empty());
  assert!(model.initial_history_loading);
  assert!(!model.can_page);
}

#[test]
fn rail_model_ignores_chat_message_only_updates() {
  let mut lobby = LobbyState {
    channels: vec![voice_channel(10, "General")],
    selected_channel_id: Some(10),
    text_channels: vec![text_channel(30, "chat")],
    selected_text_channel_id: Some(30),
    users_by_channel: HashMap::from([(10, vec![user(7, "local", false, false)])]),
    ..LobbyState::default()
  };
  let info = info(7);
  let before = (
    rail_header_model(&info, &lobby),
    rail_channels_model(&info, &lobby),
    rail_bottom_model(&info, &lobby),
  );

  lobby.chat_messages_by_channel.insert(30, vec![chat_message(1, 30, 8)]);
  let after = (
    rail_header_model(&info, &lobby),
    rail_channels_model(&info, &lobby),
    rail_bottom_model(&info, &lobby),
  );

  assert_eq!(before, after);
}

#[test]
fn chat_pane_model_ignores_voice_presence_only_updates() {
  let mut lobby = LobbyState {
    chat_messages_by_channel: HashMap::from([(30, vec![chat_message(1, 30, 8)])]),
    users_by_channel: HashMap::from([(10, vec![user(8, "remote", false, false)])]),
    ..LobbyState::default()
  };
  let info = info(7);
  let before = chat_pane_model(&info, &lobby, 30, true);

  lobby
    .users_by_channel
    .get_mut(&10)
    .expect("voice users")
    .push(user(9, "another", true, false));
  let after = chat_pane_model(&info, &lobby, 30, true);

  assert_eq!(before, after);
}

#[test]
fn main_top_bar_model_ignores_chat_message_only_updates() {
  let mut lobby = LobbyState {
    text_channels: vec![text_channel(30, "chat")],
    selected_text_channel_id: Some(30),
    users_by_channel: HashMap::from([(10, vec![user(7, "local", false, false)])]),
    ..LobbyState::default()
  };
  let before = main_top_bar_model(&lobby, false);

  lobby.chat_messages_by_channel.insert(30, vec![chat_message(1, 30, 8)]);
  let after = main_top_bar_model(&lobby, false);

  assert_eq!(before, after);
}

#[test]
fn main_body_model_ignores_chat_message_only_updates() {
  let mut lobby = LobbyState {
    text_channels: vec![text_channel(30, "chat")],
    selected_text_channel_id: Some(30),
    ..LobbyState::default()
  };
  let before = main_body_model(&lobby, false);

  lobby.chat_messages_by_channel.insert(30, vec![chat_message(1, 30, 8)]);
  let after = main_body_model(&lobby, false);

  assert_eq!(before, after);
}

#[test]
fn stream_browser_model_ignores_chat_message_only_updates() {
  let channel = voice_channel(10, "General");
  let mut lobby = LobbyState {
    users_by_channel: HashMap::from([(
      10,
      vec![user(7, "local", false, false), user(8, "remote", false, false)],
    )]),
    screen_shares: vec![share(8)],
    watching_user_id: Some(8),
    ..LobbyState::default()
  };
  let before = stream_browser_model(&lobby, &channel);

  lobby.chat_messages_by_channel.insert(30, vec![chat_message(1, 30, 8)]);
  let after = stream_browser_model(&lobby, &channel);

  assert_eq!(before, after);
}

#[test]
fn stream_watching_model_ignores_chat_message_only_updates() {
  let mut lobby = LobbyState {
    users_by_channel: HashMap::from([(10, vec![user(8, "remote", false, false)])]),
    screen_shares: vec![share(8)],
    watching_user_id: Some(8),
    ..LobbyState::default()
  };
  let before = stream_watching_model(&lobby, 10);

  lobby.chat_messages_by_channel.insert(30, vec![chat_message(1, 30, 8)]);
  let after = stream_watching_model(&lobby, 10);

  assert_eq!(before, after);
}

#[test]
fn floating_stream_preview_model_ignores_chat_message_only_updates() {
  let mut lobby = LobbyState {
    channels: vec![voice_channel(10, "General")],
    users_by_channel: HashMap::from([(10, vec![user(8, "remote", false, false)])]),
    screen_shares: vec![share(8)],
    watching_user_id: Some(8),
    ..LobbyState::default()
  };
  let before = floating_stream_preview_model(&lobby);

  lobby.chat_messages_by_channel.insert(30, vec![chat_message(1, 30, 8)]);
  let after = floating_stream_preview_model(&lobby);

  assert_eq!(before, after);
}

#[test]
fn lobby_shell_model_ignores_voice_presence_only_updates() {
  let mut lobby = LobbyState {
    text_channels: vec![text_channel(30, "chat")],
    selected_text_channel_id: Some(30),
    chat_messages_by_channel: HashMap::from([(30, vec![chat_message(1, 30, 8)])]),
    users_by_channel: HashMap::from([(10, vec![user(8, "remote", false, false)])]),
    ..LobbyState::default()
  };
  let before = lobby_shell_model(&lobby);

  lobby
    .users_by_channel
    .get_mut(&10)
    .expect("voice users")
    .push(user(9, "another", false, false));
  let after = lobby_shell_model(&lobby);

  assert_eq!(before, after);
}
