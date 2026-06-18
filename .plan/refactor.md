# Store And Component Refactor Plan

Date: 2026-06-18

Scope: `crates/client`, focused on connected-session store ownership and `ui/lobby` component usage. This plan does not cover `music-bot`, `server-plugin`, or native video backend internals except where they touch lobby-visible state.

## Current State

The connected client has two overlapping state mechanisms:

- `ServerSession` owns the authoritative `Arc<Mutex<LobbyState>>` plus a `watch::Sender<LobbyState>` in `crates/client/src/session.rs`.
- `LobbyScreen` owns a `Store<LobbyState>`, copies `session.lobby()` into it during render, provides the store through context, and also mounts `LobbyStateSpy` to subscribe to `ServerSession::subscribe_lobby_updates()` in `crates/client/src/ui/lobby/mod.rs`.

The current `LobbyState` is a broad monolith in `crates/client/src/session/lobby.rs`. It contains voice channels, text channels, messages, unread state, chat history loading, command metadata, selected views, stream state, connection status, warnings, and reconnect flags. Every lobby update clones and compares this full structure.

The lobby component tree passes large snapshots downward:

- `LobbyScreen` passes a cloned `LobbyState` into `LobbyRail` and a borrowed `&LobbyState` into the main pane.
- `LobbyRail` clones `text_channels`, `unread_text_channel_ids`, `channels`, `users_by_channel`, and `screen_shares` into `TextChannelsProps` and `VoiceChannelsProps`.
- Chat, stream browser, watched stream, preview, and context-menu code all derive their own current-channel view from the full lobby snapshot.
- Several leaf components still pull `ServerSession`, `Storage`, or `Store<LobbyState>` from context while also receiving props, which makes data dependencies harder to see from the call site.

Optimistic UI mutations are split between session methods and the UI store:

- Joining voice calls `session.select_channel(channel_id)` and then `lobby_store.set(session.lobby())`.
- Leaving voice calls `session.leave_channel_locally()` and then `lobby_store.set(session.lobby())`.
- Server updates also publish through the watch channel, so UI and session can race to write equivalent snapshots.

## Problems To Solve

1. Single ownership: UI should not manually mirror authoritative session state or write a shadow copy.
2. Smaller invalidation: a chat message should not force voice rail props and stream cards to compare a whole `LobbyState`.
3. Clear component contracts: components should receive explicit view models and action handles, not arbitrary full-state access plus hidden contexts.
4. Stable optimistic updates: local channel selection, leave, watch, and chat-history loading should flow through one command path and one published revision.
5. Testability: selectors and reducers should be pure enough to test without mounting `lurq` components.

## Target Shape

Keep `ServerSession` as the high-level runtime boundary, but introduce a narrower UI-facing store API.

```text
network/server tasks
  -> ServerSession command/reducer methods
  -> SessionState / LobbyState domains
  -> published LobbySnapshot revision
  -> UI selectors / view models
  -> components
```

State should have one authoritative writer: `ServerSession`. `lurq::Store` may still be used inside the UI, but only as a local subscription cache owned by a small wrapper, not as a second writer that actions update directly.

## Proposed Modules

Add a small store/view-model layer before changing component rendering:

```text
crates/client/src/session/
  store.rs              # publish/subscribe revision helpers, snapshot metadata
  lobby.rs              # pure lobby reducer/domain state

crates/client/src/ui/lobby/
  model.rs              # UI view models and selectors from LobbyState/LobbySnapshot
  commands.rs           # UI action wrappers that call ServerSession only
```

Possible view models:

- `LobbyShellModel`: connection, selected pane, last error, debug mode.
- `RailModel`: server identity, local user row, connection status, text rail, voice rail.
- `TextRailModel`: channels, selected channel, unread ids.
- `VoiceRailModel`: channels with users, local user id/role, stream badges.
- `ChatPaneModel`: selected channel, messages, loading flags, paging flags, command registry.
- `StreamBrowserModel`: channel, users, streams, watched id, local streaming flag.
- `WatchedStreamModel`: watched user, stream metadata, switcher streams, error state.

Use cheap shared data where possible (`Arc<[T]>`, `Arc<str>`, `Arc<HashMap<...>>`, or small ID lists) so most props compare by generation or pointer identity instead of deep clones.

## Refactor Phases

### Phase 0: Baseline And Guardrails

- Run the existing gate before the refactor:
  - `cargo fmt --all --check`
  - `cargo check -p client`
  - `cargo test -p client --lib`
  - `cargo test -p client --test chat`
- Add focused tests around the current reducer behavior before moving files:
  - selected text channel survives valid channel-list refresh;
  - selected text channel falls back when removed;
  - voice user move updates only one channel cache;
  - watched stream clears when target leaves or stops streaming;
  - chat-history loading is not duplicated.
