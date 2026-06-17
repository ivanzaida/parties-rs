use std::collections::HashMap;

use super::watched_stream_for_channel;
use crate::{
  network::protocol::{ChannelId, Role, UserId, VideoCodecId, control::ScreenShareMetadata},
  session::{LobbyChannel, LobbyScreenShare, LobbyState, LobbyUser},
};

fn channel(id: ChannelId) -> LobbyChannel {
  LobbyChannel {
    id,
    name: format!("Channel {id}"),
    max_users: 0,
    sort_order: id,
    user_count: 0,
  }
}

fn user(user_id: UserId) -> LobbyUser {
  LobbyUser {
    user_id,
    username: format!("user-{user_id}"),
    role: Role::User,
    muted: false,
    deafened: false,
    speaking: false,
  }
}

fn share(sharer_user_id: UserId) -> LobbyScreenShare {
  LobbyScreenShare {
    sharer_user_id,
    metadata: ScreenShareMetadata {
      codec: VideoCodecId::H264,
      width: 1280,
      height: 720,
    },
  }
}

#[test]
fn watched_stream_for_channel_returns_selected_stream_only_for_matching_channel() {
  let lobby = LobbyState {
    channels: vec![channel(10), channel(20)],
    users_by_channel: HashMap::from([(10, vec![user(2)]), (20, vec![user(3)])]),
    screen_shares: vec![share(2), share(3)],
    watching_user_id: Some(2),
    ..LobbyState::default()
  };

  let stream = watched_stream_for_channel(&lobby, 10).expect("watched stream in channel");

  assert_eq!(stream.share.sharer_user_id, 2);
  assert_eq!(stream.user.map(|user| user.user_id), Some(2));
  assert!(watched_stream_for_channel(&lobby, 20).is_none());
}

#[test]
fn watched_stream_for_channel_returns_none_without_selected_stream() {
  let lobby = LobbyState {
    users_by_channel: HashMap::from([(10, vec![user(2)])]),
    screen_shares: vec![share(2)],
    watching_user_id: None,
    ..LobbyState::default()
  };

  assert!(watched_stream_for_channel(&lobby, 10).is_none());
}
