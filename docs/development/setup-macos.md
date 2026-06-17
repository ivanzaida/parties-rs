# macOS Development Setup

macOS support is built in CI for arm64 through `macos-15`.

## Build

```bash
cargo build --package client
```

## Release Pipeline Responsibilities

The CI workflow handles the parts that normally require signing assets and Apple credentials:

- Download Sparkle.
- Build the `.app` bundle.
- Import Developer ID certificate.
- Sign nested Sparkle components and the app.
- Create and sign DMG.
- Notarize and staple DMG.
- Generate Sparkle appcast.

## Video Notes

macOS video uses Apple-native APIs and optional software AV1 fallback. See [Video](../architecture/video.md).

