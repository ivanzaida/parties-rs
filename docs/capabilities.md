# Capabilities

## Client

| Area | Capability |
| --- | --- |
| Identity | Create, restore, and persist local identity material. |
| Servers | Save servers, connect, reconnect, and show trust warnings. |
| Chat | Send text, receive history, show attachments, and expose server chat commands. |
| Voice | Join channels, mute, deafen, push-to-talk, play remote voice, and play notification sounds. |
| Audio settings | Select input/output devices, tune activation, normalization, echo cancellation, and notification volume. |
| Streaming | Share screen, windows, webcam, and stream audio. |
| Video | Encode/decode AV1, H.265, and H.264 through platform-native paths where supported. |
| Notifications | Play built-in or custom MP3 notification sounds. |
| Updates | Check releases, stage updates, and restart into a staged executable. |
| Diagnostics | Structured logging, debug counters, profiler hooks, Windows symbol path diagnostics, and Sentry crash reporting. |
| Localization | Resource-backed strings for supported locales. |

## Video Codec Matrix

| Platform | AV1 encode | H.265 encode | H.264 encode | AV1 decode | H.265 decode | H.264 decode |
| --- | --- | --- | --- | --- | --- | --- |
| Windows | Hardware | Hardware | Hardware | Hardware | Hardware | Hardware |
| macOS | VideoToolbox | VideoToolbox | VideoToolbox | VideoToolbox or opt-in rav1d fallback | VideoToolbox | VideoToolbox |

See [Video Encoder/Decoder Implementation](video-codecs.md) for the detailed current state.

## Plugin System

The plugin ABI supports:

- Reading sessions, users, channels, and chat.
- Moderating chat.
- Creating chat commands.
- Creating and controlling bot users.
- Sending bot chat.
- Joining bot voice channels.
- Sending bot audio.
- Live query/autocomplete for chat command inputs.

## Music Bot

The music bot supports:

- `/play {query:string...}` for SoundCloud search, tracks, and playlists.
- `/stop`
- `/skip`
- `/queue`
- `/nowplaying`
- Bot user creation and voice-channel-scoped playback.
- MP3/AAC decoding and Opus output for server voice injection.