- Add a small test helper for building `LobbyState` fixtures. Current tests repeatedly construct large states by hand.

### Phase 1: Remove UI Writes To The Lobby Store

Goal: all state changes go through `ServerSession`.

- Replace `JoinChannelAction { session, lobby_store, task }` with an action that only calls `ServerSession` methods.
- Replace `VoiceControlFuture { session, lobby_store, task }` with a session-only action.
- Remove direct `lobby_store.set(session.lobby())` from rail/voice actions.
- Keep optimistic behavior by ensuring the corresponding `ServerSession` method publishes immediately.
- Add tests for optimistic join/leave:
  - after `select_channel`, subscribers observe the selected channel once;
  - after `leave_channel_locally`, subscribers observe the cleared channel once.

Acceptance: no code outside the store/subscriber wrapper calls `Store<LobbyState>::set` for lobby state.

### Phase 2: Replace The Shadow Store With A Store Handle

Goal: `LobbyScreen` should subscribe once and render from one current snapshot.

- Introduce a `LobbyStoreHandle` or `LobbySubscription` that owns:
  - current snapshot signal/store;
  - watch receiver;
  - last applied generation.
- Move the `LobbyStateSpy` behavior into that wrapper and rename it to match its role.
- Stop copying `session.lobby()` into the UI store every render. Use `session.lobby()` only to initialize the subscription cache before the first watch update.
- Include a monotonically increasing generation in published updates so the UI can ignore stale async completions without comparing full `LobbyState`.
- Prefer `watch::Sender<LobbySnapshot>` where `LobbySnapshot` includes `{ generation, lobby }`.

Acceptance: `LobbyScreen::render` reads one UI snapshot and no longer performs full snapshot equality checks on every render.

### Phase 3: Split Reducer State From View Models

Goal: keep reducer state authoritative, but stop exposing the entire reducer shape to every component.

- Keep pure reducer functions in `session/lobby.rs`; avoid mixing UI derivations into this module.
- Add `ui/lobby/model.rs` selectors:
  - `rail_model(info, lobby, debug_mode_enabled)`;
  - `chat_pane_model(info, lobby, selected_channel)`;
  - `stream_browser_model(lobby, channel_id, local_user_id)`;
  - `watched_stream_model(lobby, watched_user_id)`.
- Move UI-only derivations out of render code:
  - `selected_text_channel`;
  - `stream_browser_channel`;
  - `unique_lobby_member_count`;
  - `screen_shares_for_channel`;
  - `watched_stream`;
  - local user label/voice state lookup.
- Test selectors directly under `crates/client/tests/unit/ui/lobby/model.rs`.

Acceptance: major render functions no longer scan `LobbyState` directly except at selector boundaries.

### Phase 4: Slim Component Props

Goal: component props should be stable, small, and explicit.

- Replace `LobbyRailProps { info, lobby, ... }` with `LobbyRailProps { model, actions }`.
- Replace `TextChannelsProps` with a `TextRailModel` that already contains selected/unread status per row.
- Replace `VoiceChannelsProps` with a `VoiceRailModel` that contains channel row models and user row models.
- Replace repeated `Option<FutureAction>` fields in `UserContextOverlayProps` with one `UserMenuActions` struct.
- Keep local UI state (`expanded`, hover, modal anchors, text input, scroll state) inside components. Do not move these into session state.
- Avoid `ctx.use_context::<ServerSession>()` inside leaf row functions. Resolve actions at the component boundary and pass closures/action structs down.

Acceptance: `PartialEq` impls no longer need to ignore action fields to avoid rerenders, and row components compare small models rather than full maps/vectors.

### Phase 5: Normalize Commands And Effects

Goal: one command path for local and async session mutations.

- Create a small command/action layer for UI:
  - `join_voice(channel_id)`;
  - `leave_voice()`;
  - `select_text_channel(channel_id)`;
  - `select_debug_chat()`;
  - `open_stream_browser(channel_id)`;
  - `watch_stream(user_id)`;
  - `stop_watching()`;
  - `start_stream(input)`;
  - `stop_stream()`;
  - `send_chat(input)`;
  - `request_chat_history(channel_id, before_id)`.
- Commands should call `ServerSession` for local optimistic state and server/network objects for async work, but UI components should not know which state is optimistic.
- Move repeated `no_connected_server` and settings-loading logic out of individual components where practical.
- Preserve current behavior: failed join should roll back to previous channel, failed first join should clear local selection.

Acceptance: render functions construct actions once per render boundary and row components only invoke action methods.

### Phase 6: State Domain Split

