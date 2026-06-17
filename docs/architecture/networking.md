# Networking

The client networking layer uses QUIC through `quinn`, with TLS handled by `rustls`. The code is split into a transport wrapper in `src/network/server.rs` and protocol encoders/decoders in `src/network/protocol`.

## Streams And Frames

The protocol distinguishes control messages from realtime audio/video packets. Control traffic carries authentication, server state, channel lists, voice state, chat messages, role changes, screen share metadata, and chat command definitions.

Realtime media packets are handled by the session voice and video runtimes after network receive.

## Reconnect

Session code owns reconnect behavior. Pending reconnect state is restored with the current app settings so voice and stream playback can resume when possible.

## Server Query

`src/network/server_query.rs` provides lightweight server discovery/query information separate from an authenticated session.

