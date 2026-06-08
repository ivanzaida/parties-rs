# Video Sharing Investigation

This document summarizes how video/screen sharing works in the existing C++ `parties` codebase and how much of that behavior is currently present in `parties-rs`.

Sources inspected:

- `F:\gavno\parties\common\include\parties\protocol.h`
- `F:\gavno\parties\common\include\parties\video_common.h`
- `F:\gavno\parties\server\src\server.cpp`
- `F:\gavno\parties\server\src\quic_server.cpp`
- `F:\gavno\parties\client\src\windows\app.cpp`
- `F:\gavno\parties\client\src\app_core.cpp`
- `F:\gavno\parties\client\include\client\lobby_model.h`
- `F:\gavno\parties-rs\src\network\protocol\*.rs`
- `F:\gavno\parties-rs\src\network\server.rs`
- `F:\gavno\parties-rs\src\session.rs`

## Mental Model

Video sharing is a voice-channel-scoped screen-share system.

- A user must be authenticated and inside a voice channel to share or watch.
- Multiple users may share in the same channel.
- Each viewer watches at most one sharer at a time.
- Opening/watching a share is explicit. Being in voice does not automatically subscribe to video.
- Screen-share video and screen-share audio are separate packet streams.

The server is not a video mixer. It tracks active sharers and forwards packets from one sharer to only those viewers whose `subscribed_sharer` matches that sharer.

## Wire Protocol

### Control Plane

Control messages are reliable, length-prefixed messages on the control stream.

Relevant control types:

| Direction | Message | ID | Payload |
| --- | --- | ---: | --- |
| C2S | `SCREEN_SHARE_START` | `0x0007` | `codec(1), width(2), height(2)` |
| C2S | `SCREEN_SHARE_STOP` | `0x0008` | empty |
| C2S | `SCREEN_SHARE_VIEW` | `0x0009` | `target_user_id(4)`, `0` means unsubscribe |
| C2S | `SCREEN_SHARE_UPDATE` | `0x000A` | `codec(1), width(2), height(2)` |
| S2C | `SCREEN_SHARE_STARTED` | `0x010A` | `sharer_user_id(4), codec(1), width(2), height(2)` |
| S2C | `SCREEN_SHARE_STOPPED` | `0x010B` | `sharer_user_id(4)` |
| S2C | `SCREEN_SHARE_DENIED` | `0x010C` | `reason(string)` |

Codec IDs:

| Codec | ID |
| --- | ---: |
| AV1 | `0x01` |
| H.265 | `0x02` |
| H.264 | `0x03` |

### Data Plane

Data packet type is the first byte:

| Packet | Type | Transport in C++ client |
| --- | ---: | --- |
| Voice | `0x01` | QUIC datagram |
| Video frame | `0x02` | Dedicated bidirectional video stream |
| Video control | `0x03` | Video stream or datagram |
| Stream audio | `0x04` | QUIC datagram |

Video frames are length-prefixed on the dedicated video stream:

```text
stream frame:
  frame_len: u32 little-endian
  packet_type: u8 = 0x02
  frame_number: u32
  timestamp: u32
  flags: u8
  width: u16
  height: u16
  codec: u8
  encoded_video_bytes...
```

When the server forwards a video frame to a viewer, it prepends the sender ID:

```text
forwarded video:
  packet_type: u8 = 0x02
  sender_user_id: u32
  frame_number: u32
  timestamp: u32
  flags: u8
  width: u16
  height: u16
  codec: u8
  encoded_video_bytes...
```

`flags & 0x01` means keyframe.

PLI/keyframe requests use video control:

```text
video control:
  packet_type: u8 = 0x03
  subtype: u8 = 0x01  // PLI
  target_or_requester_user_id: u32
```

Stream audio is Opus stereo audio:

```text
outbound stream audio:
  packet_type: u8 = 0x04
  opus_bytes...

forwarded stream audio:
  packet_type: u8 = 0x04
  sender_user_id: u32
  opus_bytes...
```

## Server Behavior

The C++ server stores screen-share state in:

```text
channel_screen_sharers_: channel_id -> set<user_id>
Session.share_codec
Session.share_width
Session.share_height
Session.subscribed_sharer
```

### Start

`SCREEN_SHARE_START` is accepted only when the sender is authenticated and in a channel.

The server:

