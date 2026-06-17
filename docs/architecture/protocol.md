# Protocol

Client protocol code lives in `crates/client/src/network/protocol`.

## Modules

| Module | Purpose |
| --- | --- |
| `mod.rs` | Shared IDs, roles, errors, video codec IDs, and base frame definitions. |
| `codec.rs` | Binary reader/writer helpers. |
| `c2s.rs` | Client-to-server control messages. |
| `s2c.rs` | Server-to-client control messages. |
| `control.rs` | Payload structures for channels, users, chat, screen share metadata, admin results, and commands. |
| `data.rs` | Realtime data packet shapes. |
| `permissions.rs` | Role and permission matrix. |

## Roles And Permissions

Roles are:

- Owner
- Admin
- Moderator
- User

Permissions include joining/speaking in channels, moderation actions, channel management, server management, text chat, uploads, screen sharing, and webcam sharing.

## Chat Commands

Servers can publish chat command definitions to clients. The UI can display and submit these commands, while plugins can register command definitions through the server plugin ABI.

## Codec IDs

The stream protocol supports:

- AV1
- H.265 / HEVC
- H.264 / AVC

See [Video Encoder/Decoder Implementation](../video-codecs.md).

