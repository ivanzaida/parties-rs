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
- Completed the stream body action slimming slice:
  - removed unused start-stream, stop-stream, and stop-watching props from stream browser/body components;
  - kept `watch_stream` as the only stream-browser/body action dependency;
  - left stop-stream in the rail controls and stop-watching/start-stream in the top bar where they are rendered.
- Completed the chat action grouping slice:
  - added `ChatActions` as the chat pane action contract;
  - replaced separate `chat_history` and `send_chat` component props through content/chat boundaries;
  - kept lower-level chat helpers receiving only the action they invoke.
- Completed the rail stream action grouping slice:
  - added `RailStreamActions` for rail stream controls;
  - replaced separate rail stream props with one grouped action prop;
  - kept rail internals passing only watch or stop/start controls to the rows that render them.
- Completed the voice channel action grouping slice:
  - added `VoiceChannelActions` for voice channel row actions;
  - replaced separate voice channel session/join/watch props with one grouped action prop;
  - kept voice channel internals passing only join/watch/session handles to the rows that need them.
- Added selector invalidation guard tests:
  - proved rail models ignore chat-message-only updates;
  - proved chat pane models ignore voice-presence-only updates;
  - covered the fine-grained subscription rule with pure selector equality tests.
- Completed the subscriber plumbing extraction slice:
  - added one shared `LobbyModelSubscription` helper for lobby watch receiver ownership and generation dedupe;
  - replaced duplicated subscriber watch loops across shell, rail, chat, main content, stream browser, watched stream, and floating preview;
  - kept each component subscribed to only its own selected model and preserved model equality checks before store updates.
- Expanded selector invalidation guard tests:
  - covered shell, main top bar, main body, stream browser, watched stream, and floating preview models;
  - proved chat-message-only and voice-presence-only updates do not invalidate unrelated subscribed models.
- Completed action equality cleanup:
  - made text/debug channel selection actions compare by connected server identity;
  - made join-channel and optional user-control sessions compare by connected server identity;
  - kept opaque future-only actions compared by availability where no stable identity is exposed.
- Consolidated lobby session identity checks:
  - added shared helpers for server address extraction and session equality;
  - replaced repeated inline `ServerSession::info().address` comparisons across lobby props and controls.
- Removed field-only lobby snapshot reads:
  - added `ServerSession::selected_channel_id()`;
  - changed join-channel and reconnect actions to read only the selected voice channel id instead of cloning the full lobby state.
- Split debug report generation out of lobby actions:
  - moved debug user/voice/stream/channel/audio/video report builders into `ui/lobby/debug_reports.rs`;
  - kept command dispatch in `actions.rs` focused on parsing/running actions instead of owning full lobby-state inspection helpers.
- Centralized current-model hydration:
  - added shared helpers for deriving the current model from `ServerSession`;
  - removed direct `session.lobby()` calls from render-path lobby components, leaving full snapshot reads in the subscription helper and debug reports.
- Completed explicit model-store subscriber props:
  - passed local model stores directly into shell, rail, chat, content, stream browser, watched stream, and floating-preview subscribers;
  - removed remaining local model-store context reads from lobby subscribers.
- Tightened shared subscriber session binding:
  - keyed `LobbyModelSubscription` receivers by connected server identity;
  - reset receiver state and applied generation when a reused subscriber receives a different `ServerSession`.
- Narrowed rail header and bottom component boundaries:
  - mounted rail header and bottom status/control areas as explicit components;
  - passed each section only the model fields and action handles it renders, reducing rerenders from unrelated rail model changes.
- Split rail section subscriptions:
  - added header, channel-list, and bottom/status rail models with dedicated selectors;
  - replaced the combined rail model subscription with separate local stores and subscribers for each rail section.
- Removed rail join debug snapshot reads:
  - added a field-specific `ServerSession` debug summary for voice-channel join diagnostics;
  - replaced the remaining rail action `session.lobby()` clone with the narrower summary method.
- Removed the legacy combined rail model:
  - deleted the unused `LobbyRailModel` and `lobby_rail_model` selector after rail sections moved to dedicated subscriptions.
- Narrowed voice channel row rendering:
  - mounted each voice channel group and channel header as keyed components;
  - kept user rows componentized so channel/header props can skip rerenders when unrelated users update.
