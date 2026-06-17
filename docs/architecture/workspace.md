# Workspace Architecture

The root `Cargo.toml` defines a three-crate workspace:

```text
crates/client
crates/music-bot
crates/server-plugin
```

`crates/client` is the default workspace member. Its Cargo package is named `client`, while the binary remains `parties-rs` through the explicit `[[bin]]` entry.

## Crate Responsibilities

| Crate | Responsibility |
| --- | --- |
| `client` | Desktop client app, protocol client, audio/video, UI, storage, updates, diagnostics. |
| `server-plugin` | ABI constants, manifest types, permissions, host wrappers, plugin registration macro. |
| `music-bot` | Server plugin using `server-plugin` to register commands, create bot users, and stream audio. |

## Cross-Crate Dependencies

`music-bot` depends on `server-plugin`. The client does not depend on either plugin crate. Protocol concepts such as roles, permissions, users, channels, and chat commands exist in both the client protocol code and the plugin ABI because they cross the network/plugin boundary in different formats.

