use std::collections::HashSet;

use crate::{
  network::protocol::{ChannelId, UserId},
  session::{LobbyChannel, LobbyScreenShare, LobbyState, LobbyTextChannel, LobbyUser},
};

pub(super) struct ChannelScreenShare<'a> {
  pub(super) share: &'a LobbyScreenShare,
  pub(super) user: Option<&'a LobbyUser>,
}

pub(super) struct WatchedChannelScreenShare<'a> {
  pub(super) channel: &'a LobbyChannel,
  pub(super) stream: ChannelScreenShare<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TextChannelRowModel {
  pub(super) channel: LobbyTextChannel,
  pub(super) selected: bool,
  pub(super) unread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VoiceUserRowModel {
  pub(super) user: LobbyUser,
  pub(super) local: bool,
  pub(super) streaming: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VoiceChannelRowModel {
  pub(super) channel: LobbyChannel,
  pub(super) users: Vec<VoiceUserRowModel>,
  pub(super) selected: bool,
}

pub(super) fn text_channel_rows(lobby: &LobbyState) -> Vec<TextChannelRowModel> {
  lobby
    .text_channels
    .iter()
    .map(|channel| TextChannelRowModel {
      channel: channel.clone(),
      selected: lobby.selected_text_channel_id == Some(channel.id),
      unread: lobby.unread_text_channel_ids.contains(&channel.id),
    })
    .collect()
}

pub(super) fn voice_channel_rows(lobby: &LobbyState, local_user_id: UserId) -> Vec<VoiceChannelRowModel> {
  let streaming_user_ids = lobby
    .screen_shares
    .iter()
    .map(|share| share.sharer_user_id)
    .collect::<HashSet<_>>();

  lobby
    .channels
    .iter()
    .map(|channel| VoiceChannelRowModel {
      channel: channel.clone(),
      users: lobby
        .users_by_channel
        .get(&channel.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|user| VoiceUserRowModel {
          local: user.user_id == local_user_id,
          streaming: streaming_user_ids.contains(&user.user_id),
          user,
        })
        .collect(),
      selected: lobby.selected_channel_id == Some(channel.id),
    })
    .collect()
}

pub(super) fn selected_text_channel(lobby: &LobbyState) -> Option<&LobbyTextChannel> {
  let channel_id = lobby.selected_text_channel_id?;
  lobby.text_channels.iter().find(|channel| channel.id == channel_id)
}

pub(super) fn stream_browser_channel(lobby: &LobbyState) -> Option<&LobbyChannel> {
  let channel_id = lobby.stream_browser_channel_id?;
  lobby.channels.iter().find(|channel| channel.id == channel_id)
}

pub(super) fn unique_lobby_member_count(lobby: &LobbyState) -> usize {
  let mut users = HashSet::new();

  for user in lobby.users_by_channel.values().flatten() {
    users.insert(user.user_id);
  }

  users.len()
}

pub(super) fn screen_shares_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Vec<ChannelScreenShare<'_>> {
  let Some(users) = lobby.users_by_channel.get(&channel_id) else {
    return Vec::new();
  };
  let user_ids = users.iter().map(|user| user.user_id).collect::<HashSet<_>>();

  lobby
    .screen_shares
    .iter()
    .filter(|share| user_ids.contains(&share.sharer_user_id))
    .map(|share| ChannelScreenShare {
      share,
      user: users.iter().find(|user| user.user_id == share.sharer_user_id),
    })
    .collect()
}

pub(super) fn watched_stream(lobby: &LobbyState) -> Option<WatchedChannelScreenShare<'_>> {
  let watched_user_id = lobby.watching_user_id?;

  for channel in &lobby.channels {
    let Some(users) = lobby.users_by_channel.get(&channel.id) else {
      continue;
    };
    let Some(user) = users.iter().find(|user| user.user_id == watched_user_id) else {
      continue;
    };
    let Some(share) = lobby
      .screen_shares
      .iter()
      .find(|share| share.sharer_user_id == watched_user_id)
    else {
      continue;
    };

    return Some(WatchedChannelScreenShare {
      channel,
      stream: ChannelScreenShare {
        share,
        user: Some(user),
      },
    });
  }

  None
}

pub(super) fn watched_stream_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Option<ChannelScreenShare<'_>> {
  let watching_user_id = lobby.watching_user_id?;
  screen_shares_for_channel(lobby, channel_id)
    .into_iter()
    .find(|stream| stream.share.sharer_user_id == watching_user_id)
}

pub(super) fn stream_speaking(stream: &ChannelScreenShare<'_>) -> bool {
  stream
    .user
    .is_some_and(|user| user.speaking && !user.muted && !user.deafened)
}

#[allow(dead_code)]
pub(super) fn user_voice_channel_id(lobby: &LobbyState, user_id: UserId) -> Option<ChannelId> {
  lobby
    .users_by_channel
    .iter()
    .find_map(|(channel_id, users)| users.iter().any(|user| user.user_id == user_id).then_some(*channel_id))
}