Goal: reduce cross-domain invalidation and make future features less risky.

After the behavior-preserving phases are merged, split `LobbyState` internally:

```rust
struct LobbyState {
  connection: LobbyConnectionState,
  voice: VoiceLobbyState,
  text: TextLobbyState,
  chat: ChatLobbyState,
  streams: StreamLobbyState,
  view: LobbyViewState,
}
```

Keep compatibility methods/selectors during migration so tests can move gradually.

Suggested ownership:

- `LobbyConnectionState`: receiver running, channel list received, keepalive, ping, disconnected, warning, reconnect disabled.
- `VoiceLobbyState`: voice channels, selected voice channel, users by channel, selected users cache.
- `TextLobbyState`: text channels, selected text channel, debug chat selection, unread ids.
- `ChatLobbyState`: messages, debug messages, history loading/has_more, command registry.
- `StreamLobbyState`: screen shares, watched user.
- `LobbyViewState`: stream browser channel and view-mode decisions if these remain UI-visible session state.

Acceptance: reducer tests prove each server message mutates only expected domains.

## Component Usage Rules After Refactor

- Components receive either a model or local state signals, not a full `LobbyState`.
- Context is reserved for app-wide services (`ServerSession`, `Storage`, settings popup) at screen boundaries.
- Leaf rows do not call `ctx.use_context`.
- Props should be cheap to compare. Prefer IDs, booleans, small strings, generations, and `Arc` shared collections.
- Actions are grouped into explicit structs by component area:
  - `RailActions`;
  - `ChatActions`;
  - `StreamActions`;
  - `UserMenuActions`.
- Local interaction state stays local:
  - expansion toggles;
  - context menu anchor/open state;
  - modal source selections;
  - chat input and scroll anchors;
  - hovered state;
  - slider draft values.

## Verification Plan

Run after each phase:

```powershell
cargo fmt --all --check
cargo check -p client
cargo test -p client --lib
```

Run after phases touching chat:

```powershell
cargo test -p client --test chat
```

Manual smoke after Phase 2 and Phase 5:

1. Connect to a server.
2. Verify initial text channels and voice channels load.
3. Switch text channels and confirm unread indicators clear.
4. Join voice, switch voice, leave voice.
5. Send a chat message.
6. Start stream modal, close it, start a stream if sources are available.
7. Watch and stop watching a stream.
8. Disconnect and reconnect.

Logs should show one receiver start, no repeated lobby subscription resets, and no duplicate chat-history requests for already-loading channels.

## Suggested Implementation Order

1. Add selector tests and fixture helpers.
2. Remove direct UI writes to `Store<LobbyState>`.
3. Introduce generated lobby snapshots and replace `LobbyStateSpy`.
4. Add `ui/lobby/model.rs` selectors and migrate `content.rs`, `stream_shared.rs`, and `rail.rs` derivations into it.
5. Slim `LobbyRail`, `TextChannels`, and `VoiceChannels` props.
6. Slim chat and stream props.
7. Group action handles and remove leaf-context lookups.
8. Split `LobbyState` into domains only after behavior-preserving work is stable.

## Progress

2026-06-18:

- Committed the dirty pre-refactor checkpoint as `bbb5b06`.
- Completed the first behavior-preserving slice:
  - removed direct rail/voice action writes to `Store<LobbyState>`;
  - added generated `LobbySnapshot` updates from `ServerSession`;
  - replaced `LobbyStateSpy` with a generation-aware lobby store subscriber;
  - added `ui/lobby/model.rs` selectors and migrated stream/content derivations;
  - slimmed text and voice channel props to row models.
- Verified with:
  - `cargo fmt --all --check`;
  - `cargo check --config .cargo/local-lurq.toml -p client`;
  - `cargo test --config .cargo/local-lurq.toml -p client --lib`;
  - `cargo test --config .cargo/local-lurq.toml --target-dir target\client-test -p client --test chat`.
- Completed the rail model slice:
  - replaced `LobbyRailProps { info, lobby }` with a `LobbyRailModel`;
  - moved rail-only derivations for channel rows, local user state, connection status, and local streaming into `ui/lobby/model.rs`;
  - added a direct unit test for `lobby_rail_model`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the stream model slice:
  - added stream browser, watched stream, and floating-preview selectors;
  - removed direct full-lobby arguments from stream browser, watched stream, and floating preview renderers;
  - added selector tests for watched-stream models and floating-preview visibility.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.

## Non-Goals

- Do not rewrite the transport protocol.
- Do not replace `lurq`.
- Do not move audio/video runtime state into the UI store.
- Do not refactor native video code as part of this store cleanup.
- Do not change visual design unless component prop changes expose a real rendering bug.