- Narrowed text channel row rendering:
  - mounted each text channel row as a keyed component with only row model and select action props.
- Narrowed debug channel row rendering:
  - mounted the debug chat row as a component with only selected state and select action props.
- Narrowed rail bottom row rendering:
  - mounted the local user and control rows as components with focused props;
  - kept voice, stream, and settings control comparisons scoped to the controls row.
- Narrowed stream modal source card rendering:
  - mounted each share source card as a component with explicit source, selection, codec, and metric props;
  - kept the source selection signal as an action handle outside card equality.
- Narrowed chat command suggestion rows:
  - mounted command suggestion rows as keyed components with derived fill text, description, and usage parts;
  - kept input, registry, and invalid-feedback animation details out of row props unless they affect the rendered row.
- Narrowed stream browser card rendering:
  - mounted stream and voice-user cards as keyed components with card-specific props;
  - removed the unused local-user id prop from stream browser and watching-pane comparisons.
- Narrowed watched-stream switcher rendering:
  - mounted each stream switcher card as a keyed component with stream, selection, and click-enabled props;
  - read watch-action pending state once at the switcher boundary instead of inside every card.
- Narrowed floating preview close control:
  - mounted the close button as its own component;
  - scoped stop-watching pending-state reads to the button instead of the full floating preview render.
- Narrowed watched-stream back control:
  - mounted the top-bar back button as its own component;
  - scoped stop-watching pending-state reads to the button instead of the watched-stream top bar.
- Removed duplicate stream modal action-state reads:
  - reused the start-stream state already read by the modal for the actions row pending flag;
  - avoided an extra `start_stream.state().get()` inside modal actions.
- Narrowed voice user row watch props:
  - removed unused channel id/count props from voice user rows;
  - read watch-stream pending state once at the channel group boundary and passed row-specific availability.
- Narrowed rail stream control:
  - mounted the rail stream button as its own component;
  - scoped stop-stream pending-state reads to the button instead of the full controls row.
- Moved stream watch pending reads into cards:
  - removed watch-action pending reads from stream browser grid and watched-stream switcher parents;
  - scoped those reads to the mounted stream card components that render the clickable watch target.
- Moved voice watch pending reads into user rows:
  - removed the channel-group watch-action pending subscription;
  - scoped pending reads to mounted streaming user rows that can render the watch action.
- Keyed stream modal source cards:
  - mounted share source cards by source kind/id identity;
  - preserved per-source component instances across source list changes.
- Keyed command suggestion row components:
  - mounted filtered command suggestion rows by command fill identity;
  - kept row component instances stable when suggestion filtering changes.
- Added explicit channel row component keys:
  - keyed text channel rows, voice channel groups, voice channel headers, and voice user rows by channel/user identity;
  - kept component identity stable even inside keyed list rendering helpers.
- Keyed server settings voice channel rows:
  - mounted voice channel settings rows by channel id;
  - left text settings rows unchanged because they are plain elements rather than mounted components.
- Split server settings member rows:
  - mounted member rows by user id and moved role-picker signal reads into row/modal components;
  - prevented member role-picker changes from dirtying the full server settings screen render.
- Removed audio settings full lobby snapshot:
  - reused `ServerSession::selected_channel_id()` for voice restart guards;
  - avoided cloning the full lobby state from audio settings.

## Current Residual Reads

- `session.lobby()` remains only in debug report generation and subscription hydration/current-model fallback.
- `server_settings.rs` still snapshots `session.lobby()` in render for settings pages; split this into page-specific subscribed settings models before treating it as part of the hot connected-lobby path.
- Root `ctx.use_context` reads remain in `LobbyScreen` for session, storage, and settings-popup handles.
- Action `state().get()` reads remain where the rendered control or lifecycle owns the state:
  - mounted rail stream, stream card, stream switcher, floating preview close, watched-stream back, and voice user row controls;
  - stream modal lifecycle/error handling;
  - disconnected reconnect lifecycle/status;
  - subscription helper future state.
- No connected-lobby component reads or writes `Store<LobbyState>` directly, and no UI action writes a mirrored lobby store.

## Non-Goals

- Do not rewrite the transport protocol.
- Do not replace `lurq`.
- Do not move audio/video runtime state into the UI store.
- Do not refactor native video code as part of this store cleanup.
- Do not change visual design unless component prop changes expose a real rendering bug.
