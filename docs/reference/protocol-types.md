# Protocol Types

This is a high-level reference. The authoritative definitions are in `crates/client/src/network/protocol`.

## IDs

- `UserId`
- `ChannelId`
- message IDs
- stream/session identifiers where applicable

## Roles

- Owner
- Admin
- Moderator
- User

## Control Areas

- Authentication.
- Channel list and channel users.
- User joined/left channel.
- Voice state.
- Role changes.
- Screen share started/stopped metadata.
- Admin result.
- Text channel info.
- Chat messages and history.
- Chat file upload response.
- Chat command list.

## Media

Video codec IDs:

- AV1
- H.265
- H.264

Realtime media packets are handled outside the normal chat/control UI path by the session voice and video runtimes.

