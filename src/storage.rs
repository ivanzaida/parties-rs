use std::{
  collections::HashMap,
  env,
  error::Error,
  fmt, fs,
  path::{Path, PathBuf},
  process::Command,
  time::{SystemTime, UNIX_EPOCH},
};

use lurq::app::component::{ComponentInfo, DevtoolsInspectable};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
  identity::LocalIdentity,
  network::protocol::{PublicKey, Role, SecretKey, UserId},
};

#[derive(Debug)]
pub enum StorageError {
  Io(std::io::Error),
  Sql(rusqlite::Error),
  InvalidBlob(&'static str),
  InvalidRole(u8),
  Time(std::time::SystemTimeError),
}

impl fmt::Display for StorageError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(error) => write!(f, "io: {error}"),
      Self::Sql(error) => write!(f, "sqlite: {error}"),
      Self::InvalidBlob(column) => write!(f, "invalid identity blob: {column}"),
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
  pub launch_parties_at_login: bool,
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
  pub hotkey_push_to_talk: String,
  pub hotkey_toggle_mute: String,
  pub hotkey_toggle_deafen: String,
  pub video_webcam_device: String,
  pub video_codec: String,
  pub video_scale_percent: i32,
  pub video_fps: i32,
  pub video_bitrate_mbps: f32,
  pub locale: String,
}

impl Default for AppSettings {
  fn default() -> Self {
    Self {
      start_muted_when_joining: true,
      launch_parties_at_login: false,
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
      hotkey_push_to_talk: String::new(),
      hotkey_toggle_mute: String::new(),
      hotkey_toggle_deafen: String::new(),
      video_webcam_device: String::new(),
      video_codec: "AV1".to_owned(),
      video_scale_percent: 100,
      video_fps: 60,
      video_bitrate_mbps: 20.0,
      locale: "en".to_owned(),
    }
  }
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

impl DevtoolsInspectable for StoredServer {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowState {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredUpdateState {
  pub last_checked_at: i64,
  pub last_seen_version: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Storage {
  path: PathBuf,
}

impl DevtoolsInspectable for Storage {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
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
        launch_parties_at_login,
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
        hotkey_push_to_talk,
        hotkey_toggle_mute,
        hotkey_toggle_deafen,
        video_webcam_device,
        video_codec,
        video_scale_percent,
        video_fps,
        video_bitrate_mbps,
        locale
      )
      VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
      "#,
      params![
        bool_to_int(settings.start_muted_when_joining),
        bool_to_int(settings.launch_parties_at_login),
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
        &settings.hotkey_push_to_talk,
        &settings.hotkey_toggle_mute,
        &settings.hotkey_toggle_deafen,
        &settings.video_webcam_device,
        &settings.video_codec,
        settings.video_scale_percent,
        settings.video_fps,
        settings.video_bitrate_mbps,
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
        launch_parties_at_login,
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
        hotkey_push_to_talk,
        hotkey_toggle_mute,
        hotkey_toggle_deafen,
        video_webcam_device,
        video_codec,
        video_scale_percent,
        video_fps,
        video_bitrate_mbps,
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
      launch_parties_at_login: int_to_bool(row.get(1)?),
      display_name: row.get(2)?,
      audio_input_device: row.get(3)?,
      audio_output_device: row.get(4)?,
      notification_volume: row.get(5)?,
      notification_sound_overrides: row.get(6)?,
      noise_cancellation: int_to_bool(row.get(7)?),
      voice_normalization: int_to_bool(row.get(8)?),
      voice_normalization_target_level: row.get(9)?,
      echo_cancellation: int_to_bool(row.get(10)?),
      voice_activation: int_to_bool(row.get(11)?),
      voice_activation_threshold: row.get(12)?,
      push_to_talk: int_to_bool(row.get(13)?),
      hotkey_push_to_talk: row.get(14)?,
      hotkey_toggle_mute: row.get(15)?,
      hotkey_toggle_deafen: row.get(16)?,
      video_webcam_device: row.get(17)?,
      video_codec: row.get(18)?,
      video_scale_percent: row.get(19)?,
      video_fps: row.get(20)?,
      video_bitrate_mbps: row.get(21)?,
      locale: row.get(22)?,
    })
  }

  pub fn save_window_state(&self, state: WindowState) -> Result<(), StorageError> {
    let conn = self.connection()?;
    conn.execute(
      "INSERT OR REPLACE INTO app_window_state (id, x, y, width, height) VALUES (1, ?1, ?2, ?3, ?4)",
      params![state.x, state.y, state.width as i64, state.height as i64],
    )?;
    Ok(())
  }

