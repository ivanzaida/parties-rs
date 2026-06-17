# Quickstart

## Prerequisites

- Rust stable with edition 2024 support.
- Windows or macOS for the client runtime paths.
- On Linux, the music bot CI path installs `pkg-config` and `libopus-dev`.

## Build The Client

```powershell
cargo build --package client
```

The client binary is:

```text
target/debug/parties-rs.exe
```

For a Windows release build with PDBs:

```powershell
$env:CARGO_PROFILE_RELEASE_DEBUG = "2"
cargo build --release --target x86_64-pc-windows-msvc --package client
```

## Run Tests

```powershell
cargo test --workspace
```

For the focused chat tests:

```powershell
cargo test -p client --test chat
```

## Build The Music Bot

```powershell
cargo build --release --package music-bot --lib
```

The Linux release artifact is built in CI as `libmusic_bot.so` and packaged with `plugin.toml`.