1. Adds the user to `channel_screen_sharers_[channel_id]`.
2. Stores codec/width/height on the session.
3. Sends `SCREEN_SHARE_STARTED` to every authenticated user in that channel, including the sharer.

The Windows C++ client initially sends `SCREEN_SHARE_START` with `codec=0,width=0,height=0`, then sends `SCREEN_SHARE_UPDATE` after the encoder initializes with real codec and dimensions. Existing C++ UI does not depend on the metadata for rendering sharer cards.

Important caveat: `SCREEN_SHARE_UPDATE` updates server-side session metadata, but the inspected C++ server does not broadcast an update message to existing clients. Late joiners receive current metadata because the server uses session metadata when notifying a newly joined user about already-active sharers.

### Late Join

When a user joins a voice channel, the server sends normal channel user state and then sends `SCREEN_SHARE_STARTED` for each active sharer already in that channel.

### Watch

`SCREEN_SHARE_VIEW(target_user_id)` validates that the target is an active sharer in the viewer's current channel.

If valid:

1. Server sets `viewer_session.subscribed_sharer = target_user_id`.
2. Server sends an automatic PLI to the sharer so the viewer gets a fresh keyframe.

`SCREEN_SHARE_VIEW(0)` clears the subscription.

### Forward Video

For each incoming video frame:

1. Server verifies sender is authenticated, in a channel, and currently an active sharer.
2. Server prepends the sender user ID.
3. Server forwards only to sessions in the same channel where `subscribed_sharer == sender_user_id`.

The server bypasses the normal polling loop for video frames and forwards them from the QUIC receive path to reduce latency.

### Forward Stream Audio

Stream audio follows the same subscription rule as video:

- Sender must be an active sharer.
- Recipients must be in the same channel and subscribed to that sharer.
- Server prepends sender user ID and forwards as datagrams.

### Stop

`SCREEN_SHARE_STOP`, channel leave, disconnect, or captured target loss stops the share.

The server:

1. Removes the sharer from `channel_screen_sharers_`.
2. Clears any viewer subscriptions pointing to that user.
3. Sends `SCREEN_SHARE_STOPPED` to users in the channel.

## C++ Client Flow

### Start Sharing

On Windows:

1. User clicks share.
2. App opens a share picker.
3. `ScreenCapture` enumerates monitors and windows.
4. User selects a target.
5. Capture starts with selected FPS.
6. Encoder thread starts.
7. Optional scaling is applied before encoding.
8. Encoded frames are wrapped in the video packet header and sent on the video stream.
9. System/process loopback audio is captured, Opus-encoded, and sent as `STREAM_AUDIO_PACKET_TYPE` datagrams.
10. Client sends `SCREEN_SHARE_START`.
11. Once encoder is initialized, client sends `SCREEN_SHARE_UPDATE` with actual codec and dimensions.

Settings:

- Codec: AV1, H.265, H.264
- Scale: source, 75%, 50%, 25%
- FPS: 15, 30, 60, 120
- Bitrate: 0.5-20 Mbps in UI, clamped by common video limits

### Watch Sharing

The C++ UI model has:

```text
someone_sharing
sharers: Vec<ActiveSharer>
viewing_sharer_id
stream_volume
stream_fullscreen
stream_fps
```

When `SCREEN_SHARE_STARTED` arrives:

1. Client resolves sharer name from current channel users.
2. Adds/updates `model.sharers`.
3. Sets `someone_sharing`.
4. Marks that user as `streaming` in the channel user list.

If there are sharers but `viewing_sharer_id == 0`, the UI shows sharer cards with a `Watch` action.

When the user watches a sharer:

1. Existing watched sharer is stopped if different.
2. `viewing_sharer_` is set.
3. `awaiting_keyframe_ = true`.
4. Decode thread starts before requesting the stream.
5. Client sends `SCREEN_SHARE_VIEW(target_user_id)`.
6. Client sends a PLI.
7. UI sets `viewing_sharer_id`.

Incoming video frames are ignored unless `sender_id == viewing_sharer_`. Non-keyframes are ignored while awaiting a keyframe.

Decoded frames are copied into shared planes and rendered through a custom RmlUi `video_frame` element. The renderer uploads those planes as a GPU texture.

### Stop Watching

Stop watching:

1. Stops decode thread.
2. Clears `viewing_sharer_`.
3. Sends `SCREEN_SHARE_VIEW(0)`.
4. Clears the video element.

### Stop Sharing

