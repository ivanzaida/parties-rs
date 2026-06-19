use std::{
  collections::{HashMap, HashSet},
  env,
  error::Error,
  fmt, fs,
  path::{Path, PathBuf},
  process::Command,
  time::{SystemTime, UNIX_EPOCH},
};

use lurq::{
  app::component::{ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
  core::Store,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::{
  identity::LocalIdentity,
  network::protocol::{ChannelId, DEFAULT_PORT, PublicKey, Role, SecretKey, UserId},
};

#[derive(Debug)]
pub enum StorageError {
  Io(std::io::Error),
  Sql(rusqlite::Error),
  InvalidBlob(&'static str),
  InvalidLegacyConfig(String),
  InvalidRole(u8),
  Time(std::time::SystemTimeError),
}

impl fmt::Display for StorageError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(error) => write!(f, "io: {error}"),
      Self::Sql(error) => write!(f, "sqlite: {error}"),
      Self::InvalidBlob(column) => write!(f, "invalid identity blob: {column}"),
      Self::InvalidLegacyConfig(error) => write!(f, "unsupported legacy config format: {error}"),
      Self::InvalidRole(role) => write!(f, "invalid stored server role: {role}"),
      Self::Time(error) => write!(f, "time: {error}"),
    }
  }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
  fn from(value: std::io::Error) -> Self {
    Self::Io(value)
  }
}

impl From<rusqlite::Error> for StorageError {
  fn from(value: rusqlite::Error) -> Self {
    Self::Sql(value)
  }
}

