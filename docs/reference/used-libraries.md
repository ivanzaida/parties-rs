# Used Libraries

## Client

| Library | Used For |
| --- | --- |
| `lurq` | UI runtime, routing, rendering, resources, i18n, forms, WGPU/DX12 integration, persistent UI storage, devtools. |
| `tokio` | Async runtime and background tasks. |
| `quinn` | QUIC transport. |
| `rustls` | TLS stack for QUIC/networking. |
| `rusqlite` | Bundled SQLite storage. |
| `bip39` | Seed phrase identity flows. |
| `ed25519-dalek` | Identity signing keys. |
| `sha2` | Hashing. |
| `getrandom` | Secure randomness. |
| `cpal` | Audio input/output devices. |
| `opus` | Voice and bot audio codec. |
| `minimp3` | MP3 decoding for notifications and bot paths. |
| `rdev` | Global input hooks for hotkeys. |
| `rfd` | Native file dialogs. |
| `reqwest` | HTTP requests, including update/Sentry-related support paths. |
| `sentry`, `sentry-tracing` | Runtime crash/error reporting and tracing integration. |
| `tracing`, `tracing-subscriber`, `tracing-appender`, `tracing-log` | Structured logging. |
| `semver` | Version comparison. |
| `zip` | Archive handling. |
| `parking_lot` | Synchronization primitives. |
| `openh264-sys2` | H.264 support where needed by the video stack. |
| `shiguredo_dav1d` | rav1d AV1 software decode fallback. |
| `sonora` | Media/video support dependency used by the client video stack. |
| `windows`, `windows-core` | Windows platform APIs. |
| `core-foundation-sys`, `core-media-sys`, `core-video-sys` | macOS platform APIs. |
| `cc`, `winresource` | Native build script compilation and Windows resources. |

## Music Bot

| Library | Used For |
| --- | --- |
| `server-plugin` | Plugin ABI and host wrappers. |
| `reqwest` | SoundCloud HTTP calls. |
| `serde`, `serde_json` | SoundCloud/API JSON data. |
| `symphonia` | AAC/MP4 audio decode support. |
| `minimp3` | MP3 decode support. |
| `opus` | Bot voice output encoding. |

## Server Plugin

The `server-plugin` crate uses only the Rust standard library. It defines ABI-safe types and helper wrappers without pulling in extra dependencies.
