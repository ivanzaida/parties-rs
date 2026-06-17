# Artifact Layout

## Client Windows

Binary artifact:

```text
parties-rs-<version>-windows-x64.zip
```

Debug symbols artifact:

```text
parties-rs-<version>-windows-x64-debug-symbols.zip
```

The release job excludes debug-symbol ZIPs from public GitHub release assets after uploading symbols to Sentry.

## Client macOS

Binary artifact:

```text
parties-rs-<version>-macos-arm64.dmg
```

Sparkle appcast artifact:

```text
appcast.xml
```

## Music Bot

Plugin artifact:

```text
parties.music_bot-<version>-linux-x64.tar.gz
```

Contents:

```text
libmusic_bot.so
plugin.toml
```

