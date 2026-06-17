# Music Bot Playback

Playback is voice-channel scoped. A command caller must be in a voice channel so the bot can join the correct place and send audio.

## Queue Behavior

- `/play` resolves one or more tracks and appends them.
- Playlists are summarized to avoid flooding chat.
- `/queue` reports queued tracks.
- `/nowplaying` reports current track and progress.
- `/stop` clears active playback.
- `/skip` advances to the next queued track.

## Sources

The current source registry supports SoundCloud URLs.

