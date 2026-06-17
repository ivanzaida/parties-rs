# Chat

Chat support includes:

- Text messages.
- Text channels.
- Chat history loading.
- Attachments.
- Server-provided chat command definitions.
- Plugin-provided command definitions.

The session layer owns chat history and command state. Protocol payloads live in `network/protocol/control.rs`, and UI rendering lives under `ui/lobby/chat`.

## Scrolling

Chat scroll behavior is policy-tested under the `client` chat integration test target.

