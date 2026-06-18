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
6. Fine-grained subscriptions: components should subscribe only to the state they render. Passing smaller props is not enough if a parent still rerenders every child from a full-lobby snapshot.

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

The UI-facing store should support selector/domain subscriptions. A component should depend on a narrow model revision such as rail, chat pane, stream browser, or local controls, instead of indirectly depending on the entire lobby revision. The expected shape is:

```text
LobbySnapshot revision
  -> selector cache / domain revisions
  -> component subscribes to one selected model
```

This means a chat-message update should not rerender voice channel rows, and a speaking/stream badge update should not rerender the chat timeline unless the selected model actually changes.

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

### Phase 3: Add Selector/Domain Subscriptions

Goal: components subscribe only to the model they render.

- Introduce a UI store handle that can expose narrow subscriptions, for example:
  - `subscribe_rail_model()`;
  - `subscribe_chat_pane_model(channel_id/debug)`;
  - `subscribe_stream_browser_model(channel_id)`;
  - `subscribe_watched_stream_model()`;
  - `subscribe_local_controls_model()`.
- Track a revision per selected model or compare selected model values before notifying subscribers.
- Keep `LobbyScreen` responsible for layout and action construction, not for cloning one full `LobbyState` into all children.
- Move current `lobby_rail_model`, `chat_pane_model`, and stream selectors behind this subscription boundary after their behavior is covered by tests.
- Components should mount their own subscriber wrapper for their model or receive a small subscribed store/signal handle, not a full-lobby prop.

Acceptance: changing chat messages does not notify rail/voice/stream components unless their selected model changes; voice speaking or stream state updates do not notify chat timeline components unless their selected model changes.

### Phase 4: Split Reducer State From View Models

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

Acceptance: major render functions no longer scan `LobbyState` directly except at selector/subscription boundaries.

### Phase 5: Slim Component Props

Goal: component props should be stable, small, and explicit.

- Replace `LobbyRailProps { info, lobby, ... }` with `LobbyRailProps { model, actions }`.
- Replace `TextChannelsProps` with a `TextRailModel` that already contains selected/unread status per row.
- Replace `VoiceChannelsProps` with a `VoiceRailModel` that contains channel row models and user row models.
- Replace repeated `Option<FutureAction>` fields in `UserContextOverlayProps` with one `UserMenuActions` struct.
- Keep local UI state (`expanded`, hover, modal anchors, text input, scroll state) inside components. Do not move these into session state.
- Avoid `ctx.use_context::<ServerSession>()` inside leaf row functions. Resolve actions at the component boundary and pass closures/action structs down.

Acceptance: `PartialEq` impls no longer need to ignore action fields to avoid rerenders, and row components compare small models rather than full maps/vectors.

### Phase 6: Normalize Commands And Effects

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

