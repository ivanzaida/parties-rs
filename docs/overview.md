# Overview

Parties is a native Rust voice, chat, and streaming client with plugin support for server-side extensions. The client package is named `client`, but its shipped binary and release artifacts are still named `parties-rs`.

The workspace is intentionally split into three crates:

- `client`: the app UI, networking, audio/video, identity, storage, updates, diagnostics, and release integration.
- `server-plugin`: shared ABI and safe Rust helpers for server plugins.
- `music-bot`: a plugin built on the server-plugin API.

## Main Capabilities

- Identity setup and restoration with seed phrases and Ed25519 keys.
- Server selection, connection, trust-on-first-use warnings, and reconnect handling.
- Text chat, chat history, attachments, and server-provided chat commands.
- Voice channel participation with mute, deafen, push-to-talk, device selection, and notification sounds.
- Screen, window, and webcam streaming.
- Hardware video encode/decode on Windows and macOS.
- Settings UI for identity, servers, audio, notifications, and stream options.
- Local persistent settings and saved servers through SQLite.
- Update checks, macOS Sparkle appcast generation, and release artifacts.
- Sentry crash reporting and Windows PDB upload in CI.
- Server plugin ABI plus a SoundCloud music bot plugin.

## Supported Platforms

The client is built for Windows x64 and macOS arm64 in CI.

Windows support includes custom window chrome, DX12/WGPU rendering, CPAL audio, Media Foundation, NVIDIA/AMD video paths, PDB debug symbols, and Sentry debug file upload.

macOS support includes custom titlebar behavior, app bundling, signing, DMG creation, notarization, Sparkle appcast generation, AVFoundation/ScreenCaptureKit/VideoToolbox integration, and optional software AV1 decode fallback.

