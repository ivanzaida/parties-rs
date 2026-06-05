use std::sync::{Arc, Mutex};

use lurq::app::component::{ComponentInfo, DevtoolsInspectable};

use crate::network::{
  protocol::{Role, UserId},
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
  }
}

#[allow(dead_code)]
pub struct ConnectedServer {
  pub info: ConnectedServerInfo,
  pub server: Arc<Server>,
}

#[derive(Clone, Default)]
pub struct ServerSession {
  current: Arc<Mutex<Option<ConnectedServer>>>,
  tofu_warning: Arc<Mutex<Option<TofuWarning>>>,
}

#[allow(dead_code)]
impl ServerSession {
  pub fn set_connected(&self, connected: ConnectedServer) {
    *self.current.lock().expect("server session lock poisoned") = Some(connected);
  }

  pub fn clear(&self) {
    *self.current.lock().expect("server session lock poisoned") = None;
    self.clear_tofu_warning();
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

  pub fn set_tofu_warning(&self, warning: TofuWarning) {
    *self.tofu_warning.lock().expect("server session lock poisoned") = Some(warning);
  }

  pub fn clear_tofu_warning(&self) {
    *self.tofu_warning.lock().expect("server session lock poisoned") = None;
  }

  pub fn tofu_warning(&self) -> Option<TofuWarning> {
    self.tofu_warning.lock().expect("server session lock poisoned").clone()
  }
}
