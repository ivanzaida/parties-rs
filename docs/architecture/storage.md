# Storage

The client uses two local persistence mechanisms with separate responsibilities:

| Store | File | Owner | Purpose |
| --- | --- | --- | --- |
| SQLite via `rusqlite` | `parties.db` | `crates/client/src/storage.rs` | App/domain data that the client owns and migrates. |
| `lurq` persistent storage | `lurq.redb` | `lurq::persistent_storage` configured in `crates/client/src/main.rs` | UI/runtime state that belongs to the app shell or `lurq` runtime. |

Keep durable product data in SQLite. Keep window/UI runtime state in `lurq` persistent storage.

## SQLite: `parties.db`

SQLite is the canonical store for client-owned data. `Storage::open_default()` opens the default database and creates/migrates the schema.

Default path behavior:

- Windows and other non-macOS builds use `parties.db` next to the executable, unless a startup DB path argument is supplied.
- macOS uses `~/Library/Application Support/Parties/parties.db`.
- macOS migrates a legacy executable-adjacent DB into Application Support if needed.
- Startup arguments can override the path with forms such as `-db_file=custom.db` or `--db_path=custom.db`.

SQLite tables currently cover:

- `identity`: local seed phrase, public key, and secret key.
- `servers`: saved server addresses, names, fingerprints, display names, roles, and trust data.
- `app_settings`: user settings such as display name, audio devices, notification volume, notification sound overrides, voice settings, hotkeys, video settings, hardware decoding preference, and locale.
- `app_window_state`: legacy/fallback window position, size, and fullscreen state.
- `app_update_state`: downloaded/staged update state.
- `app_update_resume`: server/voice state used to resume after an update restart.
- `volume_overrides`: per-server/per-user voice volume overrides.
- `stream_volume_overrides`: per-server/per-user stream volume overrides.
- `voice_normalization_overrides`: per-server/per-user normalization overrides.

## `lurq` Persistent Storage: `lurq.redb`

The app configures `lurq` persistent storage at:

```text
Storage::default_data_dir()/lurq.redb
```

Today the app writes window state there:

- `window.x`
- `window.y`
- `window.width`
- `window.height`
- `window.full_screen`

Startup loads window state from `lurq.redb` first. If it is missing, startup falls back to the legacy SQLite `app_window_state` row and then writes the validated state into `lurq.redb`.

This split keeps the window/app-shell state close to the UI runtime and leaves SQLite focused on Parties domain data.

## Migration Rules

SQLite schema evolution is handled in code by checking for missing columns and applying `ALTER TABLE` statements. This keeps existing installations forward-compatible without requiring a separate migration tool.

`lurq` persistent storage should be treated as small key/value state. If a value becomes important product data, move it into SQLite and provide a migration/fallback path.

## Settings Flow

Settings screens load from `Storage`, mutate an `AppSettings` value, save it to SQLite, and then update live services where needed. Examples include notification audio settings and video hardware decoding preferences.