  pub fn load_window_state(&self) -> Result<Option<WindowState>, StorageError> {
    let conn = self.connection()?;
    let state = conn
      .query_row(
        "SELECT x, y, width, height FROM app_window_state WHERE id = 1",
        [],
        |row| {
          let width = row.get::<_, i64>(2)?.max(1) as u32;
          let height = row.get::<_, i64>(3)?.max(1) as u32;
          Ok(WindowState {
            x: row.get(0)?,
            y: row.get(1)?,
            width,
            height,
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

  pub fn save_stream_volume_override(&self, server_id: &str, user_id: UserId, volume: i32) -> Result<(), StorageError> {
    self.save_volume_override_in_table("stream_volume_overrides", server_id, user_id, volume)
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
        launch_parties_at_login INTEGER NOT NULL DEFAULT 0,
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
        hotkey_push_to_talk TEXT NOT NULL DEFAULT '',
        hotkey_toggle_mute TEXT NOT NULL DEFAULT '',
        hotkey_toggle_deafen TEXT NOT NULL DEFAULT '',
        video_webcam_device TEXT NOT NULL DEFAULT '',
        video_codec TEXT NOT NULL DEFAULT 'AV1',
        video_scale_percent INTEGER NOT NULL DEFAULT 100,
        video_fps INTEGER NOT NULL DEFAULT 60,
        video_bitrate_mbps REAL NOT NULL DEFAULT 20,
        locale TEXT NOT NULL DEFAULT 'en'
      );

      CREATE TABLE IF NOT EXISTS app_window_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        x INTEGER NOT NULL,
        y INTEGER NOT NULL,
        width INTEGER NOT NULL DEFAULT 1280,
        height INTEGER NOT NULL DEFAULT 900
      );

      CREATE TABLE IF NOT EXISTS app_update_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        last_checked_at INTEGER NOT NULL DEFAULT 0,
        last_seen_version TEXT NOT NULL DEFAULT ''
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
    if !column_exists(&conn, "app_settings", "launch_parties_at_login")? {
      conn.execute(
        "ALTER TABLE app_settings ADD COLUMN launch_parties_at_login INTEGER NOT NULL DEFAULT 0",
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

fn default_db_path() -> PathBuf {
  if let Some(path) = startup_db_file_arg(env::args_os().skip(1)) {
    return path;
  }

  env::current_exe()
    .ok()
    .and_then(db_path_next_to_executable)
    .unwrap_or_else(|| PathBuf::from("parties.db"))
}

fn db_path_next_to_executable(executable: impl AsRef<Path>) -> Option<PathBuf> {
  executable.as_ref().parent().map(|parent| parent.join("parties.db"))
}

fn startup_db_file_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<PathBuf> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();

    for prefix in ["-db_file=", "--db_file="] {
      if let Some(path) = arg_text.strip_prefix(prefix).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
      }
    }

    if arg_text == "-db_file" || arg_text == "--db_file" {
      return args.next().map(PathBuf::from);
    }
  }

  None
}

#[cfg(test)]
mod tests {
  use std::time::{SystemTime, UNIX_EPOCH};

  use super::*;
  use crate::identity;

  const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident";

  #[test]
  fn startup_db_file_arg_supports_equals_form() {
    assert_eq!(
      startup_db_file_arg([std::ffi::OsString::from("-db_file=custom.db")]),
      Some(PathBuf::from("custom.db"))
    );
  }

  #[test]
  fn startup_db_file_arg_supports_separate_value_form() {
    assert_eq!(
      startup_db_file_arg([
        std::ffi::OsString::from("--db_file"),
        std::ffi::OsString::from("custom.db")
      ]),
      Some(PathBuf::from("custom.db"))
    );
  }

  #[test]
  fn default_db_path_uses_executable_directory() {
    let executable = PathBuf::from("Apps").join("Parties").join("parties-rs.exe");
    assert_eq!(
      db_path_next_to_executable(&executable),
      Some(PathBuf::from("Apps").join("Parties").join("parties.db"))
    );
  }

  #[test]
  fn identity_round_trips() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = env::temp_dir().join(format!("parties-rs-storage-{nonce}.db"));
    let storage = Storage::open(&path).unwrap();
    let identity = identity::restore_seed_phrase(PHRASE).unwrap();

    storage.save_identity(&identity).unwrap();

    let loaded = storage.load_identity().unwrap().unwrap();
    assert_eq!(loaded, identity);

    storage.delete_identity().unwrap();
    assert!(storage.load_identity().unwrap().is_none());

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }

  #[test]
  fn servers_round_trip() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = env::temp_dir().join(format!("parties-rs-storage-servers-{nonce}.db"));
    let storage = Storage::open(&path).unwrap();
    let server = StoredServer {
      address: "127.0.0.1:7800".to_owned(),
      server_name: "Local".to_owned(),
      user_id: 7,
      role: Role::Admin,
      certificate_fingerprint: "aa:bb".to_owned(),
      server_password: "secret".to_owned(),
      display_name: "alice".to_owned(),
    };

    storage.save_server(&server).unwrap();

    let servers = storage.load_servers().unwrap();
    assert_eq!(servers, vec![server]);

    storage.delete_server("127.0.0.1:7800").unwrap();
    assert!(storage.load_servers().unwrap().is_empty());

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }

  #[test]
  fn settings_round_trip() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = env::temp_dir().join(format!("parties-rs-storage-settings-{nonce}.db"));
    let storage = Storage::open(&path).unwrap();

    assert_eq!(storage.load_settings().unwrap(), AppSettings::default());

    let settings = AppSettings {
      start_muted_when_joining: false,
      launch_parties_at_login: true,
      display_name: "alice".to_owned(),
      audio_input_device: "Microphone".to_owned(),
      audio_output_device: "Speakers".to_owned(),
      notification_volume: 72,
      notification_sound_overrides: r#"{"chat_message":"user_kicked"}"#.to_owned(),
      noise_cancellation: false,
      voice_normalization: true,
      voice_normalization_target_level: 84,
      echo_cancellation: true,
      voice_activation: false,
      voice_activation_threshold: 31,
      push_to_talk: true,
      hotkey_push_to_talk: "Ctrl+P".to_owned(),
      hotkey_toggle_mute: "Ctrl+M".to_owned(),
      hotkey_toggle_deafen: "Ctrl+D".to_owned(),
      video_webcam_device: "Webcam".to_owned(),
      video_codec: "H.264".to_owned(),
      video_scale_percent: 75,
      video_fps: 30,
      video_bitrate_mbps: 12.5,
      locale: "uk".to_owned(),
    };
    storage.save_settings(&settings).unwrap();
    assert_eq!(storage.load_settings().unwrap(), settings);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }

  #[test]
  fn window_state_round_trips() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = env::temp_dir().join(format!("parties-rs-storage-window-{nonce}.db"));
    let storage = Storage::open(&path).unwrap();

    assert_eq!(storage.load_window_state().unwrap(), None);

    let state = WindowState {
      x: 320,
      y: 180,
      width: 1440,
      height: 960,
    };
    storage.save_window_state(state).unwrap();
    assert_eq!(storage.load_window_state().unwrap(), Some(state));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }

