use std::{
  collections::{HashMap, HashSet},
  time::{Instant, SystemTime, UNIX_EPOCH},
};

use super::{
  chat_commands::{ChatCommandRegistry, CommandDefinition},
  chat_history::{ChatHistoryMessage, merge_chat_history_messages, merge_chat_messages},
};
use crate::{
  network::protocol::{
    ChannelId, Role, S2C, ServerErrorCode, UserId,
    control::{
      ChannelInfo, ChannelUser as ProtocolChannelUser, ChatMessage as ProtocolChatMessage, ScreenShareMetadata,
      TextChannelInfo,
    },
  },
  services::notifications::NotificationSound,
};

pub const DEBUG_CHAT_CHANNEL_ID: ChannelId = u32::MAX;
const DEBUG_CHAT_SENDER_ID: UserId = 0;
const DEBUG_CHAT_SENDER_NAME: &str = "Debug";

impl ChatHistoryMessage for ProtocolChatMessage {
  fn chat_id(&self) -> u64 {
    self.id
  }

  fn chat_timestamp(&self) -> u64 {
    self.timestamp
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyChannel {
  pub id: ChannelId,
  pub name: String,
  pub max_users: u32,
  pub sort_order: u32,
  pub user_count: u32,
}

impl From<ChannelInfo> for LobbyChannel {
  fn from(channel: ChannelInfo) -> Self {
    Self {
      id: channel.id,
      name: channel.name,
      max_users: channel.max_users,
      sort_order: channel.sort_order,
      user_count: channel.user_count,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyTextChannel {
  pub id: ChannelId,
  pub name: String,
  pub sort_order: u32,
}

impl From<TextChannelInfo> for LobbyTextChannel {
  fn from(channel: TextChannelInfo) -> Self {
    Self {
      id: channel.id,
      name: channel.name,
      sort_order: channel.sort_order,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyUser {
  pub user_id: UserId,
  pub username: String,
  pub role: Role,
  pub muted: bool,
  pub deafened: bool,
  pub speaking: bool,
}

impl From<ProtocolChannelUser> for LobbyUser {
  fn from(user: ProtocolChannelUser) -> Self {
    Self {
      user_id: user.user_id,
      username: user.username,
      role: user.role,
      muted: user.muted,
      deafened: user.deafened,
      speaking: false,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyScreenShare {
  pub sharer_user_id: UserId,
  pub metadata: ScreenShareMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyConnectionWarningKind {
  KeepalivePongOverdue,
  VoiceReceiverStopped,
  VideoReceiverStopped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyConnectionWarning {
  pub kind: LobbyConnectionWarningKind,
  pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LobbyState {
  pub channels: Vec<LobbyChannel>,
  pub selected_channel_id: Option<ChannelId>,
  pub stream_browser_channel_id: Option<ChannelId>,
  pub text_channels: Vec<LobbyTextChannel>,
  pub selected_text_channel_id: Option<ChannelId>,
  pub debug_chat_selected: bool,
  pub chat_messages_by_channel: HashMap<ChannelId, Vec<ProtocolChatMessage>>,
  pub debug_chat_messages: Vec<ProtocolChatMessage>,
  pub next_debug_chat_message_id: u64,
  pub unread_text_channel_ids: HashSet<ChannelId>,
  pub chat_history_loading: HashSet<ChannelId>,
  pub chat_history_has_more: HashMap<ChannelId, bool>,
  pub chat_command_registry: ChatCommandRegistry,
  pub users: Vec<LobbyUser>,
  pub users_by_channel: HashMap<ChannelId, Vec<LobbyUser>>,
  pub screen_shares: Vec<LobbyScreenShare>,
  pub watching_user_id: Option<UserId>,
  pub receiver_running: bool,
  pub channel_list_received: bool,
  pub keepalive_ok: bool,
  pub ping_ms: Option<u32>,
  pub disconnected: bool,
  pub last_error: Option<String>,
  pub connection_warning: Option<LobbyConnectionWarning>,
  pub auto_reconnect_disabled: bool,
}

#[derive(Default)]
pub(super) struct LeaveChannelEffects {
  pub(super) left_voice: bool,
  pub(super) watching_change: Option<Option<UserId>>,
  pub(super) clear_video_cache_user: Option<UserId>,
  pub(super) forget_speaking_user: Option<UserId>,
}

pub(super) fn select_channel(lobby: &mut LobbyState, channel_id: ChannelId) {
  let previous = lobby.selected_channel_id;
  lobby.selected_channel_id = Some(channel_id);
  lobby.stream_browser_channel_id = None;
  sync_selected_users(lobby);
  tracing::debug!(target: "lobby",
    "[lobby] selected voice channel: previous={previous:?} current={channel_id} users={}",
    lobby.users.len()
  );
}

pub(super) fn leave_channel_locally(lobby: &mut LobbyState, local_user_id: Option<UserId>) -> LeaveChannelEffects {
  let mut effects = LeaveChannelEffects::default();
  if let Some(channel_id) = lobby.selected_channel_id.take() {
    effects.left_voice = true;
    tracing::info!(target: "lobby", "[lobby] leaving voice channel locally: channel={channel_id} local_user={local_user_id:?}");
    if let Some(user_id) = local_user_id
      && let Some(users) = lobby.users_by_channel.get_mut(&channel_id)
    {
      users.retain(|user| user.user_id != user_id);
    }
  }
  if let Some(user_id) = local_user_id {
    lobby.screen_shares.retain(|share| share.sharer_user_id != user_id);
    effects.clear_video_cache_user = Some(user_id);
    effects.forget_speaking_user = Some(user_id);
  }
  let (previous_user_id, changed) = set_watching_user(lobby, None);
  if changed {
    effects.watching_change = Some(previous_user_id);
  }
  lobby.stream_browser_channel_id = None;
  lobby.users.clear();
  sync_cached_channel_counts(lobby);
  effects
}

pub(super) fn select_text_channel(lobby: &mut LobbyState, channel_id: ChannelId) {
  let previous = lobby.selected_text_channel_id;
  lobby.selected_text_channel_id = Some(channel_id);
  lobby.debug_chat_selected = false;
  lobby.unread_text_channel_ids.remove(&channel_id);
  lobby.stream_browser_channel_id = None;
  tracing::debug!(target: "lobby", "[lobby] selected text channel: previous={previous:?} current={channel_id}");
}

pub(super) fn select_debug_chat(lobby: &mut LobbyState) {
  lobby.selected_text_channel_id = None;
  lobby.debug_chat_selected = true;
  lobby.stream_browser_channel_id = None;
  tracing::debug!(target: "lobby", "[lobby] selected debug chat");
}

pub(super) fn push_debug_chat_message(lobby: &mut LobbyState, text: String) {
  let id = lobby.next_debug_chat_message_id.max(1);
  lobby.next_debug_chat_message_id = id.saturating_add(1);
  lobby.debug_chat_messages.push(ProtocolChatMessage {
    id,
    channel_id: DEBUG_CHAT_CHANNEL_ID,
    sender_id: DEBUG_CHAT_SENDER_ID,
    sender_name: DEBUG_CHAT_SENDER_NAME.to_owned(),
    timestamp: current_timestamp_millis(),
    text,
    pinned: false,
    attachments: Vec::new(),
  });
}

fn current_timestamp_millis() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
    .unwrap_or(0)
}

pub(super) fn open_stream_browser(lobby: &mut LobbyState, channel_id: ChannelId) {
  if lobby.selected_channel_id == Some(channel_id) && lobby.channels.iter().any(|channel| channel.id == channel_id) {
    lobby.selected_text_channel_id = None;
    lobby.debug_chat_selected = false;
    lobby.stream_browser_channel_id = Some(channel_id);
    tracing::info!(target: "video", "[video] stream browser opened: channel={channel_id}");
  }
}

pub(super) fn close_stream_browser(lobby: &mut LobbyState) {
  if lobby.stream_browser_channel_id.is_some() {
    tracing::info!(target: "video",
      "[video] stream browser closed: previous={:?}",
      lobby.stream_browser_channel_id
    );
  }
  lobby.stream_browser_channel_id = None;
}

pub(super) fn begin_chat_history_request(lobby: &mut LobbyState, channel_id: ChannelId, _before_id: u64) -> bool {
  if lobby.chat_history_has_more.get(&channel_id) == Some(&false) || lobby.chat_history_loading.contains(&channel_id) {
    return false;
  }
  lobby.chat_history_loading.insert(channel_id);
  true
}

pub(super) fn finish_chat_history_request(lobby: &mut LobbyState, channel_id: ChannelId, has_more: bool) {
  lobby.chat_history_loading.remove(&channel_id);
  lobby.chat_history_has_more.insert(channel_id, has_more);
}

pub(super) fn sync_selected_users(lobby: &mut LobbyState) {
  lobby.users = lobby
    .selected_channel_id
    .and_then(|channel_id| lobby.users_by_channel.get(&channel_id).cloned())
    .unwrap_or_default();
}

pub(super) fn sync_cached_channel_counts(lobby: &mut LobbyState) {
  for channel in &mut lobby.channels {
    if let Some(users) = lobby.users_by_channel.get(&channel.id) {
      channel.user_count = users.len() as u32;
    }
  }
}

pub(super) fn set_watching_user(lobby: &mut LobbyState, user_id: Option<UserId>) -> (Option<UserId>, bool) {
  let previous_user_id = lobby.watching_user_id;
  lobby.watching_user_id = user_id;
  if let Some(user_id) = user_id
    && let Some(channel_id) = lobby.selected_channel_id
    && lobby
      .users_by_channel
      .get(&channel_id)
      .is_some_and(|users| users.iter().any(|user| user.user_id == user_id))
  {
    lobby.selected_text_channel_id = None;
    lobby.debug_chat_selected = false;
    lobby.stream_browser_channel_id = Some(channel_id);
  }
  (previous_user_id, previous_user_id != user_id)
}

pub(super) fn apply_local_voice_state(users: &mut [LobbyUser], local_user_id: Option<UserId>, state: (bool, bool)) {
  let Some(local_user_id) = local_user_id else {
    return;
  };

  if let Some(user) = users.iter_mut().find(|user| user.user_id == local_user_id) {
    user.muted = state.0;
    user.deafened = state.1;
  }
}

pub(super) fn user_in_selected_voice_channel(lobby: &LobbyState, user_id: UserId) -> bool {
  lobby
    .selected_channel_id
    .and_then(|channel_id| lobby.users_by_channel.get(&channel_id))
    .is_some_and(|users| users.iter().any(|user| user.user_id == user_id))
}

#[derive(Clone, Debug)]
pub(super) struct ServerMessageContext {
  pub(super) local_user_id: Option<UserId>,
  pub(super) local_display_name: String,
  pub(super) local_voice_state: (bool, bool),
  pub(super) pending_keepalive_ping: Option<Instant>,
}

#[derive(Default)]
pub(super) struct ServerMessageEffects {
  pub(super) local_voice_update: Option<(bool, bool)>,
  pub(super) stop_local_voice: bool,
  pub(super) clear_speaking_user: Option<UserId>,
  pub(super) notification_sound: Option<NotificationSound>,
  pub(super) watching_change: Option<Option<UserId>>,
  pub(super) clear_video_cache_users: Vec<UserId>,
  pub(super) current_role_update: Option<Role>,
}

pub(super) fn message_mentions_display_name(text: &str, display_name: &str) -> bool {
  let display_name = display_name.trim();
  if display_name.is_empty() {
    return false;
  }

  let text = text.to_ascii_lowercase();
  let display_name = display_name.to_ascii_lowercase();
  if text.contains(&format!("@{display_name}")) {
    return true;
  }
  if display_name.contains(char::is_whitespace) {
    return text.contains(&display_name);
  }

  text
    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
    .any(|token| token == display_name)
}

pub(super) fn apply_server_message(
  lobby: &mut LobbyState,
  message: S2C,
  context: ServerMessageContext,
) -> ServerMessageEffects {
  let local_user_id = context.local_user_id;
  let local_display_name = context.local_display_name;
  let local_voice_state = context.local_voice_state;
  let mut effects = ServerMessageEffects::default();

  match message {
    S2C::ChannelList(list) => {
      tracing::debug!(target: "lobby",
        "[lobby] received voice channel list: channels={} selected={:?}",
        list.channels.len(),
        lobby.selected_channel_id
      );
      let selected = lobby.selected_channel_id;
      lobby.channels = list.channels.into_iter().map(LobbyChannel::from).collect();
      lobby.channels.sort_by_key(|channel| channel.sort_order);
      let channel_ids: Vec<_> = lobby.channels.iter().map(|channel| channel.id).collect();
      lobby
        .users_by_channel
        .retain(|channel_id, _| channel_ids.contains(channel_id));
      lobby.channel_list_received = true;

      if selected.is_some_and(|id| lobby.channels.iter().any(|channel| channel.id == id)) {
        lobby.selected_channel_id = selected;
        sync_selected_users(lobby);
      } else {
        lobby.selected_channel_id = None;
        lobby.stream_browser_channel_id = None;
        lobby.users.clear();
      }
    }
    S2C::ChatChannelList { channels } => {
      tracing::debug!(target: "lobby",
        "[lobby] received text channel list: channels={} selected={:?}",
        channels.len(),
        lobby.selected_text_channel_id
      );
      let selected = lobby.selected_text_channel_id;
      lobby.text_channels = channels.into_iter().map(LobbyTextChannel::from).collect();
      lobby.text_channels.sort_by_key(|channel| channel.sort_order);
      let channel_ids: Vec<_> = lobby.text_channels.iter().map(|channel| channel.id).collect();
      lobby
        .chat_messages_by_channel
        .retain(|channel_id, _| channel_ids.contains(channel_id));
      lobby
        .unread_text_channel_ids
        .retain(|channel_id| channel_ids.contains(channel_id));
      lobby
        .chat_history_loading
        .retain(|channel_id| channel_ids.contains(channel_id));
      lobby
        .chat_history_has_more
        .retain(|channel_id, _| channel_ids.contains(channel_id));

      if lobby.debug_chat_selected {
        lobby.selected_text_channel_id = None;
      } else if selected.is_some_and(|id| lobby.text_channels.iter().any(|channel| channel.id == id)) {
        lobby.selected_text_channel_id = selected;
      } else {
        lobby.selected_text_channel_id = lobby.text_channels.first().map(|channel| channel.id);
      }
    }
    S2C::ChatCommandList(list) => {
      tracing::debug!(target: "lobby", "[lobby] received chat command list: commands={}", list.commands.len());
      lobby.chat_command_registry = ChatCommandRegistry::from_definitions(
        list
          .commands
          .into_iter()
          .map(|command| CommandDefinition::server_advertised(command.name, command.description, command.usage)),
      );
    }
    S2C::ChatMessage(message) => {
      let should_notify = local_user_id != Some(message.sender_id);
      let message_mentions_local_user =
        should_notify && message_mentions_display_name(&message.text, &local_display_name);
      tracing::debug!(target: "chat",
        "[chat] received message: id={} channel={} sender={} local={} notify={}",
        message.id,
        message.channel_id,
        message.sender_id,
        !should_notify,
        should_notify
      );
      if should_notify && lobby.selected_text_channel_id != Some(message.channel_id) {
        lobby.unread_text_channel_ids.insert(message.channel_id);
      }
      merge_chat_messages(
        lobby.chat_messages_by_channel.entry(message.channel_id).or_default(),
        [message],
      );
      if should_notify {
        effects.notification_sound = Some(if message_mentions_local_user {
          NotificationSound::Mention
        } else {
          NotificationSound::ChatMessage
        });
      }
    }
    S2C::ChatHistoryResp(response) => {
      let message_count = response.messages.len();
      tracing::info!(target: "chat::history",
        "[chat/history] response received: channel={} messages={} has_more={}",
        response.channel_id,
        message_count,
        response.has_more
      );
      lobby.chat_history_loading.remove(&response.channel_id);
      lobby
        .chat_history_has_more
        .insert(response.channel_id, response.has_more);
      merge_chat_history_messages(
        lobby.chat_messages_by_channel.entry(response.channel_id).or_default(),
        response.messages,
      );
    }
    S2C::ChatMessageDeleted { message_id, channel_id } => {
      tracing::info!(target: "chat", "[chat] message deleted: id={message_id} channel={channel_id}");
      if let Some(messages) = lobby.chat_messages_by_channel.get_mut(&channel_id) {
        messages.retain(|message| message.id != message_id);
      }
    }
    S2C::ChannelUserList(list) => {
      tracing::debug!(target: "lobby",
        "[lobby] received channel user list: channel={} users={} selected={:?}",
        list.channel_id,
        list.users.len(),
        lobby.selected_channel_id
      );
      let mut users = list.users.into_iter().map(LobbyUser::from).collect::<Vec<_>>();
      apply_local_voice_state(&mut users, local_user_id, local_voice_state);
      for user in &users {
        for (channel_id, cached_users) in &mut lobby.users_by_channel {
          if *channel_id != list.channel_id {
            cached_users.retain(|cached| cached.user_id != user.user_id);
          }
        }
      }
      lobby.users_by_channel.insert(list.channel_id, users);
      sync_selected_users(lobby);
      sync_cached_channel_counts(lobby);
    }
    S2C::UserJoinedChannel(joined) => {
      let joined_user_id = joined.user_id;
      let joined_username = joined.username.clone();
      let joined_channel_id = joined.channel_id;
      let joined_role = joined.role;
      let selected_channel_id = lobby.selected_channel_id;
      let was_in_selected_channel = selected_channel_id
        .and_then(|channel_id| lobby.users_by_channel.get(&channel_id))
        .is_some_and(|users| users.iter().any(|user| user.user_id == joined_user_id));
      for (channel_id, users) in &mut lobby.users_by_channel {
        if *channel_id != joined_channel_id {
          users.retain(|user| user.user_id != joined_user_id);
        }
      }
      let users = lobby.users_by_channel.entry(joined_channel_id).or_default();
      let inserted = if users.iter().any(|user| user.user_id == joined_user_id) {
        false
      } else {
        let local = local_user_id == Some(joined_user_id);
        users.push(LobbyUser {
          user_id: joined_user_id,
          username: joined.username,
          role: joined.role,
          muted: local && local_voice_state.0,
          deafened: local && local_voice_state.1,
          speaking: false,
        });
        true
      };
      if lobby.selected_channel_id == Some(joined_channel_id) {
        sync_selected_users(lobby);
      }
      tracing::debug!(target: "lobby",
        "[lobby] user joined voice channel: user={} name='{}' channel={} role={:?} local={} inserted={} selected={:?}",
        joined_user_id,
        joined_username,
        joined_channel_id,
        joined_role,
        local_user_id == Some(joined_user_id),
        inserted,
        selected_channel_id
      );
      if inserted {
        sync_cached_channel_counts(lobby);
        if local_user_id != Some(joined_user_id) {
          if selected_channel_id == Some(joined_channel_id) {
            effects.notification_sound = Some(NotificationSound::VoiceJoin);
          } else if was_in_selected_channel {
            effects.notification_sound = Some(NotificationSound::VoiceLeave);
          }
        }
      }
    }
    S2C::UserLeftChannel(left) => {
      let local_left = local_user_id == Some(left.user_id);
      let was_in_selected_channel = lobby
        .selected_channel_id
        .and_then(|channel_id| lobby.users_by_channel.get(&channel_id))
        .is_some_and(|users| users.iter().any(|user| user.user_id == left.user_id));
      for users in lobby.users_by_channel.values_mut() {
        users.retain(|user| user.user_id != left.user_id);
      }
      tracing::debug!(target: "lobby",
        "[lobby] user left voice channel: user={} channel={} local={} was_selected_channel={}",
        left.user_id,
        left.channel_id,
        local_left,
        was_in_selected_channel
      );
      if local_left {
        effects.stop_local_voice = true;
      }
      effects.clear_speaking_user = Some(left.user_id);
      lobby.screen_shares.retain(|share| share.sharer_user_id != left.user_id);
      effects.clear_video_cache_users.push(left.user_id);
      if local_left || lobby.watching_user_id == Some(left.user_id) {
        let (previous_user_id, changed) = set_watching_user(lobby, None);
        if changed {
          effects.watching_change = Some(previous_user_id);
        }
      }
      if local_left && lobby.selected_channel_id == Some(left.channel_id) {
        lobby.selected_channel_id = None;
        lobby.stream_browser_channel_id = None;
        lobby.users.clear();
      } else if lobby.selected_channel_id == Some(left.channel_id) {
        sync_selected_users(lobby);
      }
      sync_cached_channel_counts(lobby);
      if local_left && was_in_selected_channel {
        effects.notification_sound = Some(NotificationSound::UserKicked);
      } else if !local_left && was_in_selected_channel {
        effects.notification_sound = Some(NotificationSound::VoiceLeave);
      }
    }
    S2C::UserVoiceState(state) => {
      let local_state_changed_externally =
        local_user_id == Some(state.user_id) && local_voice_state != (state.muted, state.deafened);
      tracing::debug!(target: "voice",
        "[voice] user state changed: user={} muted={} deafened={} local={}",
        state.user_id,
        state.muted,
        state.deafened,
        local_user_id == Some(state.user_id)
      );
      if local_user_id == Some(state.user_id) {
        effects.local_voice_update = Some((state.muted, state.deafened));
        if local_state_changed_externally {
          effects.notification_sound = Some(NotificationSound::ModerationAction);
        }
      }
      for users in lobby.users_by_channel.values_mut() {
        if let Some(user) = users.iter_mut().find(|user| user.user_id == state.user_id) {
          user.muted = state.muted;
          user.deafened = state.deafened;
        }
      }
      sync_selected_users(lobby);
    }
    S2C::UserRoleChanged(changed) => {
      tracing::debug!(target: "lobby",
        "[lobby] user role changed: user={} role={:?} local={}",
        changed.user_id,
        changed.role,
        local_user_id == Some(changed.user_id)
      );
      for users in lobby.users_by_channel.values_mut() {
        if let Some(user) = users.iter_mut().find(|user| user.user_id == changed.user_id) {
          user.role = changed.role;
        }
      }
      sync_selected_users(lobby);
      if local_user_id == Some(changed.user_id) {
        effects.current_role_update = Some(changed.role);
      }
    }
    S2C::KeepalivePong => {
      lobby.keepalive_ok = true;
      if lobby
        .connection_warning
        .as_ref()
        .is_some_and(|warning| warning.kind == LobbyConnectionWarningKind::KeepalivePongOverdue)
      {
        lobby.connection_warning = None;
      }
      if let Some(sent_at) = context.pending_keepalive_ping {
        lobby.ping_ms = Some(sent_at.elapsed().as_millis().min(u128::from(u32::MAX)) as u32);
      }
    }
    S2C::ScreenShareStarted(started) => {
      let should_notify_stream_started =
        local_user_id != Some(started.sharer_user_id) && user_in_selected_voice_channel(lobby, started.sharer_user_id);
      tracing::info!(target: "video",
        "[video] screen share started: user={} codec={:?} size={}x{} local={}",
        started.sharer_user_id,
        started.metadata.codec,
        started.metadata.width,
        started.metadata.height,
        local_user_id == Some(started.sharer_user_id)
      );
      if let Some(existing) = lobby
        .screen_shares
        .iter_mut()
        .find(|share| share.sharer_user_id == started.sharer_user_id)
      {
        existing.metadata = started.metadata;
      } else {
        lobby.screen_shares.push(LobbyScreenShare {
          sharer_user_id: started.sharer_user_id,
          metadata: started.metadata,
        });
      }
      if should_notify_stream_started {
        effects.notification_sound = Some(NotificationSound::StreamStarted);
      }
    }
    S2C::ScreenShareStopped { sharer_user_id } => {
      let was_watching_stopped_stream = lobby.watching_user_id == Some(sharer_user_id);
      tracing::warn!(target: "video",
        "[video] screen share stopped: user={} local={} watched={}",
        sharer_user_id,
        local_user_id == Some(sharer_user_id),
        was_watching_stopped_stream
      );
      lobby
        .screen_shares
        .retain(|share| share.sharer_user_id != sharer_user_id);
      effects.clear_video_cache_users.push(sharer_user_id);
      if was_watching_stopped_stream {
        let (previous_user_id, changed) = set_watching_user(lobby, None);
        if changed {
          effects.watching_change = Some(previous_user_id);
        }
        if local_user_id != Some(sharer_user_id) {
          effects.notification_sound = Some(NotificationSound::StreamEnded);
        }
      }
    }
    S2C::ScreenShareDenied { reason } => {
      tracing::warn!(target: "video", "[video] screen share denied: {reason}");
      lobby.last_error = Some(reason);
      effects.notification_sound = Some(NotificationSound::ModerationAction);
    }
    S2C::ServerError { code, message: reason } => {
      tracing::error!(target: "network", "[network] server error: code={} message={reason}", code.as_u16());
      if matches!(code, ServerErrorCode::Kicked | ServerErrorCode::Replaced) {
        effects.notification_sound = Some(NotificationSound::UserKicked);
        effects.stop_local_voice = true;
        lobby.disconnected = true;
        lobby.receiver_running = false;
        lobby.connection_warning = None;
        lobby.auto_reconnect_disabled = true;
        lobby.stream_browser_channel_id = None;
        lobby.screen_shares.clear();
        let (previous_user_id, changed) = set_watching_user(lobby, None);
        if changed {
          effects.watching_change = Some(previous_user_id);
        }
        tracing::warn!(
          target: "network",
          "[network] server requested non-reconnectable disconnect: code={} message={reason}",
          code.as_u16()
        );
      }
      lobby.last_error = Some(reason);
    }
    S2C::AdminResult(result) => {
      tracing::info!(target: "admin",
        "[admin] result: success={} message='{}'",
        result.success,
        result.message
      );
      lobby.last_error = if result.success { None } else { Some(result.message) };
    }
    S2C::AuthResponse(_)
    | S2C::ChatFileUploadResp(_)
    | S2C::ChatFileReady { .. }
    | S2C::ChatSearchResp { .. }
    | S2C::ChatPinnedResp { .. } => {}
  }

  effects
}

#[cfg(test)]
mod tests {
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
}
