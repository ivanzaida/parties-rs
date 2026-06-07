use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
};

use lurq::{
  app::component::{ComponentInfo, DevtoolsInspectable},
  core::Signal,
};

use crate::network::{
  protocol::{
    ChannelId, Role, S2C, UserId,
    control::{ChannelInfo, ChannelUser as ProtocolChannelUser, ScreenShareMetadata},
  },
  server::Server,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedServerInfo {
  pub address: String,
  pub server_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub certificate_fingerprint: String,
}

impl DevtoolsInspectable for ConnectedServerInfo {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "address",
      std::any::type_name::<String>(),
      self.address.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.server_name.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "role",
      std::any::type_name::<Role>(),
      format!("{:?}", self.role),
    ));
    buffer.push(ComponentInfo::with_value(
      "certificate_fingerprint",
      std::any::type_name::<String>(),
      self.certificate_fingerprint.clone(),
    ));
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TofuWarning {
  pub address: String,
  pub server_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub saved_fingerprint: String,
  pub received_fingerprint: String,
  pub server_password: String,
  pub display_name: String,
}

impl DevtoolsInspectable for TofuWarning {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "address",
      std::any::type_name::<String>(),
      self.address.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.server_name.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "role",
      std::any::type_name::<Role>(),
      format!("{:?}", self.role),
    ));
    buffer.push(ComponentInfo::with_value(
      "saved_fingerprint",
      std::any::type_name::<String>(),
      self.saved_fingerprint.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "received_fingerprint",
      std::any::type_name::<String>(),
      self.received_fingerprint.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "server_password",
      std::any::type_name::<String>(),
      if self.server_password.is_empty() {
        String::new()
      } else {
        "<stored>".to_owned()
      },
    ));
    buffer.push(ComponentInfo::with_value(
      "display_name",
      std::any::type_name::<String>(),
      self.display_name.clone(),
    ));
  }
}

