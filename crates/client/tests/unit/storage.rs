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
fn startup_db_file_arg_supports_db_path_alias() {
  assert_eq!(
    startup_db_file_arg([std::ffi::OsString::from("--db_path=custom.db")]),
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

#[cfg(target_os = "macos")]
#[test]
fn macos_default_db_path_uses_application_support() {
  let home = PathBuf::from("/Users/alice");
  assert_eq!(
    macos_application_support_db_path_from_home(home),
    PathBuf::from("/Users/alice")
      .join("Library")
      .join("Application Support")
      .join("Parties")
      .join("parties.db")
  );
}

#[cfg(target_os = "macos")]
#[test]
fn sqlite_sidecar_path_appends_suffix_to_db_path() {
  assert_eq!(
    sqlite_sidecar_path(Path::new("/tmp/parties.db"), "wal"),
    PathBuf::from("/tmp/parties.db-wal")
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
fn update_resume_state_is_consumed_once() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let path = env::temp_dir().join(format!("parties-rs-storage-update-resume-{nonce}.db"));
  let storage = Storage::open(&path).unwrap();
  let state = StoredUpdateResumeState {
    server_address: "127.0.0.1:7800".to_owned(),
    voice_channel_id: Some(42),
    muted: true,
    deafened: false,
  };

  assert_eq!(storage.load_update_resume_state().unwrap(), None);
  storage.save_update_resume_state(&state).unwrap();
  assert_eq!(storage.load_update_resume_state().unwrap(), Some(state.clone()));
  assert_eq!(storage.take_update_resume_state().unwrap(), Some(state));
  assert_eq!(storage.load_update_resume_state().unwrap(), None);
  assert_eq!(storage.take_update_resume_state().unwrap(), None);

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
    debug_mode_enabled: true,
    sentry_reports_enabled: Some(true),
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
    push_to_talk_release_delay_ms: 600,
    hotkey_push_to_talk: "Ctrl+P".to_owned(),
    hotkey_toggle_mute: "Ctrl+M".to_owned(),
    hotkey_toggle_deafen: "Ctrl+D".to_owned(),
    video_webcam_device: "Webcam".to_owned(),
    video_codec: "H.264".to_owned(),
    video_scale_percent: 75,
    video_fps: 30,
    video_bitrate_mbps: 12.5,
    video_hardware_decoding: false,
    locale: "uk".to_owned(),
  };
  storage.save_settings(&settings).unwrap();
  assert_eq!(storage.load_settings().unwrap(), settings);

  let _ = fs::remove_file(&path);
  let _ = fs::remove_file(format!("{}-wal", path.display()));
  let _ = fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn settings_migrates_debug_chat_to_debug_mode() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let path = env::temp_dir().join(format!("parties-rs-storage-debug-mode-migration-{nonce}.db"));
  let conn = Connection::open(&path).unwrap();
  conn
    .execute_batch(
      r#"
      CREATE TABLE app_settings (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        start_muted_when_joining INTEGER NOT NULL DEFAULT 1,
        debug_chat_enabled INTEGER NOT NULL DEFAULT 0
      );
      INSERT INTO app_settings (id, debug_chat_enabled) VALUES (1, 1);
      "#,
    )
    .unwrap();
  drop(conn);

  let storage = Storage::open(&path).unwrap();

  assert!(storage.load_settings().unwrap().debug_mode_enabled);

  let _ = fs::remove_file(&path);
  let _ = fs::remove_file(format!("{}-wal", path.display()));
  let _ = fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn imports_legacy_parties_config() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let path = env::temp_dir().join(format!("parties-rs-storage-legacy-import-{nonce}.db"));
  let legacy_path = env::temp_dir().join(format!("parties-rs-storage-legacy-source-{nonce}.db"));
  let conn = Connection::open(&legacy_path).unwrap();
  conn
    .execute_batch(
      r#"
      CREATE TABLE identity (
        id INTEGER PRIMARY KEY,
        seed_phrase TEXT NOT NULL,
        public_key BLOB NOT NULL,
        secret_key BLOB NOT NULL
      );
      CREATE TABLE saved_servers (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        host TEXT NOT NULL,
        port INTEGER NOT NULL DEFAULT 7800,
        fingerprint TEXT NOT NULL DEFAULT '',
        last_username TEXT NOT NULL DEFAULT '',
        password TEXT NOT NULL DEFAULT ''
      );
      CREATE TABLE tofu_certs (
        host TEXT NOT NULL,
        port INTEGER NOT NULL,
        fingerprint TEXT NOT NULL,
        first_seen TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        PRIMARY KEY (host, port)
      );
      "#,
    )
    .unwrap();
  conn
    .execute(
      "INSERT INTO identity (id, seed_phrase, public_key, secret_key) VALUES (1, ?1, ?2, ?3)",
      params!["alpha beta", vec![1_u8; 32], vec![2_u8; 32]],
    )
    .unwrap();
  conn
      .execute(
        "INSERT INTO saved_servers (id, name, host, port, fingerprint, last_username, password) VALUES (1, 'Legacy', 'example.com', 7800, '', 'ivan', 'pw')",
        [],
      )
      .unwrap();
  conn
    .execute(
      "INSERT INTO tofu_certs (host, port, fingerprint) VALUES ('example.com', 7800, 'aa:bb')",
      [],
    )
    .unwrap();
  drop(conn);

  let storage = Storage::open(&path).unwrap();
  let summary = storage.import_legacy_parties_config(&legacy_path).unwrap();

  assert_eq!(
    summary,
    LegacyPartiesImportSummary {
      imported_identity: true,
      imported_servers: 1
    }
  );
  assert_eq!(storage.load_identity().unwrap().unwrap().secret_key, [2_u8; 32]);
  assert_eq!(storage.load_settings().unwrap().display_name, "ivan");
  assert_eq!(
    storage.load_server("example.com:7800").unwrap().unwrap(),
    StoredServer {
      address: "example.com:7800".to_owned(),
      server_name: "Legacy".to_owned(),
      user_id: 0,
      role: Role::User,
      certificate_fingerprint: "aa:bb".to_owned(),
      server_password: "pw".to_owned(),
      display_name: "ivan".to_owned()
    }
  );

  let _ = fs::remove_file(&path);
  let _ = fs::remove_file(format!("{}-wal", path.display()));
  let _ = fs::remove_file(format!("{}-shm", path.display()));
  let _ = fs::remove_file(&legacy_path);
  let _ = fs::remove_file(format!("{}-wal", legacy_path.display()));
  let _ = fs::remove_file(format!("{}-shm", legacy_path.display()));
}

#[test]
fn rejects_invalid_legacy_parties_config_format() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let path = env::temp_dir().join(format!("parties-rs-storage-invalid-legacy-target-{nonce}.db"));
  let legacy_path = env::temp_dir().join(format!("parties-rs-storage-invalid-legacy-source-{nonce}.db"));
  let conn = Connection::open(&legacy_path).unwrap();
  conn
    .execute("CREATE TABLE unrelated (id INTEGER PRIMARY KEY)", [])
    .unwrap();
  drop(conn);

  let storage = Storage::open(&path).unwrap();
  let error = storage.import_legacy_parties_config(&legacy_path).unwrap_err();

  assert!(matches!(error, StorageError::InvalidLegacyConfig(_)));

  let _ = fs::remove_file(&path);
  let _ = fs::remove_file(format!("{}-wal", path.display()));
  let _ = fs::remove_file(format!("{}-shm", path.display()));
  let _ = fs::remove_file(&legacy_path);
  let _ = fs::remove_file(format!("{}-wal", legacy_path.display()));
  let _ = fs::remove_file(format!("{}-shm", legacy_path.display()));
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

  assert!(!storage.load_user_normalization("server-a", 7).unwrap());
  storage.save_user_normalization("server-a", 7, true).unwrap();
  storage.save_user_normalization("server-a", 9, true).unwrap();
  storage.save_user_normalization("server-b", 7, true).unwrap();
  assert!(storage.load_user_normalization("server-a", 7).unwrap());
  assert!(storage.load_user_normalization("server-b", 7).unwrap());
  assert_eq!(storage.load_user_normalizations("server-a").unwrap().len(), 2);
  storage.save_user_normalization("server-a", 7, false).unwrap();
  assert!(!storage.load_user_normalization("server-a", 7).unwrap());
  assert!(storage.load_user_normalization("server-a", 9).unwrap());

  let _ = fs::remove_file(&path);
  let _ = fs::remove_file(format!("{}-wal", path.display()));
  let _ = fs::remove_file(format!("{}-shm", path.display()));
}
