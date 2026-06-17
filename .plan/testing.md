# Client Stability Testing Plan

Date: 2026-06-17

Scope: `crates/client` only. `server-plugin` and `music-bot` are intentionally out of scope for this plan.

This plan is for bringing the desktop client to a predictable stability baseline. It is broader than `docs/development/testing.md`, which is a command reference. This file defines what client behavior deserves coverage, what is currently missing, and what should block release.

## Current State

- Client Rust files: about 111.
- Source unit-test bodies are stored under `crates/client/tests/unit` by source-domain path and included back into source modules with `#[path = ...]` hooks.
- Client test attributes found directly in `crates/client/src`: 0.
- Reachable client unit test target: `cargo test -p client --lib`.
- Reachable client integration test target: `cargo test -p client --test chat`.
- Current verified client unit coverage: `cargo test -p client --lib` runs 267 tests.
- Current verified chat integration coverage: `cargo test -p client --test chat` runs 10 tests.
- Verified on 2026-06-17:
  - `cargo fmt --all --check` passes.
  - `cargo clippy -p client --all-targets` passes with existing warnings.
  - `cargo check -p client` passes.
  - `cargo test -p client --lib` passes with 267 tests.
  - `cargo test --target-dir target\client-test -p client --test chat` passes with 10 tests.
  - `cargo check --config .cargo/local-lurq.toml -p client` passes and uses local `F:\gavno\lurq`.
- Existing effective coverage is strongest around pure logic that has been explicitly pulled into integration tests or can be compiled independently:
  - chat history merge and scroll policy
  - protocol helper logic where reachable
  - storage/identity/audio/video helper tests if made reachable
- Existing coverage is weak or absent for full app workflows:
  - identity setup to server selection to lobby
  - saved server and TOFU flows
  - real QUIC session behavior under disconnect/reconnect
  - live CPAL device changes
  - full voice-channel join/leave/switch workflows beyond pure state transitions
  - outgoing intro sound device/runtime behavior beyond pure state and mixer coverage
  - screen-share start/stop while watching another stream
  - DX12 device-loss recovery
  - multi-GPU adapter selection
  - modal overlay and hit-testing regressions
  - release-package smoke tests
- Local `lurq` validation must prove the local patch is actually used. If Cargo prints `Patch 'lurq ...' was not used in the crate graph`, the check is invalid.

## Stability Goals

1. Make crashes, freezes, and disconnect loops reproducible before fixing them.
2. Prevent high-frequency UI invalidation from audio/video hot paths.
3. Verify that every user-visible state transition has a deterministic owner.
4. Cover GPU/audio/device-specific behavior with targeted manual and automated tests.
5. Make release builds prove the same stability expectations as debug builds.

## Required Client Gates

These commands should be the minimum local client gate:

```powershell
cargo fmt --all --check
cargo clippy -p client --all-targets
cargo check -p client
cargo test -p client --lib
cargo test -p client --test chat
cargo check --config .cargo/local-lurq.toml -p client
```

Windows-specific notes:

- Close `target\debug\parties-rs.exe` before running tests that relink the binary.
- If the app must stay open, run integration tests with a separate target dir, for example `cargo test --target-dir target\client-test -p client --test chat`.
- Verify MSVC Build Tools and `link.exe` are available before accepting Windows build results.
- Run GPU/video smoke tests on both default GPU and forced GPU preference when testing hybrid systems.

## P0 Test Infrastructure Work

### Keep Client Unit Tests Reachable

`crates/client/src/lib.rs` exists to compile the client modules as a library test harness while leaving the shipped app binary as `parties-rs`. Keep `cargo test -p client --lib` in every local and CI gate. Any new pure logic in `crates/client/src` should have tests that run through that target.

### Add A Fake Session/Server Harness

The client needs an in-process fake server or protocol-level harness that can drive `ServerSession` without a real public server. Required capabilities:

- query response success/failure/timeout;
- auth success/failure;
- TOFU fingerprint match/change;
- channel list and user join/leave/move events;
- chat history and live chat events;
- voice state events;
- stream start/stop events;
- abrupt transport close and reconnect.