#[allow(dead_code)]
pub struct ConnectedServer {
  pub info: ConnectedServerInfo,
  pub server: Arc<Server>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyChannel {
  pub id: ChannelId,
  pub name: String,
  pub max_users: u32,
  pub sort_order: u32,
  pub user_count: u32,
  pub key_received: bool,
}

impl From<ChannelInfo> for LobbyChannel {
  fn from(channel: ChannelInfo) -> Self {
    Self {
      id: channel.id,
      name: channel.name,
      max_users: channel.max_users,
      sort_order: channel.sort_order,
      user_count: channel.user_count,
      key_received: false,
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
}

impl From<ProtocolChannelUser> for LobbyUser {
  fn from(user: ProtocolChannelUser) -> Self {
    Self {
      user_id: user.user_id,
      username: user.username,
      role: user.role,
      muted: user.muted,
      deafened: user.deafened,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyScreenShare {
  pub sharer_user_id: UserId,
  pub metadata: ScreenShareMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LobbyState {
  pub channels: Vec<LobbyChannel>,
  pub selected_channel_id: Option<ChannelId>,
  pub users: Vec<LobbyUser>,
  pub users_by_channel: HashMap<ChannelId, Vec<LobbyUser>>,
  pub screen_shares: Vec<LobbyScreenShare>,
  pub watching_user_id: Option<UserId>,
  pub receiver_running: bool,
  pub channel_list_received: bool,
  pub keepalive_ok: bool,
  pub disconnected: bool,
  pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct ServerSession {
  current: Arc<Mutex<Option<ConnectedServer>>>,
  tofu_warning: Arc<Mutex<Option<TofuWarning>>>,
  lobby: Arc<Mutex<LobbyState>>,
  receiver_started: Arc<Mutex<bool>>,
  revision: Signal<u64>,
}

impl Default for ServerSession {
  fn default() -> Self {
    Self {
      current: Arc::new(Mutex::new(None)),
      tofu_warning: Arc::new(Mutex::new(None)),
      lobby: Arc::new(Mutex::new(LobbyState::default())),
      receiver_started: Arc::new(Mutex::new(false)),
      revision: Signal::new(0),
    }
  }
}

#[allow(dead_code)]
impl ServerSession {
  pub fn set_connected(&self, connected: ConnectedServer) {
    *self.current.lock().expect("server session lock poisoned") = Some(connected);
    *self.lobby.lock().expect("server session lock poisoned") = LobbyState::default();
    *self.receiver_started.lock().expect("server session lock poisoned") = false;
    self.bump_revision();
  }

  pub fn clear(&self) {
    *self.current.lock().expect("server session lock poisoned") = None;
    self.clear_tofu_warning();
    *self.lobby.lock().expect("server session lock poisoned") = LobbyState::default();
    *self.receiver_started.lock().expect("server session lock poisoned") = false;
    self.bump_revision();
  }

  pub fn info(&self) -> Option<ConnectedServerInfo> {
    self
      .current
      .lock()
      .expect("server session lock poisoned")
      .as_ref()
      .map(|connected| connected.info.clone())
  }

  pub fn server(&self) -> Option<Arc<Server>> {
    self
      .current
      .lock()
      .expect("server session lock poisoned")
      .as_ref()
      .map(|connected| connected.server.clone())
  }

  pub fn local_voice_state(&self) -> Option<(bool, bool)> {
    let user_id = self.info()?.user_id;
    let lobby = self.lobby.lock().expect("server session lock poisoned");

    lobby
      .users
      .iter()
      .chain(lobby.users_by_channel.values().flatten())
      .find(|user| user.user_id == user_id)
      .map(|user| (user.muted, user.deafened))
      .or(Some((false, false)))
  }

  pub fn set_local_voice_state(&self, muted: bool, deafened: bool) {
    let Some(user_id) = self.info().map(|info| info.user_id) else {
      return;
    };

    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      for user in &mut lobby.users {
        if user.user_id == user_id {
          user.muted = muted;
          user.deafened = deafened;
        }
      }
      for users in lobby.users_by_channel.values_mut() {
        for user in users {
          if user.user_id == user_id {
            user.muted = muted;
            user.deafened = deafened;
          }
        }
      }
    }

    self.bump_revision();
  }

  pub fn set_tofu_warning(&self, warning: TofuWarning) {
    *self.tofu_warning.lock().expect("server session lock poisoned") = Some(warning);
  }

  pub fn clear_tofu_warning(&self) {
    *self.tofu_warning.lock().expect("server session lock poisoned") = None;
  }

  pub fn tofu_warning(&self) -> Option<TofuWarning> {
    self.tofu_warning.lock().expect("server session lock poisoned").clone()
  }

  pub fn lobby(&self) -> LobbyState {
    self.lobby.lock().expect("server session lock poisoned").clone()
  }

  pub fn revision(&self) -> Signal<u64> {
    self.revision.clone()
  }

  fn bump_revision(&self) {
    self.revision.update(|revision| *revision = revision.wrapping_add(1));
  }

  fn sync_selected_users(lobby: &mut LobbyState) {
    lobby.users = lobby
      .selected_channel_id
      .and_then(|channel_id| lobby.users_by_channel.get(&channel_id).cloned())
      .unwrap_or_default();
  }

  fn sync_cached_channel_counts(lobby: &mut LobbyState) {
    for channel in &mut lobby.channels {
      if let Some(users) = lobby.users_by_channel.get(&channel.id) {
        channel.user_count = users.len() as u32;
      }
    }
  }

  pub fn select_channel(&self, channel_id: ChannelId) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.selected_channel_id = Some(channel_id);
      for channel in &mut lobby.channels {
        channel.key_received = false;
      }
      Self::sync_selected_users(&mut lobby);
    }
    self.bump_revision();
  }

  pub fn set_watching_user(&self, user_id: Option<UserId>) {
    self
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .watching_user_id = user_id;
    self.bump_revision();
  }

  pub async fn run_lobby_receiver(&self) {
    let Some(server) = self.server() else {
      return;
    };
    if self.lobby.lock().expect("server session lock poisoned").disconnected {
      return;
    }

    {
      let mut started = self.receiver_started.lock().expect("server session lock poisoned");
      if *started {
        return;
      }
      *started = true;
    }
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.receiver_running = true;
      lobby.last_error = None;
    }
    self.bump_revision();

    let session = self.clone();
    let _ = server.ping().await;

    loop {
      match server.recv().await {
        Ok(message) => {
          session.apply_server_message(message);
        }
        Err(error) => {
          session.mark_lobby_error(error.to_string());
          break;
        }
      };
    }

    *session.receiver_started.lock().expect("server session lock poisoned") = false;
    session
      .lobby
      .lock()
      .expect("server session lock poisoned")
      .receiver_running = false;
    session.bump_revision();
  }

  pub fn mark_lobby_error(&self, message: String) {
    {
      let mut lobby = self.lobby.lock().expect("server session lock poisoned");
      lobby.receiver_running = false;
      lobby.disconnected = true;
      lobby.last_error = Some(message);
    }
    self.bump_revision();
  }

  fn apply_server_message(&self, message: S2C) {
    let mut lobby = self.lobby.lock().expect("server session lock poisoned");

    match message {
      S2C::ChannelList(list) => {
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
          Self::sync_selected_users(&mut lobby);
        } else {
          lobby.selected_channel_id = None;
          lobby.users.clear();
        }
      }
      S2C::ChannelUserList(list) => {
        let users = list.users.into_iter().map(LobbyUser::from).collect::<Vec<_>>();
        for user in &users {
          for (channel_id, cached_users) in &mut lobby.users_by_channel {
            if *channel_id != list.channel_id {
              cached_users.retain(|cached| cached.user_id != user.user_id);
            }
          }
        }
        lobby.users_by_channel.insert(list.channel_id, users);
        Self::sync_selected_users(&mut lobby);
        Self::sync_cached_channel_counts(&mut lobby);
      }
      S2C::UserJoinedChannel(joined) => {
        for (channel_id, users) in &mut lobby.users_by_channel {
          if *channel_id != joined.channel_id {
            users.retain(|user| user.user_id != joined.user_id);
          }
        }
        let users = lobby.users_by_channel.entry(joined.channel_id).or_default();
        let inserted = if users.iter().any(|user| user.user_id == joined.user_id) {
          false
        } else {
          users.push(LobbyUser {
            user_id: joined.user_id,
            username: joined.username,
            role: joined.role,
            muted: false,
            deafened: false,
          });
          true
        };
        if lobby.selected_channel_id == Some(joined.channel_id) {
          Self::sync_selected_users(&mut lobby);
        }
        if inserted {
          Self::sync_cached_channel_counts(&mut lobby);
        }
      }
      S2C::UserLeftChannel(left) => {
        if let Some(users) = lobby.users_by_channel.get_mut(&left.channel_id) {
          users.retain(|user| user.user_id != left.user_id);
        }
        if lobby.selected_channel_id == Some(left.channel_id) {
          Self::sync_selected_users(&mut lobby);
        }
        Self::sync_cached_channel_counts(&mut lobby);
      }
      S2C::UserVoiceState(state) => {
        for users in lobby.users_by_channel.values_mut() {
          if let Some(user) = users.iter_mut().find(|user| user.user_id == state.user_id) {
            user.muted = state.muted;
            user.deafened = state.deafened;
          }
        }
        Self::sync_selected_users(&mut lobby);
      }
      S2C::UserRoleChanged(changed) => {
        for users in lobby.users_by_channel.values_mut() {
          if let Some(user) = users.iter_mut().find(|user| user.user_id == changed.user_id) {
            user.role = changed.role;
          }
        }
        Self::sync_selected_users(&mut lobby);
        if let Some(current) = self.current.lock().expect("server session lock poisoned").as_mut()
          && current.info.user_id == changed.user_id
        {
          current.info.role = changed.role;
        }
      }
      S2C::KeepalivePong => {
        lobby.keepalive_ok = true;
      }
      S2C::ChannelKey(key) => {
        if let Some(channel) = lobby.channels.iter_mut().find(|channel| channel.id == key.channel_id) {
          channel.key_received = true;
        }
      }
      S2C::ScreenShareStarted(started) => {
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
      }
      S2C::ScreenShareStopped { sharer_user_id } => {
        lobby
          .screen_shares
          .retain(|share| share.sharer_user_id != sharer_user_id);
        if lobby.watching_user_id == Some(sharer_user_id) {
          lobby.watching_user_id = None;
        }
      }
      S2C::ScreenShareDenied { reason } | S2C::ServerError { message: reason } => {
        lobby.last_error = Some(reason);
      }
      S2C::AdminResult(result) if !result.success => {
        lobby.last_error = Some(result.message);
      }
      S2C::AuthResponse(_)
      | S2C::AdminResult(_)
      | S2C::ChatMessage(_)
      | S2C::ChatHistoryResp(_)
      | S2C::ChatMessageDeleted { .. }
      | S2C::ChatFileUploadResp(_)
      | S2C::ChatFileReady { .. }
      | S2C::ChatSearchResp { .. }
      | S2C::ChatPinnedResp { .. }
      | S2C::ChatChannelList { .. } => {}
    }

    drop(lobby);
    self.bump_revision();
  }
}
