# Music Bot Supported Sources

The current music bot source registry supports SoundCloud URLs.

Supported forms include:

- `https://soundcloud.com/...`
- `http://soundcloud.com/...`
- `https://www.soundcloud.com/...`
- `http://www.soundcloud.com/...`

The plugin uses SoundCloud credentials from plugin variables:

```toml
soundcloud_client_id = "env:SOUNDCLOUD_CLIENT_ID"
soundcloud_client_secret = "env:SOUNDCLOUD_CLIENT_SECRET"
```

The `soundcloud-probe` binary can be used during development to inspect resolver behavior:

```powershell
cargo run --package music-bot --bin soundcloud-probe -- <soundcloud-url>
```

