# Music Bot Overview

`crates/music-bot` is a Parties server plugin that registers music playback chat commands and streams audio into the caller's voice channel through a bot user.

## Commands

- `/play {query:string...}` searches SoundCloud or queues a SoundCloud URL.
- `/stop` stops playback and clears the queue.
- `/skip` skips the current track.
- `/queue` shows queued tracks.
- `/nowplaying` shows the current track.

## Audio Pipeline

The bot searches and resolves SoundCloud URLs, downloads audio, decodes MP3/AAC where supported, encodes voice audio with Opus, and sends it through the server plugin host.