Stop sharing:

1. Stops self-watch if watching own stream.
2. Stops stream-audio capture.
3. Stops screen capture.
4. Stops encode thread.
5. Releases encoder and textures.
6. Sets `model.is_sharing = false`.
7. Sends `SCREEN_SHARE_STOP`.

## Current `parties-rs` Status

The Rust client currently has partial protocol support:

- Control message IDs for screen sharing are present.
- `C2S` can encode `ScreenShareStart`, `ScreenShareStop`, `ScreenShareView`, and `ScreenShareUpdate`.
- `S2C` can decode `ScreenShareStarted`, `ScreenShareStopped`, and `ScreenShareDenied`.
- Data packet structs exist for `VideoFrame`, `ForwardedVideoFrame`, `VideoControl`, `StreamAudioPacket`, and `ForwardedStreamAudioPacket`.
- `Server` exposes methods for start/stop/view/update, PLI, and video control datagrams.
- `Session` tracks `LobbyScreenShare { sharer_user_id, metadata }`, clears it on stop, and has `watching_user_id`.
- The lobby rail can mark users as streaming based on `lobby.screen_shares`.

But the Rust client does not yet have a complete video runtime:

- No screen capture service exists.
- No encoder/decoder service exists.
- No stream-audio capture/player exists.
- The dedicated video stream is retained and has length-prefixed send/receive helpers.
- `watching_user_id` is wired to the stream subscription UI, but there is no decode/render runtime yet.
- The stream browser can show active sharers and send start/stop/watch control messages.

## Implications For The Rust UI

Keep these rules:

- Zero streams: show no-stream browser empty state.
- One or more streams: show stream discovery/watch targets, not the old voice member-list detail screen.
- Watching a stream should select exactly one sharer.
- Switching streams should replace the current watched stream.
- Starting stream browsing should not automatically watch any stream.
- If a watched stream stops, clear `watching_user_id` and return to stream discovery/empty state.

The Rust `screen_shares` list already has enough information for stream counts and discovery cards, assuming user names are resolved from `users_by_channel`.

## Rust Implementation TODO

To make video sharing functional in `parties-rs`:

1. Keep video stream handles in `network::Server` instead of discarding them. Done.
2. Implement length-prefixed video stream send/receive matching the C++ transport. Done.
   - outbound: `[u32 len][0x02][frame payload]`
   - inbound: parse `[u32 len]` frames and decode forwarded packet type.
3. Decide whether Rust sends PLI over datagram or video stream. Current Rust watcher action sends PLI as a datagram; the stream helper also exists.
4. Add watcher actions. Control-plane wiring is started; decode/render runtime remains.
   - `view_screen_share(target_user_id)`
   - set local `watching_user_id`
   - start receive/decode loop
   - request keyframe
5. Add share actions. Control-plane start/stop is wired; capture/encode remains.
   - capture target picker
   - capture frames
   - encode frames
   - `start_screen_share`
   - `update_screen_share` after encoder init
   - send video frames on video stream
   - `stop_screen_share`
6. Add stream audio if desired:
   - capture loopback/process audio
   - Opus encode/decode
   - send/receive packet type `0x04`
   - mix into output with separate stream volume.
7. Handle metadata carefully:
   - Existing viewers may see `0x0` metadata if start is sent before encoder init.
   - Decode should trust dimensions/codec from each video frame header.
   - Stream cards should not require metadata unless the server starts broadcasting updates.
8. Add cleanup:
   - leave channel clears watched stream
   - disconnect stops local sharing/watching
   - `SCREEN_SHARE_STOPPED` clears watched stream if it was the active target
   - decoder waits for a keyframe after subscribe or codec/context reset.

## Key Differences Between Legacy C++ And Current Rust

| Area | C++ `parties` | Rust `parties-rs` |
| --- | --- | --- |
| Control protocol | implemented | implemented |
| Video packet codecs | implemented | structs implemented |
| Dedicated video stream | implemented and used | opened then discarded |
| Screen capture | Windows/macOS/iOS code exists | not implemented |
| Encoding | AV1/H.265/H.264 through native/hardware backends | not implemented |
| Decoding/rendering | native decoder + custom video element | not implemented |
| Stream audio | implemented | not implemented |
| UI discovery | sharer cards/tabs | empty/browser UI in progress |
| Watch subscription | implemented | server API exists, UI/runtime not wired |
