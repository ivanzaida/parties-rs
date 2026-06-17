# Release Pipeline

## Client Workflow

Workflow: `.github/workflows/build-client.yml`

The workflow:

1. Reads `crates/client/Cargo.toml`.
2. Builds only when the package name/version changes, on manual dispatch, or when the expected tag is missing.
3. Builds Windows x64 and macOS arm64.
4. Uploads Windows PDB debug files to Sentry.
5. Packages Windows ZIP and macOS DMG.
6. Signs and notarizes macOS artifacts.
7. Generates Sparkle appcast.
8. Creates or updates GitHub release `v<version>`.
9. Includes changelog entries from the previous `v*` tag.

## Music Bot Workflow

Workflow: `.github/workflows/build-music-bot.yml`

The workflow:

1. Reads `crates/music-bot/Cargo.toml`.
2. Builds Linux x64 plugin library.
3. Generates `plugin.toml`.
4. Packages `parties.music_bot-<version>-linux-x64.tar.gz`.
5. Creates or updates GitHub release `music-bot-v<version>`.
6. Includes changelog entries from the previous `music-bot-v*` tag.

