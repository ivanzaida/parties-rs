# Music Bot Setup

The plugin manifest template is `crates/music-bot/plugin.template.toml`.

Required variables:

```toml
[variables]
soundcloud_client_id = "env:SOUNDCLOUD_CLIENT_ID"
soundcloud_client_secret = "env:SOUNDCLOUD_CLIENT_SECRET"
```

The CI release packages:

- `libmusic_bot.so`
- generated `plugin.toml`

The generated manifest injects the package version from `crates/music-bot/Cargo.toml` and sets the library filename for the Linux artifact.

