# Windows Development Setup

Windows is the primary local development environment for this repo.

## Build

```powershell
cargo build --package client
```

## Release With PDBs

```powershell
$env:CARGO_PROFILE_RELEASE_DEBUG = "2"
cargo build --release --target x86_64-pc-windows-msvc --package client
```

## Notes

- Use PowerShell commands from the repo root.
- The shipped binary is `parties-rs.exe`.
- The Cargo package is `client`.
- Native Windows video bridge code is compiled by `crates/client/build.rs`.

