# Parties Documentation

Parties is a Rust workspace for the `parties-rs` desktop client and server-side plugin tooling. The repo currently contains:

- `crates/client`: the desktop app binary, built as `parties-rs`.
- `crates/server-plugin`: Rust types and ABI helpers for Parties server plugins.
- `crates/music-bot`: a server plugin that adds SoundCloud-backed music playback.

## Start Here

- [Overview](overview.md)
- [Quickstart](quickstart.md)
- [Capabilities](capabilities.md)
- [Workspace Architecture](architecture/workspace.md)
- [Used Libraries](reference/used-libraries.md)

## Operations

- [Build](development/build.md)
- [Release Pipeline](development/release-pipeline.md)
- [Sentry](development/sentry.md)
- [CI Secrets](reference/ci-secrets.md)
- [Environment Variables](reference/environment-variables.md)

## Feature Areas

- [Client Architecture](architecture/client.md)
- [Storage](architecture/storage.md)
- [Client Install](client/install.md)
- [Identity And Servers](client/identity-and-servers.md)
- [Settings](client/settings.md)
- [Audio](architecture/audio.md)
- [Video](architecture/video.md)
- [Networking](architecture/networking.md)
- [Protocol](architecture/protocol.md)
- [Plugins](architecture/plugins.md)
- [Music Bot](music-bot/overview.md)
- [Music Bot Sources](music-bot/supported-sources.md)