### Phase 7: State Domain Split

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
5. Add selector/domain subscriptions so components can subscribe only to the selected model they render.
6. Slim `LobbyRail`, `TextChannels`, and `VoiceChannels` props behind those subscriptions.
7. Slim chat and stream props behind those subscriptions.
8. Group action handles and remove leaf-context lookups.
9. Split `LobbyState` into domains only after behavior-preserving work is stable.

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
- Completed the chat pane model slice:
  - added `ChatPaneModel` for message selection, initial history loading, paging availability, local user id, and last error;
  - removed direct full-lobby arguments from `text_channel_detail`;
  - added selector tests for chat paging and initial loading state.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the first fine-grained subscription slice:
  - moved rail model updates into a zero-size `LobbyRailModelSubscriber`;
  - `LobbyRail` now owns a local `Store<Option<LobbyRailModel>>` and receives only connection identity/debug props;
  - the subscriber advances the full lobby watch, but only updates the visible rail store when `LobbyRailModel` changes.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the chat pane fine-grained subscription slice:
  - wrapped the chat detail view in a `TextChannelDetail` component with a local `Store<Option<ChatPaneModel>>`;
  - added a zero-size `ChatPaneModelSubscriber` that updates only the selected chat model;
  - stopped deriving and passing `ChatPaneModel` from `content.rs`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the owned stream model prerequisite:
  - converted stream browser, watched stream, and floating preview models from borrowed lobby references to owned comparable structs;
  - added devtools support for stream browser/watching models so they can be stored in component-local stores;
  - updated stream renderers and tests for owned stream share/user data.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the stream browser fine-grained subscription slice:
  - wrapped stream browsing in a `StreamBrowserPane` component with a local `Store<Option<StreamBrowserModel>>`;
  - added a zero-size `StreamBrowserModelSubscriber` that updates only the selected channel stream browser model;
  - stopped deriving and passing `StreamBrowserModel` from `content.rs`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the watched stream fine-grained subscription slice:
  - wrapped stream channel content in a `StreamWatchingPane` component with a local `Store<Option<StreamWatchingModel>>`;
  - added a zero-size `StreamWatchingModelSubscriber` that updates only the selected channel watched-stream model;
  - stopped deriving and passing `StreamWatchingModel` from the main body, letting the stream channel wrapper switch between browser and watched-stream views.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the floating stream preview fine-grained subscription slice:
  - wrapped the root floating preview in a zero-size `FloatingStreamPreviewPane` with a local `Store<Option<WatchedChannelScreenShare>>`;
  - added a zero-size `FloatingStreamPreviewModelSubscriber` that updates only the floating-preview model;
  - stopped deriving `floating_stream_preview_model` in `LobbyScreen`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the main top bar fine-grained subscription slice:
  - added `MainTopBarModel` for debug chat, text chat, watched stream, stream browser, and default voice top-bar states;
  - wrapped the top bar in a `MainTopBar` component with a local `Store<Option<MainTopBarModel>>`;
  - added a zero-size `MainTopBarModelSubscriber` and stopped deriving top-bar state from the full lobby in `content::main`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the main body fine-grained subscription slice:
  - added `MainBodyModel` for debug chat, text chat, stream channel, empty voice, and select-channel states;
  - wrapped main body routing in a `MainBody` component with a local `Store<Option<MainBodyModel>>`;
  - added a zero-size `MainBodyModelSubscriber` and removed the full-lobby prop from `content::main`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the lobby shell store narrowing slice:
  - replaced `LobbyScreen`'s `Store<LobbyState>` with `Store<Option<LobbyShellModel>>`;
  - added `LobbyShellModel` for receiver/disconnect state and initial chat-history targets;
  - changed the root subscriber to apply only shell-model changes, and updated the disconnected screen to consume the shell model instead of the full lobby.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the first rail action cleanup slice:
  - added explicit text-channel and debug-chat select action handles;
  - passed those actions from `LobbyRail` into text/debug channel props;
  - removed hidden `ServerSession` context reads from `TextChannels` and `DebugChannels`.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the second rail action cleanup slice:
  - passed the leave-session handle from `LobbyRail` into the rail header leave button;
  - passed the current local voice state from `LobbyRail` into the voice control row;
  - removed hidden `ServerSession` context reads from those rail leaf controls.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the voice channel action cleanup slice:
  - passed the session handle explicitly from `LobbyRail` into `VoiceChannels`;
  - kept user-menu action construction local to `VoiceChannels`, but removed its hidden `ServerSession` context read;
  - voice channel rows now receive their stream-browser/session dependency through props.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the user context overlay storage cleanup slice:
  - passed `Storage` explicitly from `LobbyRail` through `VoiceChannels` into `UserContextOverlay`;
  - passed storage into user volume and normalization controls through props;
  - removed hidden `Storage` context reads from user context overlay controls.
- Re-verified the same formatter, client check, client library tests, and isolated chat integration test.
- Completed the stream pane session cleanup slice:
  - passed `ServerSession` explicitly into the stream browser pane;
  - initialized stream browser, watched stream, and floating preview stores from props instead of hidden context reads;
  - kept stream model subscribers as the only stream components reading `ServerSession` context for lobby-watch updates.
- Completed the main content session cleanup slice:
  - passed `ServerSession` explicitly into the main top bar component;
  - initialized main top-bar and body model stores from props instead of hidden context reads;
  - kept main content model subscribers as the only content components reading `ServerSession` context for lobby-watch updates.
- Completed the settings-popup dependency cleanup slice:
  - read `SettingsPopupHandle` once at the lobby boundary;
  - passed the handle explicitly into the rail controls and stream modal;
  - removed leaf `SettingsPopupHandle` context reads from lobby controls.
- Completed the rail service dependency cleanup slice:
  - passed `ServerSession` and `Storage` into `LobbyRail` from the lobby boundary;
  - built rail actions from explicit props instead of hidden context reads;
  - left the rail model subscriber as the only rail component reading context for lobby-watch updates.
- Completed the subscriber session dependency cleanup slice:
  - passed `ServerSession` explicitly into shell, rail, chat, content, and stream model subscribers;
  - removed subscriber `ServerSession` context reads from lobby components;
  - left subscriber context reads limited to their local model stores.
- Completed the user menu action grouping slice:
  - added `UserMenuActions` as the explicit moderation action contract for the user context overlay;
  - replaced four separate overlay action props with one grouped action prop;
  - kept overlay prop comparison focused on model data and action availability.

## Non-Goals

- Do not rewrite the transport protocol.
- Do not replace `lurq`.
- Do not move audio/video runtime state into the UI store.
- Do not refactor native video code as part of this store cleanup.
- Do not change visual design unless component prop changes expose a real rendering bug.
