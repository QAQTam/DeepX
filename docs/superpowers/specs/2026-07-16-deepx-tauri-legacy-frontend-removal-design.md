# DeepX Tauri Legacy Frontend Removal Design

**Date:** 2026-07-16

**Status:** Approved by user on 2026-07-16

**Source baseline:** `main@440eb7b`

**Decision:** Remove the legacy frontend implementation completely. `RawSessionState` and its typed event runtime become the only event-derived session model; surviving local UI concerns are isolated from protocol state.

## 1. Context

The current Tauri frontend contains two concurrent implementations.

The authoritative renderer already uses:

`Agent2Ui -> sessionEventRuntime -> reduceAgentEvent -> RawSessionState -> projectSession -> ConversationTranscript`

The legacy implementation remains mounted beside it:

- `App.tsx` sends part of the same event stream into `createChatStore`.
- `createChatStore` retains shadow turns, streaming state, usage, skills, compact state, ask-user state, dashboard data, and command helpers.
- `ChatView` renders conversation rows from `RawSessionState` but reads most controls from the legacy store.
- The new `TaskSidebar` is visible while a complete legacy `<aside class="sidebar">` is still mounted and hidden by CSS.
- `AppShell` is implemented and tested but not mounted.
- Multiple prototype components, stores, styles, and a timestamped backup file have no production consumers.

This duplication allows the transcript, composer, interaction dialogs, session restore, and hidden shell to disagree about whether a turn exists or is still running. The immediate scope is a frontend architecture correction, not the later `deepx.exe`/WebSocket extraction.

## 2. Goals

1. Delete the legacy conversation store, hidden shell, dead components, dead styles, dead tests, and backup artifacts.
2. Preserve every event-bearing behavior that the legacy store currently supplies to active UI.
3. Make generated `Agent2Ui` the only accepted agent-event interface in TypeScript after the Tauri boundary.
4. Keep one event-derived state per session and derive rendered conversation, streaming status, interactions, skills, compact status, environment data, usage, and dashboard data from it.
5. Keep workspace selection and request-submission progress as explicitly local UI state, never as a shadow conversation model.
6. Preserve stable streaming during normal use, session switches, WebView refresh, Tauri lifecycle replay, and session-seed remapping.
7. Retain existing frontend reliability repairs: keyed reactive rows, latest-wins Markdown rendering, animation-frame batching, reload snapshots, and lifecycle replay.

## 3. Non-goals

- Do not introduce `deepx.exe`, WebSocket, WinUI 3, ratatui, protocol V2, or a new backend thread manager.
- Do not perform a broad `deepx-msglp` or Ring-loop rewrite.
- Do not redesign the product visually beyond replacing the hidden legacy shell with the already-created shell components.
- Do not preserve unreachable prototype UI merely because files exist. Only active behavior and event-carried information require a mapped replacement.
- Do not hand-edit generated TypeScript bindings.

## 4. Authoritative Frontend Architecture

### 4.1 Typed event boundary

The Tauri listener and replay command are typed as `Agent2Ui` and `Agent2Ui[]`. `sessionReplayBuffer`, event dispatch, reducer calls, tests, and callbacks must not use `Record<string, unknown>` for agent events.

One dispatcher owns two responsibilities:

1. Push every `Agent2Ui` event into the session runtime.
2. Execute narrowly scoped side effects that are not state reduction, such as toasts, reconnection attempts, workspace loading, or session-list refresh.

The dispatcher must switch on `event.type` with exhaustive checking. It must not call a second conversation/event store.

### 4.2 Per-session state

`RawSessionState` remains the client projection of backend state. It owns:

- ordered turns, rounds, reasoning, answers, tool calls, tool results, and ordered stdout/stderr progress;
- turn status, timing, stop reason, and usage;
- session readiness, identity, title, model, context limit, pagination, tokens, and cache metrics;
- skills, environment deltas, notices, dashboard tasks/recent edits, and bounded audit activity;
- compact lifecycle, including active text and the terminal `turns_compacted` value;
- an ordered, idempotent queue of pending permission, ask-user, and plan interactions plus resolved interaction history;
- bounded metric history used by `ContextPanel`.

