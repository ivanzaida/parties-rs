use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread,
  time::{Duration, Instant},
};

use lurq::app::component::{ComponentInfo, DevtoolsFormatter, DevtoolsInspectable};
use parking_lot::Mutex;

use super::{LobbyConnectionWarningKind, video, voice_runtime};
use crate::network::{
  protocol::{Role, S2C, UserId},
  server::Server,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedServerInfo {
  pub address: String,
  pub server_name: String,
  pub display_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub certificate_fingerprint: String,
}

impl DevtoolsInspectable for ConnectedServerInfo {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "address",
      std::any::type_name::<String>(),
      self.address.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.server_name.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "display_name",
      std::any::type_name::<String>(),
      self.display_name.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "role",
      std::any::type_name::<Role>(),
      format!("{:?}", self.role),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
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
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "address",
      std::any::type_name::<String>(),
      self.address.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.server_name.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "role",
      std::any::type_name::<Role>(),
      format!("{:?}", self.role),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "saved_fingerprint",
      std::any::type_name::<String>(),
      self.saved_fingerprint.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "received_fingerprint",
      std::any::type_name::<String>(),
      self.received_fingerprint.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "server_password",
      std::any::type_name::<String>(),
      if self.server_password.is_empty() {
        String::new()
      } else {
        "<stored>".to_owned()
      },
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
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

struct ConnectedServerState {
  info: ConnectedServerInfo,
  server: Option<Arc<Server>>,
}

pub(super) struct ConnectionRuntime {
  current: Mutex<Option<ConnectedServerState>>,
  tofu_warning: Mutex<Option<TofuWarning>>,
  receiver_started: Mutex<bool>,
  receiver_stop: Mutex<Option<Arc<AtomicBool>>>,
  pending_keepalive_ping: Mutex<Option<Instant>>,
  last_network_activity: Mutex<Instant>,
  shutdown_requested: AtomicBool,
}

impl ConnectionRuntime {
  pub(super) fn new() -> Self {
    Self {
      current: Mutex::new(None),
      tofu_warning: Mutex::new(None),
      receiver_started: Mutex::new(false),
      receiver_stop: Mutex::new(None),
      pending_keepalive_ping: Mutex::new(None),
      last_network_activity: Mutex::new(Instant::now()),
      shutdown_requested: AtomicBool::new(false),
    }
  }

  pub(super) fn set_connected(&self, connected: ConnectedServer) {
    self.shutdown_requested.store(false, Ordering::Relaxed);
    self.stop_receivers();
    self.clear_pending_keepalive_ping();
    self.mark_network_activity();
    *self.current.lock() = Some(ConnectedServerState {
      info: connected.info,
      server: Some(connected.server),
    });
    *self.receiver_started.lock() = false;
  }

  #[cfg(test)]
  pub(super) fn set_connected_info_for_test(&self, info: ConnectedServerInfo) {
    self.shutdown_requested.store(false, Ordering::Relaxed);
    self.stop_receivers();
    self.clear_pending_keepalive_ping();
    self.mark_network_activity();
    *self.current.lock() = Some(ConnectedServerState { info, server: None });
    *self.receiver_started.lock() = false;
  }

  pub(super) fn clear(&self) {
    self.shutdown_requested.store(false, Ordering::Relaxed);
    self.stop_receivers();
    self.clear_pending_keepalive_ping();
    self.mark_network_activity();
    *self.current.lock() = None;
    *self.receiver_started.lock() = false;
    self.clear_tofu_warning();
  }

  pub(super) fn request_shutdown(&self) {
    self.shutdown_requested.store(true, Ordering::Relaxed);
    self.stop_receivers();
  }

  pub(super) fn shutdown_requested(&self) -> bool {
    self.shutdown_requested.load(Ordering::Relaxed)
  }

  pub(super) fn info(&self) -> Option<ConnectedServerInfo> {
    self.current.lock().as_ref().map(|connected| connected.info.clone())
  }

  pub(super) fn server(&self) -> Option<Arc<Server>> {
    self
      .current
      .lock()
      .as_ref()
      .and_then(|connected| connected.server.clone())
  }

  pub(super) fn update_current_role(&self, local_user_id: Option<UserId>, role: Role) {
    let mut current = self.current.lock();
    if let Some(current) = current.as_mut()
      && Some(current.info.user_id) == local_user_id
    {
      current.info.role = role;
    }
  }

  pub(super) fn set_tofu_warning(&self, warning: TofuWarning) {
    *self.tofu_warning.lock() = Some(warning);
  }

  pub(super) fn clear_tofu_warning(&self) {
    *self.tofu_warning.lock() = None;
  }

  pub(super) fn tofu_warning(&self) -> Option<TofuWarning> {
    self.tofu_warning.lock().clone()
  }

  pub(super) fn stop_receivers(&self) {
    if let Some(stop) = self.receiver_stop.lock().as_ref() {
      stop.store(true, Ordering::Relaxed);
    }
  }

  pub(super) fn try_begin_receiver(&self) -> bool {
    let mut started = self.receiver_started.lock();
    if *started {
      return false;
    }
    *started = true;
    true
  }

  pub(super) fn set_receiver_stop(&self, stop: Option<Arc<AtomicBool>>) {
    *self.receiver_stop.lock() = stop;
  }

  pub(super) fn finish_receiver(&self) {
    *self.receiver_stop.lock() = None;
    *self.receiver_started.lock() = false;
  }

  pub(super) fn mark_network_activity(&self) {
    *self.last_network_activity.lock() = Instant::now();
  }

  pub(super) fn network_idle_for(&self, now: Instant) -> Duration {
    now.duration_since(*self.last_network_activity.lock())
  }

  pub(super) fn pending_ping_age_ms(&self, now: Instant) -> Option<u128> {
    self
      .pending_keepalive_ping
      .lock()
      .as_ref()
      .map(|sent_at| now.duration_since(*sent_at).as_millis())
  }

  pub(super) fn take_pending_keepalive_ping(&self) -> Option<Instant> {
    self.pending_keepalive_ping.lock().take()
  }

  pub(super) fn clear_pending_keepalive_ping(&self) {
    *self.pending_keepalive_ping.lock() = None;
  }

  pub(super) fn pending_keepalive_timed_out(&self, now: Instant, timeout: Duration) -> bool {
    let mut pending = self.pending_keepalive_ping.lock();
    match *pending {
      Some(sent_at) if now.duration_since(sent_at) >= timeout => true,
      Some(_) => false,
      None => {
        *pending = Some(now);
        false
      }
    }
  }
}

impl Default for ConnectionRuntime {
  fn default() -> Self {
    Self::new()
  }
}

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) trait ConnectionSession:
  Clone + Send + Sync + 'static + video::VideoReceiverSession + voice_runtime::VoiceReceiverSession
{
  fn connected_server(&self) -> Option<Arc<Server>>;
  fn is_shutdown_requested(&self) -> bool;
  fn lobby_disconnected(&self) -> bool;
  fn try_begin_lobby_receiver(&self) -> bool;
  fn set_video_receiver_stop(&self, stop: Option<Arc<AtomicBool>>);
  fn finish_lobby_receiver(&self);
  fn bump_connection_revision(&self);
  fn mark_connection_network_activity(&self);
  fn network_idle_for(&self, now: Instant) -> Duration;
  fn pending_keepalive_timed_out(&self, now: Instant, timeout: Duration) -> bool;
  fn set_connection_warning(&self, kind: LobbyConnectionWarningKind, message: String);
  fn mark_lobby_error(&self, message: String);
  fn apply_server_message(&self, message: S2C);
}

pub(super) async fn run_lobby_receiver<S>(session: S)
where
  S: ConnectionSession,
{
  let Some(server) = session.connected_server() else {
    tracing::warn!(target: "network", "[network] lobby receiver not started: no connected server");
    return;
  };
  if session.is_shutdown_requested() {
    tracing::warn!(target: "network", "[network] lobby receiver not started: shutdown is in progress");
    return;
  }
  if session.lobby_disconnected() {
    tracing::warn!(target: "network", "[network] lobby receiver not started: lobby is disconnected");
    return;
  }
  if !session.try_begin_lobby_receiver() {
    tracing::warn!(target: "network", "[network] lobby receiver already running");
    return;
  }

  tracing::info!(target: "network", "[network] lobby receiver started");
  session.bump_connection_revision();

  let ping_session = session.clone();
  let ping_server = server.clone();
  let ping_task = tokio::spawn(async move {
    run_keepalive_sender(ping_session, ping_server).await;
  });
  let voice_session = session.clone();
  let voice_server = server.clone();
  let voice_task = tokio::spawn(async move {
    voice_runtime::run_voice_activity_receiver(voice_session, voice_server).await;
  });
  let video_stop = Arc::new(AtomicBool::new(false));
  session.set_video_receiver_stop(Some(video_stop.clone()));
  let video_thread = {
    let video_session = session.clone();
    let video_server = server.clone();
    let video_runtime = tokio::runtime::Handle::current();
    let video_stop = video_stop.clone();
    thread::Builder::new()
      .name("parties-video-receiver".to_owned())
      .spawn(move || {
        video::run_video_receiver(video_session, video_server, video_runtime, video_stop);
      })
      .ok()
  };

  loop {
    match server.recv().await {
      Ok(message) => {
        session.mark_connection_network_activity();
        session.apply_server_message(message);
      }
      Err(error) => {
        let mut error = error.to_string();
        if let Some(close_reason) = server.connection().close_reason() {
          error = format!("{error}; close_reason={close_reason}");
        }
        tracing::warn!(target: "network", "[network] lobby receiver error: {error}");
        session.mark_lobby_error(error);
        break;
      }
    };
  }

  tracing::info!(
    target: "voice",
    "[voice] aborting voice receiver because lobby receiver stopped: {}",
    voice_runtime::VoiceReceiverSession::connection_debug_context(&session)
  );
  voice_task.abort();
  video_stop.store(true, Ordering::Relaxed);
  server.wake_video_datagram_reader();
  ping_task.abort();
  drop(video_thread);
  session.finish_lobby_receiver();
  tracing::info!(target: "network", "[network] lobby receiver stopped");
  session.bump_connection_revision();
}

async fn run_keepalive_sender<S>(session: S, server: Arc<Server>)
where
  S: ConnectionSession,
{
  loop {
    if let Some(error) = server.connection().close_reason() {
      tracing::warn!(target: "network", "[network] connection closed; forcing reconnect: {error}");
      session.mark_lobby_error(format!("connection closed: {error}"));
      break;
    }

    let now = Instant::now();
    let keepalive_timed_out = session.pending_keepalive_timed_out(now, KEEPALIVE_TIMEOUT);
    if keepalive_timed_out {
      let idle_for = session.network_idle_for(now);
      if idle_for < KEEPALIVE_TIMEOUT {
        tracing::debug!(
          target: "network",
          "[network] keepalive pong overdue but inbound traffic is active: ping_age={}s idle={}s",
          KEEPALIVE_TIMEOUT.as_secs(),
          idle_for.as_secs()
        );
        session.set_connection_warning(
          LobbyConnectionWarningKind::KeepalivePongOverdue,
          format!(
            "No pong for {}s, but traffic is still arriving.",
            KEEPALIVE_TIMEOUT.as_secs()
          ),
        );
        tokio::time::sleep(KEEPALIVE_INTERVAL).await;
        continue;
      }
      tracing::warn!(
        target: "network",
        "[network] keepalive timed out: no pong or inbound traffic received within {}s; forcing reconnect",
        KEEPALIVE_TIMEOUT.as_secs()
      );
      session.mark_lobby_error(format!(
        "keepalive timed out after {}s without pong or inbound traffic",
        KEEPALIVE_TIMEOUT.as_secs()
      ));
      break;
    }
    if let Err(error) = server.ping().await {
      tracing::warn!(target: "network", "[network] keepalive send failed; forcing reconnect: {error}");
      session.mark_lobby_error(format!("keepalive send failed: {error}"));
      break;
    }
    tokio::time::sleep(KEEPALIVE_INTERVAL).await;
  }
}

#[cfg(test)]
#[path = "../../tests/unit/session/connection.rs"]
mod tests;
