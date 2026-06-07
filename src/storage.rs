use std::{
  env,
  error::Error,
  fmt, fs,
  path::PathBuf,
  process::Command,
  time::{SystemTime, UNIX_EPOCH},
};

use lurq::app::component::{ComponentInfo, DevtoolsInspectable};
use rusqlite::{Connection, params};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSettings {
  pub start_muted_when_joining: bool,
  pub launch_parties_at_login: bool,
  pub display_name: String,
}

impl Default for AppSettings {
  fn default() -> Self {
    Self {
      start_muted_when_joining: true,
      launch_parties_at_login: false,
      display_name: default_display_name(),
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
      INSERT OR REPLACE INTO app_settings (id, start_muted_when_joining, launch_parties_at_login, display_name)
      VALUES (1, ?1, ?2, ?3)
      "#,
      params![
        bool_to_int(settings.start_muted_when_joining),
        bool_to_int(settings.launch_parties_at_login),
        &settings.display_name
      ],
    )?;
    Ok(())
  }

  pub fn load_settings(&self) -> Result<AppSettings, StorageError> {
    let conn = self.connection()?;
    let mut stmt = conn.prepare(
      r#"
      SELECT start_muted_when_joining, launch_parties_at_login, display_name
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
    })
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
    conn.execute("DELETE FROM servers WHERE address = ?1", params![address])?;
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
        display_name TEXT NOT NULL DEFAULT ''
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
  if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
    return PathBuf::from(local_app_data).join("Parties").join("parties.db");
  }

  if cfg!(target_os = "macos")
    && let Some(home) = env::var_os("HOME")
  {
    return PathBuf::from(home)
      .join("Library")
      .join("Application Support")
      .join("Parties")
      .join("parties.db");
  }

  if let Some(xdg_data_home) = env::var_os("XDG_DATA_HOME") {
    return PathBuf::from(xdg_data_home).join("parties").join("parties.db");
  }

  if let Some(home) = env::var_os("HOME") {
    return PathBuf::from(home)
      .join(".local")
      .join("share")
      .join("parties")
      .join("parties.db");
  }

  PathBuf::from("parties.db")
}

#[cfg(test)]
mod tests {
  use std::time::{SystemTime, UNIX_EPOCH};

  use super::*;
  use crate::identity;

  const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident";

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
    };
    storage.save_settings(&settings).unwrap();
    assert_eq!(storage.load_settings().unwrap(), settings);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
  }
}
