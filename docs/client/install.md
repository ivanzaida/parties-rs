# Client Install

The release pipeline publishes platform-specific client artifacts.

## Windows

Download:

```text
parties-rs-<version>-windows-x64.zip
```

Extract and run:

```text
parties-rs.exe
```

Windows debug symbols are packaged separately for CI/Sentry symbol upload and are not intended as a normal user-facing artifact.

## macOS

Download:

```text
parties-rs-<version>-macos-arm64.dmg
```

The DMG is signed and notarized in CI. The app bundle includes Sparkle update metadata.

## From Source

```powershell
cargo build --package client
```

The debug binary is under `target/debug`.