impl From<std::time::SystemTimeError> for StorageError {
  fn from(value: std::time::SystemTimeError) -> Self {
    Self::Time(value)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppSettings {
  pub start_muted_when_joining: bool,
  pub debug_mode_enabled: bool,
  pub sentry_reports_enabled: Option<bool>,
  pub display_name: String,
  pub audio_input_device: String,
  pub audio_output_device: String,
  pub notification_volume: i32,
  pub notification_sound_overrides: String,
  pub noise_cancellation: bool,
  pub voice_normalization: bool,
  pub voice_normalization_target_level: i32,
  pub echo_cancellation: bool,
  pub voice_activation: bool,
  pub voice_activation_threshold: i32,
  pub push_to_talk: bool,
  pub push_to_talk_release_delay_ms: i32,
  pub hotkey_push_to_talk: String,
  pub hotkey_toggle_mute: String,
  pub hotkey_toggle_deafen: String,
  pub video_webcam_device: String,
  pub video_codec: String,
  pub video_scale_percent: i32,
  pub video_fps: i32,
  pub video_bitrate_mbps: f32,
  pub video_hardware_decoding: bool,
  pub locale: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct AppDisplayName {
  pub value: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct AppDebugModeEnabled {
  pub value: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct AppSentryReportsEnabled {
  pub value: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct AppLocale {
  pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct AppHotkeySettings {
  pub push_to_talk: String,
  pub toggle_mute: String,
  pub toggle_deafen: String,
}

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
pub struct AppStreamSettings {
  pub video_codec: String,
  pub video_scale_percent: i32,
  pub video_fps: i32,
  pub video_bitrate_mbps: f32,
}

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
pub struct AppVideoSettings {
  pub video_webcam_device: String,
  pub video_codec: String,
  pub video_scale_percent: i32,
  pub video_fps: i32,
  pub video_bitrate_mbps: f32,
  pub video_hardware_decoding: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct AppAudioSettings {
  pub start_muted_when_joining: bool,
  pub audio_input_device: String,
  pub audio_output_device: String,
  pub notification_volume: i32,
  pub notification_sound_overrides: String,
  pub noise_cancellation: bool,
  pub voice_normalization: bool,
  pub voice_normalization_target_level: i32,
  pub echo_cancellation: bool,
  pub voice_activation: bool,
  pub voice_activation_threshold: i32,
  pub push_to_talk: bool,
  pub push_to_talk_release_delay_ms: i32,
}

impl Default for AppStreamSettings {
  fn default() -> Self {
    let settings = AppSettings::default();
    Self {
      video_codec: settings.video_codec,
      video_scale_percent: settings.video_scale_percent,
      video_fps: settings.video_fps,
      video_bitrate_mbps: settings.video_bitrate_mbps,
    }
  }
}

impl Default for AppVideoSettings {
  fn default() -> Self {
    Self::from(&AppSettings::default())
  }
}

impl From<&AppSettings> for AppVideoSettings {
  fn from(settings: &AppSettings) -> Self {
    Self {
      video_webcam_device: settings.video_webcam_device.clone(),
      video_codec: settings.video_codec.clone(),
      video_scale_percent: settings.video_scale_percent,
      video_fps: settings.video_fps,
      video_bitrate_mbps: settings.video_bitrate_mbps,
      video_hardware_decoding: settings.video_hardware_decoding,
    }
  }
}

impl Default for AppAudioSettings {
  fn default() -> Self {
    Self::from(&AppSettings::default())
  }
}

impl From<&AppSettings> for AppAudioSettings {
  fn from(settings: &AppSettings) -> Self {
    Self {
      start_muted_when_joining: settings.start_muted_when_joining,
      audio_input_device: settings.audio_input_device.clone(),
      audio_output_device: settings.audio_output_device.clone(),
      notification_volume: settings.notification_volume,
      notification_sound_overrides: settings.notification_sound_overrides.clone(),
      noise_cancellation: settings.noise_cancellation,
      voice_normalization: settings.voice_normalization,
      voice_normalization_target_level: settings.voice_normalization_target_level,
      echo_cancellation: settings.echo_cancellation,
      voice_activation: settings.voice_activation,
      voice_activation_threshold: settings.voice_activation_threshold,
      push_to_talk: settings.push_to_talk,
      push_to_talk_release_delay_ms: settings.push_to_talk_release_delay_ms,
    }
  }
}

impl Default for AppSettings {
  fn default() -> Self {
    Self {
      start_muted_when_joining: true,
      debug_mode_enabled: false,
      sentry_reports_enabled: None,
      display_name: default_display_name(),
      audio_input_device: String::new(),
      audio_output_device: String::new(),
      notification_volume: 100,
      notification_sound_overrides: String::new(),
      noise_cancellation: true,
      voice_normalization: true,
      voice_normalization_target_level: 100,
      echo_cancellation: false,
      voice_activation: true,
      voice_activation_threshold: 27,
      push_to_talk: false,
      push_to_talk_release_delay_ms: 0,
      hotkey_push_to_talk: String::new(),
      hotkey_toggle_mute: String::new(),
      hotkey_toggle_deafen: String::new(),
      video_webcam_device: String::new(),
      video_codec: "AV1".to_owned(),
      video_scale_percent: 100,
      video_fps: 60,
      video_bitrate_mbps: 20.0,
      video_hardware_decoding: true,
      locale: "en".to_owned(),
    }
  }
}

impl DevtoolsInspectable for AppSettings {}
impl DevtoolsInspectable for LocalIdentity {}

#[derive(Clone)]
pub struct AppSettingsUpdater {
  settings_store: Store<AppSettings>,
  storage: Option<Storage>,
}

impl AppSettingsUpdater {
  pub fn new(settings_store: Store<AppSettings>, storage: Option<Storage>) -> Self {
    Self {
      settings_store,
      storage,
    }
  }

  pub fn update(&self, f: impl FnOnce(&mut AppSettings)) -> AppSettings {
    update_app_settings(&self.settings_store, self.storage.as_ref(), f)
  }

  pub fn has_storage(&self) -> bool {
    self.storage.is_some()
  }
}

pub fn update_app_settings(
  settings_store: &Store<AppSettings>,
  storage: Option<&Storage>,
  f: impl FnOnce(&mut AppSettings),
) -> AppSettings {
  let mut settings = settings_store.get();
  let previous = settings.clone();
  f(&mut settings);

  if settings != previous {
    if let Some(storage) = storage
      && let Err(error) = storage.save_settings(&settings)
    {
      tracing::debug!(target: "settings", "failed to save app settings: {error}");
    }
    settings_store.set(settings.clone());
  }

  settings
}

pub fn default_display_name() -> String {
  ["USERNAME", "USER", "LOGNAME"]
    .iter()
    .find_map(|key| env::var(key).ok())
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty())
    .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredServer {
  pub address: String,
  pub server_name: String,
  pub user_id: UserId,
  pub role: Role,
  pub certificate_fingerprint: String,
  pub server_password: String,
  pub display_name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerUserAudioPreferences {
  pub voice_volumes: HashMap<UserId, i32>,
  pub stream_volumes: HashMap<UserId, i32>,
  pub normalized_users: HashSet<UserId>,
}

impl DevtoolsInspectable for ServerUserAudioPreferences {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserAudioPreferences {
  pub servers: HashMap<String, ServerUserAudioPreferences>,
}

impl DevtoolsInspectable for UserAudioPreferences {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct LegacyPartiesImportSummary {
  pub imported_identity: bool,
  pub imported_servers: usize,
}

impl DevtoolsInspectable for StoredServer {
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
      "certificate_fingerprint",
      std::any::type_name::<String>(),
      self.certificate_fingerprint.clone(),
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

pub fn stored_server_by_address(servers: &[StoredServer], address: &str) -> Option<StoredServer> {
  servers.iter().find(|server| server.address == address).cloned()
}

pub fn upsert_stored_server(
  servers_store: Option<&Store<Vec<StoredServer>>>,
  storage: Option<&Storage>,
  server: StoredServer,
) -> Result<(), StorageError> {
  if let Some(storage) = storage {
    storage.save_server(&server)?;
  }

  if let Some(servers_store) = servers_store {
    let mut servers = servers_store.get();
    servers.retain(|existing| existing.address != server.address);
    servers.push(server);
    sort_stored_servers(&mut servers);
    servers_store.set(servers);
  }

  Ok(())
}

pub fn delete_stored_server(
  servers_store: Option<&Store<Vec<StoredServer>>>,
  storage: Option<&Storage>,
  address: &str,
) -> Result<(), StorageError> {
  if let Some(storage) = storage {
    storage.delete_server(address)?;
  }

  if let Some(servers_store) = servers_store {
    let mut servers = servers_store.get();
    let original_len = servers.len();
    servers.retain(|server| server.address != address);
    if servers.len() != original_len {
      servers_store.set(servers);
    }
  }

  Ok(())
}

pub fn save_local_identity(
  identity_store: Option<&Store<Option<LocalIdentity>>>,
  storage: Option<&Storage>,
  identity: LocalIdentity,
) -> Result<(), StorageError> {
  if let Some(storage) = storage {
    storage.save_identity(&identity)?;
  }

  if let Some(identity_store) = identity_store {
    identity_store.set(Some(identity));
  }

  Ok(())
}

pub fn delete_local_identity(
  identity_store: Option<&Store<Option<LocalIdentity>>>,
  storage: Option<&Storage>,
) -> Result<(), StorageError> {
  if let Some(storage) = storage {
    storage.delete_identity()?;
  }

  if let Some(identity_store) = identity_store {
    identity_store.set(None);
  }

  Ok(())
}

pub fn server_user_audio_preferences(
  preferences_store: Option<&Store<UserAudioPreferences>>,
  storage: Option<&Storage>,
  server_id: &str,
) -> ServerUserAudioPreferences {
  if let Some(preferences) = preferences_store
    .as_ref()
    .and_then(|preferences| preferences.with(|preferences| preferences.servers.get(server_id).cloned()))
  {
    return preferences;
  }

  let preferences = storage
    .and_then(|storage| storage.load_server_user_audio_preferences(server_id).ok())
    .unwrap_or_default();
  if let Some(preferences_store) = preferences_store {
    let mut all_preferences = preferences_store.get();
    all_preferences
      .servers
      .insert(server_id.to_owned(), preferences.clone());
    preferences_store.set(all_preferences);
  }
  preferences
}

pub fn save_voice_volume_override(
  preferences_store: Option<&Store<UserAudioPreferences>>,
  storage: Option<&Storage>,
  server_id: &str,
  user_id: UserId,
  volume: i32,
) -> Result<(), StorageError> {
  let volume = volume.clamp(0, 100);
  if let Some(storage) = storage {
    storage.save_volume_override(server_id, user_id, volume)?;
  }
  update_server_user_audio_preferences(preferences_store, server_id, |preferences| {
    if volume == 100 {
      preferences.voice_volumes.remove(&user_id);
    } else {
      preferences.voice_volumes.insert(user_id, volume);
    }
  });
  Ok(())
}

pub fn save_stream_volume_override(
  preferences_store: Option<&Store<UserAudioPreferences>>,
  storage: Option<&Storage>,
  server_id: &str,
  user_id: UserId,
  volume: i32,
) -> Result<(), StorageError> {
  let volume = volume.clamp(0, 100);
  if let Some(storage) = storage {
    storage.save_stream_volume_override(server_id, user_id, volume)?;
  }
  update_server_user_audio_preferences(preferences_store, server_id, |preferences| {
    if volume == 100 {
      preferences.stream_volumes.remove(&user_id);
    } else {
      preferences.stream_volumes.insert(user_id, volume);
    }
  });
  Ok(())
}

pub fn save_voice_normalization_override(
  preferences_store: Option<&Store<UserAudioPreferences>>,
  storage: Option<&Storage>,
  server_id: &str,
  user_id: UserId,
  enabled: bool,
) -> Result<(), StorageError> {
  if let Some(storage) = storage {
    storage.save_user_normalization(server_id, user_id, enabled)?;
  }
  update_server_user_audio_preferences(preferences_store, server_id, |preferences| {
    if enabled {
      preferences.normalized_users.insert(user_id);
    } else {
      preferences.normalized_users.remove(&user_id);
    }
  });
  Ok(())
}

fn update_server_user_audio_preferences(
  preferences_store: Option<&Store<UserAudioPreferences>>,
  server_id: &str,
  update: impl FnOnce(&mut ServerUserAudioPreferences),
) {
  let Some(preferences_store) = preferences_store else {
    return;
  };
  let mut all_preferences = preferences_store.get();
  update(all_preferences.servers.entry(server_id.to_owned()).or_default());
  preferences_store.set(all_preferences);
}

fn sort_stored_servers(servers: &mut [StoredServer]) {
  servers.sort_by(|a, b| {
    let left = if a.server_name.trim().is_empty() {
      &a.address
    } else {
      &a.server_name
    };
    let right = if b.server_name.trim().is_empty() {
      &b.address
    } else {
      &b.server_name
    };
    left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
  });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowState {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
  pub full_screen: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredUpdateState {
  pub last_checked_at: i64,
  pub last_seen_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredUpdateResumeState {
  pub server_address: String,
  pub voice_channel_id: Option<ChannelId>,
  pub muted: bool,
  pub deafened: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Storage {
  path: PathBuf,
}

impl DevtoolsInspectable for Storage {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "path",
      std::any::type_name::<Self>(),
      self.path.display().to_string(),
    ));
  }
}

impl Storage {
  pub fn open_default() -> Result<Self, StorageError> {
    Self::open(default_db_path())
  }

  pub fn default_data_dir() -> PathBuf {
    default_db_path()
      .parent()
      .map(PathBuf::from)
      .unwrap_or_else(|| PathBuf::from("."))
  }

  pub fn open_default_data_dir() -> bool {
    let path = Self::default_data_dir();
    if fs::create_dir_all(&path).is_err() {
      return false;
    }

    open_folder(&path)
  }

  pub fn open(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
    let path = path.into();
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
      fs::create_dir_all(parent)?;
    }

    let storage = Self { path };
    storage.create_schema()?;
    Ok(storage)
  }

  pub fn save_identity(&self, identity: &LocalIdentity) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute(
      "INSERT OR REPLACE INTO identity (id, seed_phrase, public_key, secret_key) VALUES (1, ?1, ?2, ?3)",
      params![
        identity.seed_phrase.as_deref().unwrap_or(""),
        identity.public_key.as_slice(),
        identity.secret_key.as_slice()
      ],
    )?;
    Ok(())
  }

  pub fn delete_identity(&self) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute("DELETE FROM identity WHERE id = 1", [])?;
    Ok(())
  }

  pub fn load_identity(&self) -> Result<Option<LocalIdentity>, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare("SELECT seed_phrase, public_key, secret_key FROM identity WHERE id = 1")?;
    let mut rows = stmt.query([])?;

    let Some(row) = rows.next()? else {
      return Ok(None);
    };

    let seed_phrase: String = row.get(0)?;
    let public_key_blob: Vec<u8> = row.get(1)?;
    let secret_key_blob: Vec<u8> = row.get(2)?;

    Ok(Some(LocalIdentity {
      seed_phrase: if seed_phrase.is_empty() {
        None
      } else {
        Some(seed_phrase)
      },
      public_key: fixed_32::<PublicKey>(&public_key_blob, "public_key")?,
      secret_key: fixed_32::<SecretKey>(&secret_key_blob, "secret_key")?,
    }))
  }

  pub fn has_identity(&self) -> Result<bool, StorageError> {
    Ok(self.load_identity()?.is_some())
  }

  pub fn save_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute(
      r#"
      INSERT OR REPLACE INTO app_settings (
        id,
        start_muted_when_joining,
        debug_mode_enabled,
        sentry_reports_enabled,
        display_name,
        audio_input_device,
        audio_output_device,
        notification_volume,
        notification_sound_overrides,
        noise_cancellation,
        voice_normalization,
        voice_normalization_target_level,
        echo_cancellation,
        voice_activation,
        voice_activation_threshold,
        push_to_talk,
        push_to_talk_release_delay_ms,
        hotkey_push_to_talk,
        hotkey_toggle_mute,
        hotkey_toggle_deafen,
        video_webcam_device,
        video_codec,
        video_scale_percent,
        video_fps,
        video_bitrate_mbps,
        video_hardware_decoding,
        locale
      )
      VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)
      "#,
      params![
        bool_to_int(settings.start_muted_when_joining),
        bool_to_int(settings.debug_mode_enabled),
        settings.sentry_reports_enabled.map(bool_to_int),
        &settings.display_name,
        &settings.audio_input_device,
        &settings.audio_output_device,
        settings.notification_volume,
        &settings.notification_sound_overrides,
        bool_to_int(settings.noise_cancellation),
        bool_to_int(settings.voice_normalization),
        settings.voice_normalization_target_level,
        bool_to_int(settings.echo_cancellation),
        bool_to_int(settings.voice_activation),
        settings.voice_activation_threshold,
        bool_to_int(settings.push_to_talk),
        settings.push_to_talk_release_delay_ms.clamp(0, 2000),
        &settings.hotkey_push_to_talk,
        &settings.hotkey_toggle_mute,
        &settings.hotkey_toggle_deafen,
        &settings.video_webcam_device,
        &settings.video_codec,
        settings.video_scale_percent,
        settings.video_fps,
        settings.video_bitrate_mbps,
        bool_to_int(settings.video_hardware_decoding),
        &settings.locale
      ],
    )?;
    Ok(())
  }

  pub fn load_settings(&self) -> Result<AppSettings, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare(
      r#"
      SELECT
        start_muted_when_joining,
        debug_mode_enabled,
        sentry_reports_enabled,
        display_name,
        audio_input_device,
        audio_output_device,
        notification_volume,
        notification_sound_overrides,
        noise_cancellation,
        voice_normalization,
        voice_normalization_target_level,
        echo_cancellation,
        voice_activation,
        voice_activation_threshold,
        push_to_talk,
        push_to_talk_release_delay_ms,
        hotkey_push_to_talk,
        hotkey_toggle_mute,
        hotkey_toggle_deafen,
        video_webcam_device,
        video_codec,
        video_scale_percent,
        video_fps,
        video_bitrate_mbps,
        video_hardware_decoding,
        locale
      FROM app_settings
      WHERE id = 1
      "#,
    )?;
    let mut rows = stmt.query([])?;

    let Some(row) = rows.next()? else {
      return Ok(AppSettings::default());
    };

    Ok(AppSettings {
      start_muted_when_joining: int_to_bool(row.get(0)?),
      debug_mode_enabled: int_to_bool(row.get(1)?),
      sentry_reports_enabled: row.get::<_, Option<i64>>(2)?.map(int_to_bool),
      display_name: row.get(3)?,
      audio_input_device: row.get(4)?,
      audio_output_device: row.get(5)?,
      notification_volume: row.get(6)?,
      notification_sound_overrides: row.get(7)?,
      noise_cancellation: int_to_bool(row.get(8)?),
      voice_normalization: int_to_bool(row.get(9)?),
      voice_normalization_target_level: row.get(10)?,
      echo_cancellation: int_to_bool(row.get(11)?),
      voice_activation: int_to_bool(row.get(12)?),
      voice_activation_threshold: row.get(13)?,
      push_to_talk: int_to_bool(row.get(14)?),
      push_to_talk_release_delay_ms: row.get::<_, i32>(15)?.clamp(0, 2000),
      hotkey_push_to_talk: row.get(16)?,
      hotkey_toggle_mute: row.get(17)?,
      hotkey_toggle_deafen: row.get(18)?,
      video_webcam_device: row.get(19)?,
      video_codec: row.get(20)?,
      video_scale_percent: row.get(21)?,
      video_fps: row.get(22)?,
      video_bitrate_mbps: row.get(23)?,
      video_hardware_decoding: int_to_bool(row.get(24)?),
      locale: row.get(25)?,
    })
  }

  pub fn load_window_state(&self) -> Result<Option<WindowState>, StorageError> {
    let conn = self.connection()?;
    let state = conn
      .query_row(
        "SELECT x, y, width, height, full_screen FROM app_window_state WHERE id = 1",
        [],
        |row| {
          let width = row.get::<_, i64>(2)?.max(1) as u32;
          let height = row.get::<_, i64>(3)?.max(1) as u32;
          Ok(WindowState {
            x: row.get(0)?,
            y: row.get(1)?,
            width,
            height,
            full_screen: int_to_bool(row.get(4)?),
          })
        },
      )
      .optional()?;
    Ok(state)
  }

  pub fn save_update_state(&self, state: &StoredUpdateState) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute(
      "INSERT OR REPLACE INTO app_update_state (id, last_checked_at, last_seen_version) VALUES (1, ?1, ?2)",
      params![state.last_checked_at, &state.last_seen_version],
    )?;
    Ok(())
  }

  pub fn load_update_state(&self) -> Result<StoredUpdateState, StorageError> {
    let conn = self.connection()?;
    let state = conn
      .query_row(
        "SELECT last_checked_at, last_seen_version FROM app_update_state WHERE id = 1",
        [],
        |row| {
          Ok(StoredUpdateState {
            last_checked_at: row.get(0)?,
            last_seen_version: row.get(1)?,
          })
        },
      )
      .optional()?;

    Ok(state.unwrap_or_default())
  }

  pub fn save_update_resume_state(&self, state: &StoredUpdateResumeState) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute(
      r#"
      INSERT OR REPLACE INTO app_update_resume (id, server_address, voice_channel_id, muted, deafened)
      VALUES (1, ?1, ?2, ?3, ?4)
      "#,
      params![
        &state.server_address,
        state.voice_channel_id.map(i64::from),
        bool_to_int(state.muted),
        bool_to_int(state.deafened)
      ],
    )?;
    Ok(())
  }

  pub fn load_update_resume_state(&self) -> Result<Option<StoredUpdateResumeState>, StorageError> {
    let conn = self.connection()?;
    let state = conn
      .query_row(
        "SELECT server_address, voice_channel_id, muted, deafened FROM app_update_resume WHERE id = 1",
        [],
        |row| {
          Ok(StoredUpdateResumeState {
            server_address: row.get(0)?,
            voice_channel_id: row.get::<_, Option<i64>>(1)?.map(|value| value as ChannelId),
            muted: int_to_bool(row.get(2)?),
            deafened: int_to_bool(row.get(3)?),
          })
        },
      )
      .optional()?;
    Ok(state)
  }

  pub fn clear_update_resume_state(&self) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute("DELETE FROM app_update_resume WHERE id = 1", [])?;
    Ok(())
  }

  pub fn take_update_resume_state(&self) -> Result<Option<StoredUpdateResumeState>, StorageError> {
    let state = self.load_update_resume_state()?;
    if state.is_some() {
      self.clear_update_resume_state()?;
    }
    Ok(state)
  }

  pub fn save_server(&self, server: &StoredServer) -> Result<(), StorageError> {
    let conn = self.connection()?;
    let updated_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    conn.execute(
      r#"
      INSERT OR REPLACE INTO servers (address, server_name, user_id, role, updated_at, certificate_fingerprint, server_password, display_name)
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
      "#,
      params![
        &server.address,
        &server.server_name,
        server.user_id as i64,
        server.role as u8,
        updated_at,
        &server.certificate_fingerprint,
        &server.server_password,
        &server.display_name
      ],
    )?;
    Ok(())
  }

  pub fn import_legacy_parties_config(
    &self,
    path: impl AsRef<Path>,
  ) -> Result<LegacyPartiesImportSummary, StorageError> {
    let legacy = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    validate_legacy_parties_config_schema(&legacy)?;
    let mut summary = LegacyPartiesImportSummary::default();

    let identity = legacy
      .query_row(
        "SELECT seed_phrase, public_key, secret_key FROM identity WHERE id = 1",
        [],
        |row| {
          let seed_phrase: String = row.get(0)?;
          let public_key_blob: Vec<u8> = row.get(1)?;
          let secret_key_blob: Vec<u8> = row.get(2)?;
          Ok((seed_phrase, public_key_blob, secret_key_blob))
        },
      )
      .optional()?;

    if let Some((seed_phrase, public_key_blob, secret_key_blob)) = identity {
      self.save_identity(&LocalIdentity {
        seed_phrase: if seed_phrase.trim().is_empty() {
          None
        } else {
          Some(seed_phrase)
        },
        public_key: fixed_32::<PublicKey>(&public_key_blob, "public_key")?,
        secret_key: fixed_32::<SecretKey>(&secret_key_blob, "secret_key")?,
      })?;
      summary.imported_identity = true;
    }

    let mut first_display_name = None;
    let mut stmt = legacy.prepare(
      r#"
      SELECT
        s.name,
        s.host,
        s.port,
        COALESCE(NULLIF(s.fingerprint, ''), t.fingerprint, ''),
        s.last_username,
        s.password
      FROM saved_servers s
      LEFT JOIN tofu_certs t ON t.host = s.host AND t.port = s.port
      ORDER BY s.id ASC
      "#,
    )?;
    let rows = stmt.query_map([], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
      ))
    })?;

    for row in rows {
      let (server_name, host, port, fingerprint, display_name, password) = row?;
      let Some(address) = legacy_server_address(&host, port) else {
        continue;
      };
      if first_display_name.is_none() && !display_name.trim().is_empty() {
        first_display_name = Some(display_name.trim().to_owned());
      }
      self.save_server(&StoredServer {
        address,
        server_name,
        user_id: 0,
        role: Role::User,
        certificate_fingerprint: fingerprint,
        server_password: password,
        display_name,
      })?;
      summary.imported_servers += 1;
    }

    if let Some(display_name) = first_display_name {
      let mut settings = self.load_settings().unwrap_or_default();
      settings.display_name = display_name;
      self.save_settings(&settings)?;
    }

    Ok(summary)
  }