The pending-interaction representation must preserve the legacy ask queue and permission queue behavior. Enqueue is unique by interaction kind and id. Resolution removes the matching item, records its outcome on the owning turn, and exposes the next pending item without losing it. `ask_rejected` retains the active ask and adds the error notice so the user can retry.

### 4.3 Derived selectors

Pure selectors replace legacy boolean and metadata signals:

- `activeTurn(state)` returns the last `running` or `waiting` turn.
- `isSessionStreaming(state)` is true while an active turn exists.
- `activeInteraction(state)` returns the first pending interaction.
- `sessionUsage(state)` returns composer/context-panel metrics.
- `projectSession(state)` remains the only transcript projection.
- `failedPrompt(state)` derives retry text from the last failed turn when needed.

`done` is a terminal fallback: if a turn remains `running` or `waiting` because `turn_end` was unavailable, the reducer closes it. Normal `turn_end` remains authoritative when present.

### 4.4 Local UI state

A small per-session UI state owns only values that do not arrive as agent events:

- workspace path loaded and saved through Tauri commands;
- interaction-response submission state used to prevent duplicate clicks;
- transient panel visibility or local command errors that do not describe backend state.

It must not contain turns, rounds, streaming, pagination, usage, skills, compact state, pending interaction payloads, or restored-session data.

### 4.5 Session registry and shell

A focused session registry replaces parallel `chatStores`, `rawSessions`, `rawEventRuntimes`, and listener lookup logic in `App.tsx`. Each entry owns one raw signal, one event runtime, one local UI state, and one listener cleanup callback. Seed remapping moves the entry atomically without recreating its state.

`AppShell` becomes the mounted application shell and receives `TaskSidebar` plus the active workspace. The hidden legacy sidebar is removed. Any active navigation required by the current UI remains in `TaskSidebar`; hidden-only resize and workspace controls are not retained. Workspace continues through `ThreadHeader` and the existing workspace picker.

## 5. Legacy-to-New Data Mapping

| Legacy responsibility | New owner | Required behavior |
| --- | --- | --- |
| `turns`, round deltas, round completion | `RawSessionState.turns` | Thinking and answer deltas render without remount; completion must not erase previews when optional fields are absent. |
| tool previews, results, exec progress | `RawRound` | Tool identity, output, stdout/stderr provenance, and sequence order survive batching and restore. |
| `isStreaming`, `inputDisabled` | selectors from turn status | Composer queues follow-ups while the last turn is `running` or `waiting`; terminal events drain safely. |
| `sessionInfo`, `hasMore` | `RawSessionState.session` | Model, usage, context limit, total turns, pagination, title, tokens, and cache metrics remain visible. |
| `metricHistory` | bounded raw telemetry history | Context chart receives the same usage samples and survives a WebView refresh snapshot. |
| `askState` and ask queue | ordered raw interactions | Multiple asks are not overwritten; rejection remains retryable; resolution exposes the next ask. |
| permission queue | ordered raw interactions | Multiple requests remain ordered and idempotent; local submit state prevents duplicate responses. |
| plan review state | ordered raw interactions | Submitted plan content remains available until the matching resolution. |
| compact state/result/text | raw compact lifecycle | Streaming summary and the temporary completion result remain displayable. |
| skills | `RawSessionState.skills` | `SkillsView` reads available and active skills directly from the active raw session. |
| dashboard tasks/recent edits | raw dashboard state | `EnvironmentPopover` and task actions continue to use typed data. |
| audit records and notices | bounded raw activity/notices | Events are retained once without creating a shadow store. |
| workspace | local session UI state | Changing sessions does not leak one workspace into another. |
| error/restore text | raw notices plus selectors | Toasts remain side effects; retry text derives from the failed turn. |
| undo command | controller plus raw update/restore | A successful undo removes the turn from the authoritative state or reloads the backend snapshot; it cannot mutate only a deleted shadow store. |
| session restore/load more | raw reducer | Restored turns replace the session snapshot; older turns prepend once and preserve scroll position. |

