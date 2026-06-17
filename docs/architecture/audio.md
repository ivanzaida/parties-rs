# Audio

Audio is split between voice chat, stream audio, notification playback, and music-bot audio output.

## Client Voice

The client uses:

- `cpal` for input/output devices.
- `opus` for voice encoding/decoding.
- `rdev` for global input hooks used by hotkeys.
- Local settings for mute, deafen, voice activation, push-to-talk, normalization, and echo cancellation.

`PARTIES_AEC_DELAY_MS` can tune echo-cancellation delay.

## Notification Sounds

Notification audio supports built-in and custom MP3 sounds. `minimp3` decodes MP3 data. Runtime notification settings are pushed into the session voice state so playback can reflect user settings immediately.

## Music Bot Audio

The music bot decodes remote audio sources and sends Opus audio through the server plugin host. It uses `symphonia`, `minimp3`, and `opus`.

