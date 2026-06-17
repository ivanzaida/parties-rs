# Client Architecture

The client app lives in `crates/client` and builds the `parties-rs` binary.

## Runtime Shape

The app combines:

- `lurq` for the UI runtime, rendering, routing, resources, forms, devtools, and persistence helpers.
- `tokio` for async network and background tasks.
- `quinn` and `rustls` for QUIC transport.
- `rusqlite` for local storage.
- `cpal`, `opus`, and platform APIs for audio.
- Platform video backends for capture, encode, decode, and presentation.

## Main Source Areas

| Path | Purpose |
| --- | --- |
| `src/app.rs` | Top-level app composition, routing, settings modal, hotkeys, update pill. |
| `src/main.rs` | App startup and window configuration. |
| `src/network` | QUIC server connection and wire protocol. |
| `src/session` | Connected server state, reconnect, chat, voice, video, and presentation state. |
| `src/services` | Audio, video, storage-adjacent services, logging, updates, hotkeys, startup, notifications. |
| `src/ui` | Screens and components. |
| `src/storage.rs` | SQLite-backed settings and saved server data. |

## Session Layer

`ServerSession` is the high-level state boundary for connected-server behavior. Split session modules keep hot paths separate:

- `chat_history.rs` and `chat_commands.rs`
- `connection.rs`
- `lobby.rs`
- `speaking.rs`
- `voice_runtime.rs` and `voice_state.rs`
- `video.rs`, `video_sink.rs`, and `video_stream.rs`