Events intentionally producing no rendered state are `pong` and `shutdown_ack`. They remain exhaustively handled. `ready` changes readiness, while `done`, `cancelled`, and `error` enforce terminal state.

## 6. Refresh and Streaming Stability Contract

The frontend must maintain these invariants:

1. Register the per-session Tauri listener before invoking resume/start operations that can emit events.
2. Begin replay buffering before resume. While backend lifecycle replay is fetched, buffer live events for that seed.
3. Apply the backend replay first, then buffered live events, suppressing only exact replay duplicates. Never discard a distinct repeated delta.
4. Initialize the raw runtime from the versioned `sessionStorage` snapshot before rendering. An invalid or old snapshot is removed and falls back to backend restore/replay.
5. Commit terminal, interaction, restore, and lifecycle events immediately. Batch high-frequency deltas to at most one animation-frame commit without delaying reducer state.
6. Persist the reduced raw state after every committed update. Storage failure logs a warning but never blocks the UI commit.
7. Flush the latest reduced state during WebView cleanup, then dispose only frontend listeners/timers. Cleanup must not stop or delete the backend agent/session.
8. Resume after refresh reattaches to the existing session and merges lifecycle replay with buffered live events. It must not clear a valid restored transcript while waiting for backend data.
9. Seed remapping after `session_created` moves the registry entry, runtime, listener association, and reload-snapshot key as one operation.
10. Reducer operations are idempotent for replayable lifecycle events. Duplicate starts, previews, results, interactions, and terminal events cannot create duplicate logical rows.
11. Solid row identity and Markdown latest-wins cancellation remain unchanged so asynchronous rendering cannot overwrite newer text.
12. Schema changes to `RawSessionState` bump the reload snapshot version. Compatibility is fail-safe: discard incompatible client snapshots and recover from backend state.

The current Tauri lifecycle cache is a Phase 0 bridge, not a durable sequence journal. Strong sequence-based reconnect belongs to the later standalone backend plan.

## 7. Removal Scope

The implementation must delete, after their replacements pass tests:

- `src/store/chat.ts` and `src/store/chat.ask.test.ts`;
- the duplicate permission queue once raw interaction queue coverage exists;
- dead prototype stores `environmentStore` and `orderedProgress` plus their isolated tests;
- `AskDialog`, `AskForm`, `ThinkingBlock`, `ToolRow`, `TokenChart`, `StockChart`, and `interactions/PlanApprovalPrompt` plus obsolete tests;
- `SlashMenu`, its unused props/handler, and `styles/slash-menu.css`;
- the hidden legacy sidebar JSX, its width/version-only state, and `styles/sidebar.css`;
- the now-unreachable `ChangelogModal` and `styles/changelog.css` that are opened only by the hidden version control;
- unused `styles/token-chart.css`;
- `DiffBody.tsx.1781723177.1782263662`;
- all imports, types, event cases, comments, and tests that exist only for those implementations.

`AppShell`, `TaskSidebar`, `ConversationTranscript`, `AskUserPrompt`, `PermissionPrompt`, `CompactStatusRow`, `sessionEventRuntime`, replay buffer, view cleanup, and generated protocol bindings are retained and become authoritative.

## 8. Error Handling

- Unknown stable events fail exhaustive compile-time checks after binding regeneration; runtime boundary validation reports malformed external payloads without corrupting existing state.
- A reducer exception for one event is logged with seed and event type and surfaces a recoverable toast; it must not unregister the listener or erase prior state.
- A failed response command leaves its interaction pending and clears only the local submitting flag.
- A failed resume aborts replay buffering without dropping already buffered live events, then returns to the home view only when no usable session state exists.
- A failed workspace/dashboard auxiliary request does not block conversation streaming.
- Session deletion removes its registry entry, listener, reload snapshot, and local UI state only after the backend delete command succeeds.