Assertions:

- disconnect marks lobby disconnected exactly once;
- reconnect does not double-start lobby, voice, or video receivers;
- voice engine stops on local leave/kick/disconnect;
- pending watched stream is restored only when the target exists;
- chat history requests are not duplicated across reconnect loops.

### Add A Fake Audio Backend Boundary

Real CPAL behavior needs manual testing, but most state logic can be tested with fake input/output streams.

Required fake-backend cases:

- input device missing;
- output device missing;
- both devices missing;
- input callback panic;
- output callback cannot lock mixer;
- input device change while in channel;
- output device change while receiving audio;
- outgoing intro sound while active voice engine exists;
- notification settings update during a session.

## Automated Coverage Plan

### Identity, Startup, And Storage

Add tests for:

- Generate identity, restore seed phrase, import private key, and reject malformed input.
- Delete identity redirects to identity/start flow and clears connected session state.
- Legacy config import:
  - valid `emcifuntik/parties` database imports identity and servers;
  - wrong file type shows a user-facing error;
  - missing expected tables shows a user-facing error;
  - duplicate servers merge deterministically.
- App settings round trip:
  - display name;
  - sentry reporting;
  - audio input/output devices;
  - notification sounds;
  - outgoing intro sound;
  - voice activation;
  - push-to-talk;
  - mute/deafen hotkeys;
  - video hardware decoding;
  - stream codec/FPS/bitrate;
  - locale.
- SQLite migration from older schemas.
- `lurq.redb` window state fallback from SQLite and invalid off-screen window recovery.

### Server, Network, And TOFU

Add tests around session/network state machines:

- Query server success, timeout, malformed response.
- Auth success and auth failure.
- Saved credentials metadata after auth.
- TOFU first trust, trusted reconnect, certificate mismatch warning, and user accept/reject.
- Disconnect from:
  - client request;
  - lobby reader error;
  - voice stream error;
  - video stream error;
  - server kick.
- Reconnect:
  - preserves selected server;
  - restores text channel history;
  - rejoins previous voice channel only when expected;
  - does not duplicate receiver threads;
  - does not replay stale stream watch state after explicit leave.
- Channel user list:
  - local join;
  - local leave;
  - remote join;
  - remote leave;
  - local moved between channels;
  - remote moved between channels.

### Chat

Expand the existing `cargo test -p client --test chat` target:

- Initial history load for multiple channels.
- Pagination before oldest message.
- Deduplication when live messages overlap with history.
- Scroll pin behavior when new messages arrive.
- User scroll detach/reattach behavior.
- Mention notification routing.
- Chat command parsing and server-provided command metadata.
- Attachment rendering failure paths.

### Voice And Audio

Automated tests should cover state logic without real devices:

- Speaking tracker:
  - repeated voice packets do not cause repeated UI revisions;
  - `speaking = packet activity OR intro activity`;
  - intro activity resets on leave;
  - intro activity resets on channel switch;
  - stale intro stop from old channel cannot clear new intro.
- Voice join sound:
  - custom sound absent means no outgoing intro;
  - invalid custom sound returns an error;
  - sound is truncated to max duration;
  - fade and volume are applied;
  - local intro playback is queued through the voice mixer;
  - normal mic frames are suppressed while forced intro frames are active.
- Mixer:
  - voice, stream, and local notification streams mix without allocation growth;
  - deafen excludes voice but keeps stream/local notification where intended;
  - per-user volume and stream volume are applied;
  - stream queues are bounded.
- Capture/send:
  - mute/deafen stops transmit;
  - push-to-talk active/inactive behavior;
  - voice activation threshold and hold behavior;
  - voice normalization target behavior;
  - echo cancellation lock contention does not block capture indefinitely.

Manual device tests:

- Join voice channel with default input/output.
- Join with missing input device.
- Join with missing output device.
- Change input device while in a voice channel.
- Change output device while receiving audio.
- Toggle mute/deafen while intro sound is playing.
- Join channel, immediately switch channel, verify green speaking ring resets and new intro owns state.
- Leave channel while intro sound is playing, verify speaking ring clears.
- Push-to-talk while intro sound is playing.
- Echo cancellation on/off.
- Noise cancellation on/off.
- Custom MP3 notification and outgoing intro sounds.
- Invalid custom MP3 error path.

Performance acceptance:

- Joining a channel with outgoing intro sound must not drop app FPS below 45 on the target Windows machine.
- Voice receive with four active speakers must not cause visible UI stutter.
- Audio callbacks must not allocate per sample or hold UI/session locks.

### Video, Streaming, And GPU

Automated tests should cover pure decisions:

- Capture source filtering for monitors, windows, and webcams.
- Output size and aspect-ratio calculations.
- Codec preference/fallback selection.
- Bitrate/FPS setting validation.
- Stream state transitions:
  - local start;
  - local stop;
  - remote start;
  - remote stop;
  - watched stream changed;
  - watched stream ended;
  - reconnect while watching.
- Decoder pool reuse and replacement on codec/dimension change.
- Video sink retain/clear semantics.

Manual Windows GPU matrix:

- Default GPU selected by Windows.
- Force app to NVIDIA, verify DX12 renderer, capture, NVENC, NVDEC logs select NVIDIA.
- Force app to AMD/iGPU, verify renderer and video paths either use AMD correctly or fall back cleanly.
- Start a stream while watching another stream.
- Stop watching while start-stream modal is open.
- Select screen source while already watching a stream.
- Start stream with audio enabled and disabled.
- Start webcam stream.
- Switch watched stream users repeatedly.
- Toggle hardware decoding at runtime and after restart.
- Exercise DX12 device-loss path:
  - display sleep/wake;
  - GPU driver reset if available;
  - display mode change;
  - unplug/replug monitor;
  - start/stop screen share while renderer is under load.

Acceptance:

- DX12 `DXGI_ERROR_DEVICE_REMOVED`, `DXGI_ERROR_DEVICE_HUNG`, and swapchain present failures must reset rendering or fall back without freezing the UI thread.
- Selecting stream sources must not disconnect lobby/voice transport.
- Stream modal overlay must cover the full app content and must not block unrelated clicks after closing.
- Watching a stream must not crash if decoder init fails; it should show a recoverable stream error.

### UI And Routing

Create a UI smoke suite that drives the app through the main routes and captures screenshots:

1. `1.startup.png`
2. `2.generate-identity.png`
3. `3.server-selection.png`
4. `4.lobby.png`
5. `5.settings.png`
6. `6.stream.png`
7. `7.share-screen.png`

Each screenshot should assert:

- No blank root content.
- No text overflow in compact controls.
- No double border between system chrome and app chrome.
- Modal overlays cover the intended area.
- Main clickable elements still receive clicks after modal close.
- Key fingerprint and long server/fingerprint strings wrap or truncate intentionally.
- TOFU warning route is reachable and visible.

UI flows that deserve automated or scripted coverage:

- First launch with no identity.
- Generate identity.
- Import identity.
- Import legacy config.
- Delete identity.
- Connect to server.
- Open settings from server selection and lobby.
- Save audio/notification settings and observe live update.
- Open start-stream modal, select source, close modal.
- Open stream browser, watch stream, stop watching.

### Hotkeys

Automated tests:

- Key parsing for supported modifiers.
- Toggle mute/deafen release behavior.
- Push-to-talk press/release behavior.
- Duplicate hotkey handling.
- Disabled local hotkeys while settings are focused.

Manual tests:

- App-focused local hotkeys.
- App-unfocused global hotkeys.
- Mouse hotkeys when settings are open and closed.
- Hotkeys during reconnect and while no server is connected.

### Updates, Logging, And Diagnostics

Automated tests:

- Update state persistence.
- Resume state after staged update.
- Logger file rotation and retention.
- Sentry before-send filtering.
- Windows SEH diagnostic formatting.
- Startup GPU adapter log parsing.

Manual release smoke:

- Windows debug symbols are produced and uploaded.
- macOS app signs, notarizes, and launches.
- Sparkle appcast validates and contains the expected version.
- App logs include startup GPU, server connection, voice, video, and Sentry state.

## Manual Stability Runs

### Short Smoke Run

Use before every risky merge:

1. Launch app from `cargo run -p client`.
2. Verify startup route.
3. Connect to a test server.
4. Join voice.
5. Verify intro sound, speaking ring, and FPS.
6. Switch voice channel.
7. Leave voice.
8. Start stream modal, close it.
9. Start screen share, watch from another client, stop share.
10. Disconnect and reconnect.

Pass criteria:

- No panic.
- No UI freeze longer than one second.
- No sustained FPS below 45 outside known GPU encode spikes.
- No duplicate receiver threads in logs.
- No stale speaking state after leave/switch.

### Long Soak Run

Use nightly or before release:

- 60 minutes connected to server.
- 30 minutes in a voice channel with intermittent speakers.
- 20 join/leave cycles.
- 20 channel-switch cycles.
- 10 input-device changes.
- 10 output-device changes.
- 10 stream start/stop cycles.
- 10 watch/unwatch cycles.
- 5 reconnect cycles by killing network or server.

Capture:

- app log;
- Sentry event IDs;
- FPS profile;
- CPU/GPU usage;
- memory usage;
- network disconnect timestamps.

Fail criteria:

- Crash or access violation.
- UI stops repainting.
- Voice engine cannot recover after device change.
- Renderer never recovers from DX12 device loss.
- Lobby disconnects during local-only modal/source selection.
- Memory grows without returning after repeated stream cycles.

## Client CI Plan

Add required client CI jobs:

1. `cargo fmt --all --check`
2. `cargo clippy -p client --all-targets`
3. `cargo check -p client`
4. `cargo test -p client --test chat`
5. `cargo check --config .cargo/local-lurq.toml -p client`
6. Windows debug client build
7. macOS debug client build

Add after testable library/harness work:

1. `cargo test -p client`
2. Session fake-server integration tests
3. Voice fake-audio integration tests
4. UI route screenshot smoke tests

Optional/manual CI:

- Windows release build with PDB validation.
- macOS signed/notarized package validation.
- Hardware lab job for GPU/video matrix, if a self-hosted Windows machine is available.

## Instrumentation Needed

Add or verify logs/counters for:

- UI revision rate.
- FPS minimum/average around voice join and stream start.
- Voice engine lifecycle:
  - start;
  - stop;
  - input device change;
  - output device change;
  - outgoing intro start/stop/cancel.
- Speaking state transitions with reason: packet, intro, timeout, reset.
- CPAL callback underflow/overflow if available.
- Video encoder backend selected.
- Video decoder backend selected.
- DX12 device-loss reason and recovery action.
- Active receiver thread counts.
- Reconnect attempt IDs.

These counters should be queryable from debug UI or dumped with a single diagnostic action.

## Priority Order

1. Add client CI gates for format, clippy, check, lib tests, chat tests, and local `lurq` check.
2. Add tests for speaking/intro/channel-switch behavior.
3. Add fake-session tests for lobby join/leave/reconnect.
4. Add fake-audio tests for device change and intro playback behavior.
5. Add scripted UI route smoke with screenshots.
6. Add video/GPU manual matrix and logs-based acceptance checks.
7. Add long soak run before release.

## Release Blocking Checklist

A client release should not ship unless:

- `cargo fmt --all --check` passes.
- `cargo clippy -p client --all-targets` passes or has approved exceptions.
- `cargo test -p client --lib` passes.
- `cargo test -p client --test chat` passes.
- client builds on Windows x64 and macOS arm64.
- Windows package contains PDBs.
- macOS package signs and notarizes.
- Smoke run passes on Windows.
- Smoke run passes on macOS.
- Voice join/leave/switch does not drop FPS or leave stale speaking state.
- Stream start/watch/stop does not crash or disconnect.
- TOFU warning is visible and actionable.
- Identity delete returns to the identity/start flow.
- Logs from the smoke run contain no unhandled native exception or repeated receiver restart loop.
