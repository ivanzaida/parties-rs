# Build

## Client Debug Build

```powershell
cargo build --package client
```

## Client Release Build

```powershell
cargo build --release --target x86_64-pc-windows-msvc --package client
```

To generate Windows PDBs:

```powershell
$env:CARGO_PROFILE_RELEASE_DEBUG = "2"
cargo build --release --target x86_64-pc-windows-msvc --package client
```

## Music Bot Release Build

```powershell
cargo build --release --package music-bot --lib
```

## Native Build Inputs

Windows client builds compile C/C++ files for:

- AMD AMF.
- NVIDIA NVENC/NVDEC.
- Media Foundation decode.
- Windows capture and bridge code.
- libhevc bridge.

