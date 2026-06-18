use std::collections::HashMap;

use super::lobby_rail_model;
use crate::{
  network::protocol::{ChannelId, Role, UserId, VideoCodecId, control::ScreenShareMetadata},
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

#[test]
fn lobby_rail_model_collects_rail_only_state() {
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

  let model = lobby_rail_model(&info(7), &lobby);

  assert_eq!(model.server_name, "Parties");
  assert_eq!(model.display_name, "Local Display");
  assert_eq!(model.user_id, 7);
  assert_eq!(model.role, Role::Moderator);
  assert_eq!(model.text_channels.len(), 2);
  assert!(model.text_channels[0].selected);
  assert!(model.text_channels[1].unread);
  assert_eq!(model.voice_channels.len(), 2);
  assert!(model.voice_channels[0].selected);
  assert!(model.voice_channels[0].users[0].local);
  assert!(model.voice_channels[0].users[0].streaming);
  assert_eq!(
    model.selected_voice_channel.as_ref().map(|channel| channel.id),
    Some(10)
  );
  assert_eq!(
    model
      .connection_warning
      .as_ref()
      .map(|warning| warning.message.as_str()),
    Some("voice stopped")
  );
  assert_eq!(model.ping_ms, Some(42));
  assert_eq!(model.local_user_name.as_deref(), Some("local"));
  assert_eq!(model.local_voice_state, (true, false));
  assert!(model.local_user_in_voice);
  assert!(model.local_streaming);
  assert!(model.debug_chat_selected);
}