  pub fn load_server(&self, address: &str) -> Result<Option<StoredServer>, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare(
      r#"
      SELECT address, server_name, user_id, role, certificate_fingerprint, server_password, display_name
      FROM servers
      WHERE address = ?1
      "#,
    )?;
    let mut rows = stmt.query(params![address])?;

    let Some(row) = rows.next()? else {
      return Ok(None);
    };

    let role: u8 = row.get(3)?;
    let role = Role::from_u8(role).ok_or(StorageError::InvalidRole(role))?;
    Ok(Some(StoredServer {
      address: row.get(0)?,
      server_name: row.get(1)?,
      user_id: row.get::<_, i64>(2)? as UserId,
      role,
      certificate_fingerprint: row.get(4)?,
      server_password: row.get(5)?,
      display_name: row.get(6)?,
    }))
  }

  pub fn load_servers(&self) -> Result<Vec<StoredServer>, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare(
      r#"
      SELECT address, server_name, user_id, role, certificate_fingerprint, server_password, display_name
      FROM servers
      ORDER BY updated_at DESC, server_name ASC, address ASC
      "#,
    )?;
    let rows = stmt.query_map([], |row| {
      let role: u8 = row.get(3)?;
      Ok((
        row.get(0)?,
        row.get(1)?,
        row.get::<_, i64>(2)?,
        role,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
      ))
    })?;

    let mut servers = Vec::new();
    for row in rows {
      let (address, server_name, user_id, role, certificate_fingerprint, server_password, display_name) = row?;
      let role = Role::from_u8(role).ok_or(StorageError::InvalidRole(role))?;
      servers.push(StoredServer {
        address,
        server_name,
        user_id: user_id as UserId,
        role,
        certificate_fingerprint,
        server_password,
        display_name,
      });
    }
    Ok(servers)
  }

  pub fn delete_server(&self, address: &str) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute("DELETE FROM volume_overrides WHERE server_id = ?1", params![address])?;
    conn.execute(
      "DELETE FROM stream_volume_overrides WHERE server_id = ?1",
      params![address],
    )?;
    conn.execute(
      "DELETE FROM voice_normalization_overrides WHERE server_id = ?1",
      params![address],
    )?;
    conn.execute("DELETE FROM servers WHERE address = ?1", params![address])?;
    Ok(())
  }

  pub fn load_volume_override(&self, server_id: &str, user_id: UserId) -> Result<Option<i32>, StorageError> {
    let conn = self.connection()?;
    let volume = conn
      .query_row(
        "SELECT volume FROM volume_overrides WHERE server_id = ?1 AND user_id = ?2",
        params![server_id, user_id as i64],
        |row| row.get::<_, i32>(0),
      )
      .optional()?
      .map(|volume| volume.clamp(0, 100));
    Ok(volume)
  }

  pub fn load_volume_overrides(&self, server_id: &str) -> Result<HashMap<UserId, i32>, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare("SELECT user_id, volume FROM volume_overrides WHERE server_id = ?1")?;
    let rows = stmt.query_map(params![server_id], |row| {
      Ok((row.get::<_, i64>(0)? as UserId, row.get::<_, i32>(1)?.clamp(0, 100)))
    })?;

    let mut volumes = HashMap::new();
    for row in rows {
      let (user_id, volume) = row?;
      if volume != 100 {
        volumes.insert(user_id, volume);
      }
    }
    Ok(volumes)
  }

  pub fn save_volume_override(&self, server_id: &str, user_id: UserId, volume: i32) -> Result<(), StorageError> {
    self.save_volume_override_in_table("volume_overrides", server_id, user_id, volume)
  }

  pub fn load_stream_volume_override(&self, server_id: &str, user_id: UserId) -> Result<Option<i32>, StorageError> {
    let conn = self.connection()?;
    let volume = conn
      .query_row(
        "SELECT volume FROM stream_volume_overrides WHERE server_id = ?1 AND user_id = ?2",
        params![server_id, user_id as i64],
        |row| row.get::<_, i32>(0),
      )
      .optional()?
      .map(|volume| volume.clamp(0, 100));
    Ok(volume)
  }

  pub fn load_stream_volume_overrides(&self, server_id: &str) -> Result<HashMap<UserId, i32>, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare("SELECT user_id, volume FROM stream_volume_overrides WHERE server_id = ?1")?;
    let rows = stmt.query_map(params![server_id], |row| {
      Ok((row.get::<_, i64>(0)? as UserId, row.get::<_, i32>(1)?.clamp(0, 100)))
    })?;

    let mut volumes = HashMap::new();
    for row in rows {
      let (user_id, volume) = row?;
      if volume != 100 {
        volumes.insert(user_id, volume);
      }
    }
    Ok(volumes)
  }

  pub fn save_stream_volume_override(&self, server_id: &str, user_id: UserId, volume: i32) -> Result<(), StorageError> {
    self.save_volume_override_in_table("stream_volume_overrides", server_id, user_id, volume)
  }

  pub fn load_user_normalization(&self, server_id: &str, user_id: UserId) -> Result<bool, StorageError> {
    let conn = self.connection()?;
    let enabled = conn
      .query_row(
        "SELECT 1 FROM voice_normalization_overrides WHERE server_id = ?1 AND user_id = ?2",
        params![server_id, user_id as i64],
        |_| Ok(true),
      )
      .optional()?
      .unwrap_or(false);
    Ok(enabled)
  }

  pub fn load_user_normalizations(&self, server_id: &str) -> Result<HashSet<UserId>, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare("SELECT user_id FROM voice_normalization_overrides WHERE server_id = ?1")?;
    let rows = stmt.query_map(params![server_id], |row| Ok(row.get::<_, i64>(0)? as UserId))?;

    let mut users = HashSet::new();
    for row in rows {
      users.insert(row?);
    }
    Ok(users)
  }

  pub fn load_server_user_audio_preferences(
    &self,
    server_id: &str,
  ) -> Result<ServerUserAudioPreferences, StorageError> {
    Ok(ServerUserAudioPreferences {
      voice_volumes: self.load_volume_overrides(server_id)?,
      stream_volumes: self.load_stream_volume_overrides(server_id)?,
      normalized_users: self.load_user_normalizations(server_id)?,
    })
  }

  pub fn load_user_audio_preferences(&self) -> Result<UserAudioPreferences, StorageError> {
    let conn = self.connection()?;
    let mut preferences = HashMap::<String, ServerUserAudioPreferences>::new();
    load_volume_preferences_table(&conn, "volume_overrides", &mut preferences, |entry| {
      &mut entry.voice_volumes
    })?;
    load_volume_preferences_table(&conn, "stream_volume_overrides", &mut preferences, |entry| {
      &mut entry.stream_volumes
    })?;

    let mut stmt = conn.prepare("SELECT server_id, user_id FROM voice_normalization_overrides")?;
    let rows = stmt.query_map([], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as UserId))
    })?;
    for row in rows {
      let (server_id, user_id) = row?;
      preferences
        .entry(server_id)
        .or_default()
        .normalized_users
        .insert(user_id);
    }

    Ok(UserAudioPreferences { servers: preferences })
  }

  pub fn save_user_normalization(&self, server_id: &str, user_id: UserId, enabled: bool) -> Result<(), StorageError> {
    let conn = self.connection()?;
    if !enabled {
      conn.execute(
        "DELETE FROM voice_normalization_overrides WHERE server_id = ?1 AND user_id = ?2",
        params![server_id, user_id as i64],
      )?;
      return Ok(());
    }

    conn.execute(
      r#"
      INSERT INTO voice_normalization_overrides (server_id, user_id)
      VALUES (?1, ?2)
      ON CONFLICT(server_id, user_id) DO NOTHING
      "#,
      params![server_id, user_id as i64],
    )?;
    Ok(())
  }

  fn save_volume_override_in_table(
    &self,
    table: &str,
    server_id: &str,
    user_id: UserId,
    volume: i32,
  ) -> Result<(), StorageError> {
    let conn = self.connection()?;
    let volume = volume.clamp(0, 100);
    if volume == 100 {
      let sql = format!("DELETE FROM {table} WHERE server_id = ?1 AND user_id = ?2");
      conn.execute(&sql, params![server_id, user_id as i64])?;
      return Ok(());
    }

    let sql = format!(
      r#"
      INSERT INTO {table} (server_id, user_id, volume)
      VALUES (?1, ?2, ?3)
      ON CONFLICT(server_id, user_id) DO UPDATE SET volume = excluded.volume
      "#
    );
    conn.execute(&sql, params![server_id, user_id as i64, volume])?;
    Ok(())
  }

  fn connection(&self) -> Result<Connection, StorageError> {
    let conn = Connection::open(&self.path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
  }

  fn create_schema(&self) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute_batch(
      r#"
      CREATE TABLE IF NOT EXISTS identity (
        id          INTEGER PRIMARY KEY CHECK (id = 1),
        seed_phrase TEXT NOT NULL,
        public_key  BLOB NOT NULL,
        secret_key  BLOB NOT NULL
      );

      CREATE TABLE IF NOT EXISTS servers (
        address     TEXT PRIMARY KEY,
        server_name TEXT NOT NULL,
        user_id     INTEGER NOT NULL,
        role        INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        certificate_fingerprint TEXT NOT NULL DEFAULT '',
        server_password TEXT NOT NULL DEFAULT '',
        display_name TEXT NOT NULL DEFAULT ''
      );

      CREATE TABLE IF NOT EXISTS app_settings (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        start_muted_when_joining INTEGER NOT NULL DEFAULT 1,
        debug_mode_enabled INTEGER NOT NULL DEFAULT 0,
        sentry_reports_enabled INTEGER DEFAULT NULL,
        display_name TEXT NOT NULL DEFAULT '',
        audio_input_device TEXT NOT NULL DEFAULT '',
        audio_output_device TEXT NOT NULL DEFAULT '',
        notification_volume INTEGER NOT NULL DEFAULT 100,
        notification_sound_overrides TEXT NOT NULL DEFAULT '',
        noise_cancellation INTEGER NOT NULL DEFAULT 1,
        voice_normalization INTEGER NOT NULL DEFAULT 1,
        voice_normalization_target_level INTEGER NOT NULL DEFAULT 100,
        echo_cancellation INTEGER NOT NULL DEFAULT 0,
        voice_activation INTEGER NOT NULL DEFAULT 1,
        voice_activation_threshold INTEGER NOT NULL DEFAULT 27,
        push_to_talk INTEGER NOT NULL DEFAULT 0,
        push_to_talk_release_delay_ms INTEGER NOT NULL DEFAULT 0,
        hotkey_push_to_talk TEXT NOT NULL DEFAULT '',
        hotkey_toggle_mute TEXT NOT NULL DEFAULT '',
        hotkey_toggle_deafen TEXT NOT NULL DEFAULT '',
        video_webcam_device TEXT NOT NULL DEFAULT '',
        video_codec TEXT NOT NULL DEFAULT 'AV1',
        video_scale_percent INTEGER NOT NULL DEFAULT 100,
        video_fps INTEGER NOT NULL DEFAULT 60,
        video_bitrate_mbps REAL NOT NULL DEFAULT 20,
        video_hardware_decoding INTEGER NOT NULL DEFAULT 1,
        locale TEXT NOT NULL DEFAULT 'en'
      );

      CREATE TABLE IF NOT EXISTS app_window_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        x INTEGER NOT NULL,
        y INTEGER NOT NULL,
        width INTEGER NOT NULL DEFAULT 1280,
        height INTEGER NOT NULL DEFAULT 900,
        full_screen INTEGER NOT NULL DEFAULT 0
      );

      CREATE TABLE IF NOT EXISTS app_update_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        last_checked_at INTEGER NOT NULL DEFAULT 0,
        last_seen_version TEXT NOT NULL DEFAULT ''
      );

      CREATE TABLE IF NOT EXISTS app_update_resume (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        server_address TEXT NOT NULL,
        voice_channel_id INTEGER,
        muted INTEGER NOT NULL DEFAULT 0,
        deafened INTEGER NOT NULL DEFAULT 0
      );

      CREATE TABLE IF NOT EXISTS volume_overrides (
        server_id TEXT NOT NULL,
        user_id INTEGER NOT NULL,
        volume INTEGER NOT NULL,
        PRIMARY KEY (server_id, user_id)
      );

      CREATE TABLE IF NOT EXISTS stream_volume_overrides (
        server_id TEXT NOT NULL,
        user_id INTEGER NOT NULL,
        volume INTEGER NOT NULL,
        PRIMARY KEY (server_id, user_id)
      );

      CREATE TABLE IF NOT EXISTS voice_normalization_overrides (
        server_id TEXT NOT NULL,
        user_id INTEGER NOT NULL,
        PRIMARY KEY (server_id, user_id)
      );
      "#,
    )?;
    if !column_exists(&conn, "servers", "certificate_fingerprint")? {
      conn.execute(
        "ALTER TABLE servers ADD COLUMN certificate_fingerprint TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "servers", "server_password")? {
      conn.execute(
        "ALTER TABLE servers ADD COLUMN server_password TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "servers", "display_name")? {
      conn.execute(
        "ALTER TABLE servers ADD COLUMN display_name TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "start_muted_when_joining")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN start_muted_when_joining INTEGER NOT NULL DEFAULT 1",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "debug_mode_enabled")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN debug_mode_enabled INTEGER NOT NULL DEFAULT 0",
        [],
      )?;
      if column_exists(&conn, "app_settings", "debug_chat_enabled")? {
        conn.execute("UPDATE app_settings SET debug_mode_enabled = debug_chat_enabled", [])?;
      }
    }
    if !column_exists(&conn, "app_settings", "sentry_reports_enabled")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN sentry_reports_enabled INTEGER DEFAULT NULL",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "display_name")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN display_name TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "audio_input_device")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN audio_input_device TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "audio_output_device")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN audio_output_device TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "notification_volume")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN notification_volume INTEGER NOT NULL DEFAULT 100",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "notification_sound_overrides")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN notification_sound_overrides TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "noise_cancellation")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN noise_cancellation INTEGER NOT NULL DEFAULT 1",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "voice_normalization")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN voice_normalization INTEGER NOT NULL DEFAULT 1",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "voice_normalization_target_level")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN voice_normalization_target_level INTEGER NOT NULL DEFAULT 100",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "echo_cancellation")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN echo_cancellation INTEGER NOT NULL DEFAULT 0",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "voice_activation")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN voice_activation INTEGER NOT NULL DEFAULT 1",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "voice_activation_threshold")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN voice_activation_threshold INTEGER NOT NULL DEFAULT 27",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "push_to_talk")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN push_to_talk INTEGER NOT NULL DEFAULT 0",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "push_to_talk_release_delay_ms")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN push_to_talk_release_delay_ms INTEGER NOT NULL DEFAULT 0",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "hotkey_push_to_talk")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN hotkey_push_to_talk TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "hotkey_toggle_mute")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN hotkey_toggle_mute TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "hotkey_toggle_deafen")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN hotkey_toggle_deafen TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "video_webcam_device")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN video_webcam_device TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "video_codec")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN video_codec TEXT NOT NULL DEFAULT 'AV1'",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "video_scale_percent")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN video_scale_percent INTEGER NOT NULL DEFAULT 100",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "video_fps")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN video_fps INTEGER NOT NULL DEFAULT 60",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "video_bitrate_mbps")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN video_bitrate_mbps REAL NOT NULL DEFAULT 20",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "video_hardware_decoding")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN video_hardware_decoding INTEGER NOT NULL DEFAULT 1",
        [],
      )?;
    }
    if !column_exists(&conn, "app_settings", "locale")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN locale TEXT NOT NULL DEFAULT 'en'",
        [],
      )?;
    }
    if !column_exists(&conn, "app_window_state", "width")? {
      conn.execute(
        "ALTER TABLE app_window_state ADD COLUMN width INTEGER NOT NULL DEFAULT 1280",
        [],
      )?;
    }
    if !column_exists(&conn, "app_window_state", "height")? {
      conn.execute(
        "ALTER TABLE app_window_state ADD COLUMN height INTEGER NOT NULL DEFAULT 900",
        [],
      )?;
    }
    if !column_exists(&conn, "app_window_state", "full_screen")? {
      conn.execute(
        "ALTER TABLE app_window_state ADD COLUMN full_screen INTEGER NOT NULL DEFAULT 0",
        [],
      )?;
    }
    if !column_exists(&conn, "app_update_state", "last_checked_at")? {
      conn.execute(
        "ALTER TABLE app_update_state ADD COLUMN last_checked_at INTEGER NOT NULL DEFAULT 0",
        [],
      )?;
    }
    if !column_exists(&conn, "app_update_state", "last_seen_version")? {
      conn.execute(
        "ALTER TABLE app_update_state ADD COLUMN last_seen_version TEXT NOT NULL DEFAULT ''",
        [],
      )?;
    }
    Ok(())
  }
}