## 9. Testing Strategy

### Reducer contract tests

- Cover every `Agent2Ui["type"]` and keep the exhaustive binding test.
- Verify live event reduction and `session_restored` projection produce equivalent visible turns.
- Verify duplicate lifecycle replay is idempotent.
- Verify optional `round_complete` fields do not erase previewed calls or streamed text.
- Verify multiple ask/permission/plan interactions queue and resolve in order.
- Verify `done`, `cancelled`, `error`, rejection, compact completion, dashboard usage, audit records, pagination, and undo updates.

### Runtime and reload tests

- Verify high-frequency events batch while terminal events commit immediately.
- Verify the latest state is persisted, incompatible snapshots are discarded, and storage errors do not block commits.
- Verify replay/live interleaving preserves order and suppresses only replay duplicates.
- Verify cleanup flushes and disposes frontend resources without invoking backend shutdown.
- Verify refresh during reasoning, tool execution, waiting interaction, and final-answer streaming reconstructs the same projection and remains live.
- Verify seed remapping preserves the active runtime and snapshot.

### Component and structural tests

- Render `ChatView` using raw state and local UI state only.
- Verify thinking, tool progress, stage answers, final answer, compact status, and interactions update without remount.
- Verify older-turn pagination is actually wired to the transcript or remove the unreachable control contract.
- Mount `AppShell` with exactly one sidebar.
- Add repository guards asserting no production imports or source references to `createChatStore`, legacy sidebar classes, legacy handler names, dead component names, or `Record<string, unknown>` agent dispatch.

### Verification gate

The removal is complete only when all of the following pass from the repository root:

```powershell
pnpm --dir crates/deepx-tauri test:run
pnpm --dir crates/deepx-tauri build
cargo test -p deepx-proto -- export_bindings
cargo test -p deepx-tauri
cargo check -p deepx-tauri --tests
git diff --check
```

Manual Tauri smoke testing must additionally cover live streaming, multiple tool calls, ask/permission/plan gates, right-click WebView refresh during a turn, session switching, session deletion, and application restart.

## 10. Delivery Sequence

1. Extend reducer tests and raw state until every active legacy responsibility has a typed new owner.
2. Add reload/replay/registry regression tests before changing lifecycle ownership.
3. Move `ChatView`, `SkillsView`, composer, interactions, dashboard, metrics, workspace, pagination, and undo onto raw/selectors/local UI state.
4. Mount `AppShell` and remove the hidden legacy shell.
5. Delete `createChatStore`, duplicate interaction queues, dead components/stores/styles/tests, and backup artifacts.
6. Add structural guards and run the full automated and manual verification gates.

Each step must remain buildable and receive its own focused commit. Backend execution architecture remains unchanged.

## 11. Acceptance Criteria

The work is complete when:

- no legacy conversation store, hidden shell, dead prototype component, dead style, or timestamped backup artifact remains;
- production frontend code has one typed event path and one event-derived state per session;
- every legacy event-bearing behavior listed in the mapping table is present in the new reducer, selectors, local UI state, or an explicit side effect;
- thinking, tools, stage answers, and final answers appear continuously without session switching or restart;
- right-click WebView refresh immediately restores existing messages and continues receiving an active turn;
- session switching, refresh, and replay do not duplicate, reorder, erase, or delay logical conversation rows;
- frontend cleanup never terminates backend session lifetime;
- generated protocol bindings, frontend tests/build, Tauri tests/check, structural guards, and manual refresh smoke tests all pass;
- the only remaining `Cargo.toml` working-tree state is any pre-existing user-owned change, untouched by this work.
