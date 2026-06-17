use std::collections::HashMap;

use super::{initials_for_user, screen_shares_for_channel, stream_speaking, watched_stream};
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

fn user(user_id: UserId, username: &str) -> LobbyUser {
  LobbyUser {
    user_id,
    username: username.to_owned(),
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
      width: 1920,
      height: 1080,
    },
  }
}

#[test]
fn screen_shares_for_channel_returns_only_streams_for_users_in_that_channel() {
  let lobby = LobbyState {
    users_by_channel: HashMap::from([(10, vec![user(1, "one"), user(2, "two")])]),
    screen_shares: vec![share(2), share(3), share(1)],
    ..LobbyState::default()
  };

  let streams = screen_shares_for_channel(&lobby, 10);

  assert_eq!(
    streams
      .iter()
      .map(|stream| stream.share.sharer_user_id)
      .collect::<Vec<_>>(),
    vec![2, 1]
  );
  assert_eq!(
    streams
      .iter()
      .filter_map(|stream| stream.user.map(|user| user.username.as_str()))
      .collect::<Vec<_>>(),
    vec!["two", "one"]
  );
}

#[test]
fn screen_shares_for_channel_returns_empty_when_channel_has_no_users() {
  let lobby = LobbyState {
    screen_shares: vec![share(1)],
    ..LobbyState::default()
  };

  assert!(screen_shares_for_channel(&lobby, 10).is_empty());
}

#[test]
fn watched_stream_resolves_channel_user_and_share() {
  let lobby = LobbyState {
    channels: vec![channel(10), channel(20)],
    users_by_channel: HashMap::from([(10, vec![user(2, "two")]), (20, vec![user(3, "three")])]),
    screen_shares: vec![share(2), share(3)],
    watching_user_id: Some(2),
    ..LobbyState::default()
  };

  let watched = watched_stream(&lobby).expect("watched stream");

  assert_eq!(watched.channel.id, 10);
  assert_eq!(watched.stream.share.sharer_user_id, 2);
  assert_eq!(watched.stream.user.map(|user| user.username.as_str()), Some("two"));
}

#[test]
fn watched_stream_ignores_share_when_watched_user_is_not_in_a_channel() {
  let lobby = LobbyState {
    channels: vec![channel(10)],
    screen_shares: vec![share(2)],
    watching_user_id: Some(2),
    ..LobbyState::default()
  };

  assert!(watched_stream(&lobby).is_none());
}

#[test]
fn stream_speaking_requires_unmuted_undeafened_user() {
  let share = share(2);
  let mut active_user = user(2, "two");
  active_user.speaking = true;
  let active_stream = super::ChannelScreenShare {
    share: &share,
    user: Some(&active_user),
  };
  assert!(stream_speaking(&active_stream));

  let mut muted_user = active_user.clone();
  muted_user.muted = true;
  let muted_stream = super::ChannelScreenShare {
    share: &share,
    user: Some(&muted_user),
  };
  assert!(!stream_speaking(&muted_stream));

  let missing_user_stream = super::ChannelScreenShare {
    share: &share,
    user: None,
  };
  assert!(!stream_speaking(&missing_user_stream));
}

#[test]
fn initials_for_user_uses_first_alphanumeric_character() {
  assert_eq!(initials_for_user("lurq"), "L");
  assert_eq!(initials_for_user(" - 9lives"), "9");
  assert_eq!(initials_for_user("!!!"), "?");
}