fn open_folder(path: &std::path::Path) -> bool {
  #[cfg(target_os = "windows")]
  {
    Command::new("explorer").arg(path).spawn().is_ok()
  }
  #[cfg(target_os = "macos")]
  {
    Command::new("open").arg(path).spawn().is_ok()
  }
  #[cfg(all(unix, not(target_os = "macos")))]
  {
    Command::new("xdg-open").arg(path).spawn().is_ok()
  }
  #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
  {
    let _ = path;
    false
  }
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, StorageError> {
  let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
  let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;

  for row in rows {
    if row? == column {
      return Ok(true);
    }
  }

  Ok(false)
}

fn fixed_32<T>(bytes: &[u8], column: &'static str) -> Result<T, StorageError>
where
  T: From<[u8; 32]>,
{
  let array: [u8; 32] = bytes.try_into().map_err(|_| StorageError::InvalidBlob(column))?;
  Ok(T::from(array))
}

fn bool_to_int(value: bool) -> i64 {
  if value { 1 } else { 0 }
}

fn int_to_bool(value: i64) -> bool {
  value != 0
}

fn load_volume_preferences_table(
  conn: &Connection,
  table: &str,
  preferences: &mut HashMap<String, ServerUserAudioPreferences>,
  volume_map: impl Fn(&mut ServerUserAudioPreferences) -> &mut HashMap<UserId, i32>,
) -> Result<(), StorageError> {
  let sql = format!("SELECT server_id, user_id, volume FROM {table}");
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map([], |row| {
    Ok((
      row.get::<_, String>(0)?,
      row.get::<_, i64>(1)? as UserId,
      row.get::<_, i32>(2)?.clamp(0, 100),
    ))
  })?;

  for row in rows {
    let (server_id, user_id, volume) = row?;
    if volume != 100 {
      volume_map(preferences.entry(server_id).or_default()).insert(user_id, volume);
    }
  }
  Ok(())
}

fn validate_legacy_parties_config_schema(conn: &Connection) -> Result<(), StorageError> {
  for (table, columns) in [
    ("identity", &["id", "seed_phrase", "public_key", "secret_key"][..]),
    (
      "saved_servers",
      &["id", "name", "host", "port", "fingerprint", "last_username", "password"][..],
    ),
    ("tofu_certs", &["host", "port", "fingerprint"][..]),
  ] {
    for column in columns {
      if !column_exists(conn, table, column)? {
        return Err(StorageError::InvalidLegacyConfig(format!("missing {table}.{column}")));
      }
    }
  }
  Ok(())
}

fn legacy_server_address(host: &str, port: i64) -> Option<String> {
  let host = host.trim();
  if host.is_empty() {
    return None;
  }
  let port = u16::try_from(port).unwrap_or(DEFAULT_PORT);
  if host.starts_with('[') || !host.contains(':') {
    Some(format!("{host}:{port}"))
  } else {
    Some(format!("[{host}]:{port}"))
  }
}

fn default_db_path() -> PathBuf {
  if let Some(path) = startup_db_file_arg(env::args_os().skip(1)) {
    return path;
  }

  #[cfg(target_os = "macos")]
  {
    let path = macos_application_support_db_path();
    migrate_legacy_executable_db(&path);
    return path;
  }

  #[cfg(not(target_os = "macos"))]
  {
    legacy_executable_db_path().unwrap_or_else(|| PathBuf::from("parties.db"))
  }
}

fn legacy_executable_db_path() -> Option<PathBuf> {
  env::current_exe().ok().and_then(db_path_next_to_executable)
}

fn db_path_next_to_executable(executable: impl AsRef<Path>) -> Option<PathBuf> {
  executable.as_ref().parent().map(|parent| parent.join("parties.db"))
}

#[cfg(target_os = "macos")]
fn macos_application_support_db_path() -> PathBuf {
  env::var_os("HOME")
    .map(PathBuf::from)
    .filter(|home| !home.as_os_str().is_empty())
    .map(macos_application_support_db_path_from_home)
    .unwrap_or_else(|| PathBuf::from("parties.db"))
}

#[cfg(target_os = "macos")]
fn macos_application_support_db_path_from_home(home: PathBuf) -> PathBuf {
  home
    .join("Library")
    .join("Application Support")
    .join("Parties")
    .join("parties.db")
}

#[cfg(target_os = "macos")]
fn migrate_legacy_executable_db(new_path: &Path) {
  if new_path.exists() {
    return;
  }

  let Some(old_path) = legacy_executable_db_path() else {
    return;
  };
  if old_path == new_path || !old_path.exists() {
    return;
  }
  let Some(parent) = new_path.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
    return;
  };
  if fs::create_dir_all(parent).is_err() {
    return;
  }

  if fs::copy(&old_path, new_path).is_err() {
    return;
  }
  copy_sqlite_sidecar(&old_path, new_path, "wal");
  copy_sqlite_sidecar(&old_path, new_path, "shm");
}

#[cfg(target_os = "macos")]
fn copy_sqlite_sidecar(old_path: &Path, new_path: &Path, suffix: &str) {
  let old_sidecar = sqlite_sidecar_path(old_path, suffix);
  if !old_sidecar.exists() {
    return;
  }

  let new_sidecar = sqlite_sidecar_path(new_path, suffix);
  let _ = fs::copy(old_sidecar, new_sidecar);
}

#[cfg(target_os = "macos")]
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
  PathBuf::from(format!("{}-{suffix}", path.display()))
}

fn startup_db_file_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<PathBuf> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();

    for prefix in ["-db_file=", "--db_file=", "-db_path=", "--db_path="] {
      if let Some(path) = arg_text.strip_prefix(prefix).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
      }
    }

    if arg_text == "-db_file" || arg_text == "--db_file" || arg_text == "-db_path" || arg_text == "--db_path" {
      return args.next().map(PathBuf::from);
    }
  }

  None
}

#[cfg(test)]
#[path = "../tests/unit/storage.rs"]
mod tests;