  #[test]
  fn update_state_round_trips() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = env::temp_dir().join(format!("parties-rs-storage-update-{nonce}.db"));
    let storage = Storage::open(&path).unwrap();

    assert_eq!(storage.load_update_state().unwrap(), StoredUpdateState::default());

    let state = StoredUpdateState {
      last_checked_at: 1_777_777,
      last_seen_version: "0.1.9".to_owned(),
    };
    storage.save_update_state(&state).unwrap();
    assert_eq!(storage.load_update_state().unwrap(), state);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }

  #[test]
  fn volume_overrides_round_trip() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = env::temp_dir().join(format!("parties-rs-storage-volume-{nonce}.db"));
    let storage = Storage::open(&path).unwrap();

    assert_eq!(storage.load_volume_override("server-a", 7).unwrap(), None);

    storage.save_volume_override("server-a", 7, 42).unwrap();
    storage.save_volume_override("server-a", 9, 75).unwrap();
    storage.save_volume_override("server-b", 7, 88).unwrap();

    assert_eq!(storage.load_volume_override("server-a", 7).unwrap(), Some(42));
    assert_eq!(storage.load_volume_override("server-b", 7).unwrap(), Some(88));
    assert_eq!(storage.load_volume_overrides("server-a").unwrap().len(), 2);

    storage.save_volume_override("server-a", 7, 100).unwrap();
    assert_eq!(storage.load_volume_override("server-a", 7).unwrap(), None);
    assert_eq!(storage.load_volume_override("server-a", 9).unwrap(), Some(75));

    assert_eq!(storage.load_stream_volume_override("server-a", 7).unwrap(), None);
    storage.save_stream_volume_override("server-a", 7, 33).unwrap();
    assert_eq!(storage.load_stream_volume_override("server-a", 7).unwrap(), Some(33));
    assert_eq!(storage.load_volume_override("server-a", 7).unwrap(), None);
    storage.save_stream_volume_override("server-a", 7, 100).unwrap();
    assert_eq!(storage.load_stream_volume_override("server-a", 7).unwrap(), None);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }
}
