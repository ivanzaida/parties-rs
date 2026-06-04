use std::{env, error::Error, fmt, fs, path::PathBuf};

use lurq::app::component::{ComponentInfo, DevtoolsInspectable};
use rusqlite::{Connection, params};

use crate::{
  identity::LocalIdentity,
  network::protocol::{PublicKey, SecretKey},
};

#[derive(Debug)]
pub enum StorageError {
  Io(std::io::Error),
  Sql(rusqlite::Error),
  InvalidBlob(&'static str),
}

impl fmt::Display for StorageError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(error) => write!(f, "io: {error}"),
      Self::Sql(error) => write!(f, "sqlite: {error}"),
      Self::InvalidBlob(column) => write!(f, "invalid identity blob: {column}"),
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
      "#,
    )?;
    Ok(())
  }
}

fn fixed_32<T>(bytes: &[u8], column: &'static str) -> Result<T, StorageError>
where
  T: From<[u8; 32]>,
{
  let array: [u8; 32] = bytes.try_into().map_err(|_| StorageError::InvalidBlob(column))?;
  Ok(T::from(array))
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
}
