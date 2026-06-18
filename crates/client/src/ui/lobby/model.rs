use std::collections::HashSet;

use lurq::app::component::DevtoolsInspectable;

use crate::{
  network::protocol::{ChannelId, Role, UserId, control::ChatMessage as ProtocolChatMessage},
  session::{
    ConnectedServerInfo, LobbyChannel, LobbyConnectionWarning, LobbyScreenShare, LobbyState, LobbyTextChannel,
    LobbyUser, chat_commands::ChatCommandRegistry,
  },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChannelScreenShare {
  pub(super) share: LobbyScreenShare,
  pub(super) user: Option<LobbyUser>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WatchedChannelScreenShare {
  pub(super) channel: LobbyChannel,
  pub(super) stream: ChannelScreenShare,
}

impl DevtoolsInspectable for WatchedChannelScreenShare {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StreamBrowserModel {
  pub(super) channel: LobbyChannel,
  pub(super) users: Vec<LobbyUser>,
  pub(super) streams: Vec<ChannelScreenShare>,
  pub(super) watching_user_id: Option<UserId>,
  pub(super) error: Option<String>,
}

impl DevtoolsInspectable for StreamBrowserModel {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StreamWatchingModel {
  pub(super) stream: ChannelScreenShare,
  pub(super) streams: Vec<ChannelScreenShare>,
  pub(super) error: Option<String>,
}

impl DevtoolsInspectable for StreamWatchingModel {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChatPaneModel {
  pub(super) local_user_id: UserId,
  pub(super) messages: Vec<ProtocolChatMessage>,
  pub(super) initial_history_loading: bool,
  pub(super) can_page: bool,
  pub(super) error: Option<String>,
}

impl DevtoolsInspectable for ChatPaneModel {}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LobbyRailModel {
  pub(super) server_name: String,
  pub(super) display_name: String,
  pub(super) user_id: UserId,
  pub(super) role: Role,
  pub(super) text_channels: Vec<TextChannelRowModel>,
  pub(super) voice_channels: Vec<VoiceChannelRowModel>,
  pub(super) debug_chat_selected: bool,
  pub(super) disconnected: bool,
  pub(super) selected_voice_channel: Option<LobbyChannel>,
  pub(super) connection_warning: Option<LobbyConnectionWarning>,
  pub(super) ping_ms: Option<u32>,
  pub(super) local_user_name: Option<String>,
  pub(super) local_voice_state: (bool, bool),
  pub(super) local_user_in_voice: bool,
  pub(super) local_streaming: bool,
}

impl DevtoolsInspectable for LobbyRailModel {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MainTopBarModel {
  DebugChat,
  Text {
    channel: LobbyTextChannel,
    command_registry: ChatCommandRegistry,
    member_count: usize,
  },
  StreamWatching {
    stream: ChannelScreenShare,
  },
  StreamBrowser {
    channel: LobbyChannel,
    user_count: usize,
  },
  VoiceDefault,
}

impl DevtoolsInspectable for MainTopBarModel {}

pub(super) fn lobby_rail_model(info: &ConnectedServerInfo, lobby: &LobbyState) -> LobbyRailModel {
  LobbyRailModel {
    server_name: info.server_name.clone(),
    display_name: info.display_name.clone(),
    user_id: info.user_id,
    role: info.role,
    text_channels: text_channel_rows(lobby),
    voice_channels: voice_channel_rows(lobby, info.user_id),
    debug_chat_selected: lobby.debug_chat_selected,
    disconnected: lobby.disconnected,
    selected_voice_channel: selected_voice_channel(lobby).cloned(),
    connection_warning: lobby.connection_warning.clone(),
    ping_ms: lobby.ping_ms,
    local_user_name: local_user_name(lobby, info.user_id),
    local_voice_state: local_voice_state(lobby, info.user_id),
    local_user_in_voice: local_user_in_voice(lobby, info.user_id),
    local_streaming: lobby
      .screen_shares
      .iter()
      .any(|share| share.sharer_user_id == info.user_id),
  }
}

pub(super) fn main_top_bar_model(lobby: &LobbyState, debug_mode_enabled: bool) -> MainTopBarModel {
  if debug_mode_enabled && lobby.debug_chat_selected {
    return MainTopBarModel::DebugChat;
  }

  if let Some(channel) = selected_text_channel(lobby) {
    return MainTopBarModel::Text {
      channel: channel.clone(),
      command_registry: lobby.chat_command_registry.clone(),
      member_count: unique_lobby_member_count(lobby),
    };
  }

  if let Some(channel) = stream_browser_channel(lobby) {
    if let Some(model) = stream_watching_model(lobby, channel.id) {
      return MainTopBarModel::StreamWatching { stream: model.stream };
    }

    return MainTopBarModel::StreamBrowser {
      channel: channel.clone(),
      user_count: lobby.users_by_channel.get(&channel.id).map(Vec::len).unwrap_or(0),
    };
  }

  MainTopBarModel::VoiceDefault
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

pub(super) fn stream_browser_model(lobby: &LobbyState, channel: &LobbyChannel) -> StreamBrowserModel {
  StreamBrowserModel {
    channel: channel.clone(),
    users: lobby.users_by_channel.get(&channel.id).cloned().unwrap_or_default(),
    streams: screen_shares_for_channel(lobby, channel.id),
    watching_user_id: lobby.watching_user_id,
    error: lobby.last_error.clone(),
  }
}

pub(super) fn stream_watching_model(lobby: &LobbyState, channel_id: ChannelId) -> Option<StreamWatchingModel> {
  Some(StreamWatchingModel {
    stream: watched_stream_for_channel(lobby, channel_id)?,
    streams: screen_shares_for_channel(lobby, channel_id),
    error: lobby.last_error.clone(),
  })
}

pub(super) fn floating_stream_preview_model(lobby: &LobbyState) -> Option<WatchedChannelScreenShare> {
  let watched = watched_stream(lobby)?;
  (!main_pane_shows_watched_stream(lobby, watched.channel.id)).then_some(watched)
}

pub(super) fn chat_pane_model(
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  channel_id: ChannelId,
  server_backed: bool,
) -> ChatPaneModel {
  let messages = if server_backed {
    lobby
      .chat_messages_by_channel
      .get(&channel_id)
      .map(Vec::as_slice)
      .unwrap_or(&[])
  } else {
    lobby.debug_chat_messages.as_slice()
  };
  let oldest_message_id = messages.first().map(|message| message.id).unwrap_or(0);

  ChatPaneModel {
    local_user_id: info.user_id,
    messages: messages.to_vec(),
    initial_history_loading: messages.is_empty()
      && lobby.chat_history_loading.contains(&channel_id)
      && lobby.chat_history_has_more.get(&channel_id).copied().unwrap_or(true),
    can_page: server_backed
      && oldest_message_id != 0
      && lobby.chat_history_has_more.get(&channel_id).copied().unwrap_or(true)
      && !lobby.chat_history_loading.contains(&channel_id),
    error: lobby.last_error.clone(),
  }
}

fn selected_voice_channel(lobby: &LobbyState) -> Option<&LobbyChannel> {
  let channel_id = lobby.selected_channel_id?;
  lobby.channels.iter().find(|channel| channel.id == channel_id)
}

fn main_pane_shows_watched_stream(lobby: &LobbyState, watched_channel_id: ChannelId) -> bool {
  if lobby.debug_chat_selected || lobby.selected_text_channel_id.is_some() {
    return false;
  }

  lobby.stream_browser_channel_id == Some(watched_channel_id)
}

pub(super) fn unique_lobby_member_count(lobby: &LobbyState) -> usize {
  let mut users = HashSet::new();

  for user in lobby.users_by_channel.values().flatten() {
    users.insert(user.user_id);
  }

  users.len()
}

pub(super) fn screen_shares_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Vec<ChannelScreenShare> {
  let Some(users) = lobby.users_by_channel.get(&channel_id) else {
    return Vec::new();
  };
  let user_ids = users.iter().map(|user| user.user_id).collect::<HashSet<_>>();

  lobby
    .screen_shares
    .iter()
    .filter(|share| user_ids.contains(&share.sharer_user_id))
    .map(|share| ChannelScreenShare {
      share: share.clone(),
      user: users.iter().find(|user| user.user_id == share.sharer_user_id).cloned(),
    })
    .collect()
}

pub(super) fn watched_stream(lobby: &LobbyState) -> Option<WatchedChannelScreenShare> {
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
      channel: channel.clone(),
      stream: ChannelScreenShare {
        share: share.clone(),
        user: Some(user.clone()),
      },
    });
  }

  None
}

pub(super) fn watched_stream_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Option<ChannelScreenShare> {
  let watching_user_id = lobby.watching_user_id?;
  screen_shares_for_channel(lobby, channel_id)
    .into_iter()
    .find(|stream| stream.share.sharer_user_id == watching_user_id)
}

pub(super) fn stream_speaking(stream: &ChannelScreenShare) -> bool {
  stream
    .user
    .as_ref()
    .is_some_and(|user| user.speaking && !user.muted && !user.deafened)
}

#[allow(dead_code)]
pub(super) fn user_voice_channel_id(lobby: &LobbyState, user_id: UserId) -> Option<ChannelId> {
  lobby
    .users_by_channel
    .iter()
    .find_map(|(channel_id, users)| users.iter().any(|user| user.user_id == user_id).then_some(*channel_id))
}

fn local_user_in_voice(lobby: &LobbyState, local_user_id: UserId) -> bool {
  lobby
    .users_by_channel
    .values()
    .any(|users| users.iter().any(|user| user.user_id == local_user_id))
}

fn local_user_name(lobby: &LobbyState, local_user_id: UserId) -> Option<String> {
  lobby
    .users
    .iter()
    .chain(lobby.users_by_channel.values().flatten())
    .find(|user| user.user_id == local_user_id)
    .map(|user| user.username.clone())
}

fn local_voice_state(lobby: &LobbyState, local_user_id: UserId) -> (bool, bool) {
  lobby
    .users
    .iter()
    .chain(lobby.users_by_channel.values().flatten())
    .find(|user| user.user_id == local_user_id)
    .map(|user| (user.muted, user.deafened))
    .unwrap_or((false, false))
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/lobby/model.rs"]
mod tests;
