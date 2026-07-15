# DeepX Codex-Compatible Execution Architecture

**Date:** 2026-07-16

**Status:** Approved direction; written specification pending user review

**Decision:** Compatibility level B — preserve Codex-like core method names, DTO concepts, and lifecycle semantics without promising wire-level compatibility.

## 1. Context

DeepX currently combines a Tauri/WebView client with backend agent lifetime and event delivery closely enough that a frontend reload can affect active sessions. The immediate missing-output and delayed-stream symptoms are primarily frontend projection and lifecycle defects, but they expose deeper architectural debt: no durable replay cursor, duplicated frontend projections, protocol drift, process-global tool state, and a single-session Ring loop.

The project will therefore proceed in order:

1. Repair frontend reliability on the current architecture.
2. Immediately audit the backend against the approved Codex-inspired lifecycle model.
3. Harden the execution kernel.
4. Extract a standalone `deepx.exe` app server behind a transport-neutral contract.
5. Move Tauri, ratatui, and later WinUI 3 onto one shared client contract.

This is a staged migration, not a big-bang rewrite.

## 2. Architecture Decision

DeepX will use a selective semantic port of Codex architecture.

### Adopt

- `Thread`, `Turn`, and typed `Item` as the public conversation model.
- A process-level thread manager owning independently addressable sessions.
- A non-blocking submission loop separated from active turn execution.
- Per-thread and per-turn cancellation trees.
- Iterative model/tool turns with ordered tool-result integration.
- Explicit pending lifecycles for permission, ask-user, and plan review.
- Durable terminal events, snapshots, replay cursors, and reconnect recovery.
- Bounded request/event queues and defined slow-client behavior.
- Generated Rust/TypeScript/JSON schema from one protocol source.
- A shared client facade usable through in-process and out-of-process transports.

### Adapt

- Method names and DTO concepts will resemble Codex app-server V2 where they fit DeepX.
- Existing DeepX crates, providers, tools, permissions, session storage, skills, and product behavior remain authoritative.
- WebSocket will be an adapter behind the protocol/client interface, not the architecture itself.
- Existing `deepx-msglp` becomes the execution-kernel boundary incrementally; it is not renamed or replaced in the first migration step.

### Do not adopt

- OpenAI-specific authentication, account, telemetry, enterprise policy, remote-control, and model-routing surfaces.
- Codex prompts, feature flags, UI implementation, or unrelated MCP/plugin/application surfaces.
- Unix-specific transport choices as Windows requirements.
- Source-level copying by default. Any copied generic code requires explicit provenance and license review.
- A promise that an unmodified Codex client can connect to DeepX.

## 3. Compatibility Contract

Compatibility level B means semantic and naming compatibility, not byte-for-byte compatibility.

### Stable method family

The first stable protocol surface will use these concepts:

- `initialize`
- `thread/start`
- `thread/read`
- `thread/resume`
- `thread/list`
- `thread/subscribe`
- `turn/start`
- `turn/interrupt`
- `item/started`
- `item/delta`
- `item/completed`
- `turn/completed`

DeepX-only methods and fields must be explicit extensions rather than silent divergences. Experimental or not-yet-stable fields must be capability-gated during `initialize`.

### Stable DTO concepts

- `Thread`: identity, metadata, status, ordered turns, latest durable sequence.
- `Turn`: identity, status, user input, ordered items, terminal outcome, usage.
- `Item`: typed user message, assistant message, reasoning summary, tool call, tool result, permission request, ask-user request, plan review, error, or extension item.
- `EventEnvelope`: protocol version, thread id, optional turn/item id, monotonic sequence, event type, payload.
- `RequestEnvelope`: request id, method, thread id when applicable, idempotency key for mutating operations, payload.

Field-level equality with Codex is not required. Meaning, lifecycle, and ordering are required.

## 4. Component Boundaries

### `deepx-proto`

Owns transport-neutral protocol types, version negotiation, error codes, event envelopes, and generated bindings. Frontends must not maintain handwritten shadow unions.

### `deepx-msglp`

Owns the agent execution kernel during migration:

- `ThreadManager`: `ThreadId -> ThreadHandle`.
- Per-thread `SessionBundle` and submission queue.
- One explicit active-turn task per thread.
- Per-turn runtime context containing workspace, session, permissions, cancellation, and event sink.
- Iterative model/tool loop with no recursive lap continuation.
- Pending server requests represented by typed state, not implicit frontend assumptions.

The current process-global `CANCEL`, current session, and current workspace state must not remain on any multi-thread execution path.

### `deepx-app-server`

Produces `deepx.exe` and is the sole owner of threads, agents, persistence, cancellation, and the ordered event journal. It exposes the protocol through interchangeable transports and does not depend on Tauri or a particular UI.

### `deepx-client`

Provides one typed facade for initialization, thread lifecycle, turns, subscriptions, reconnect, and error translation. Tauri, ratatui, WinUI 3, integration tests, and diagnostic tools all use this facade.

### Frontends

Frontends own presentation state only. A WebView/component cleanup may unsubscribe a view, but it must not destroy a backend thread. Process shutdown is an explicit host action.

Within `deepx-tauri.exe`, the Rust host owns `deepx-client` and the WebSocket connection. The WebView continues to use a typed Tauri command/event gateway and never receives the companion-process token. Ratatui and WinUI 3 use the same client facade through their native host layer.

## 5. Process and Transport Design

The protocol is transport-neutral. The extraction sequence is:

1. Introduce the shared protocol and client facade while the backend can still run in-process for migration tests.
2. Start `deepx.exe` as a Tauri companion process.
3. Connect `deepx-tauri.exe` through loopback WebSocket, as requested by the product direction.
4. Retain the ability to add Windows named-pipe or stdio transports without changing DTOs or clients above the transport interface.

Local WebSocket requirements:

- Bind only to `127.0.0.1` or `[::1]`; never all interfaces by default.
- Use an ephemeral port or OS-assigned port.
- On launch, `deepx.exe` writes one bootstrap JSON record containing the selected port and a high-entropy per-process token to its parent-owned stdout pipe.
- The client supplies that token in the WebSocket `Authorization: Bearer` header; the token never appears in the URL or persistent configuration.
- Reject a missing or invalid bearer token during the WebSocket handshake, before protocol initialization.
- Limit frame size, request rate, connection count, and pending queue depth.
- Do not persist the companion token.

Transport disconnect must detach the connection, not terminate its subscribed threads.

## 6. Data and Event Flow

### Startup and restore

1. The host starts or discovers `deepx.exe`.
2. The client authenticates and calls `initialize` with supported protocol/capabilities.
3. The client calls `thread/read` or `thread/resume`.
4. The server returns a snapshot and `last_seq`.
5. The client subscribes with `after_seq = last_seq`.
6. The server replays later durable events, then continues with live events.

Snapshot and replay must close the race between reading state and subscribing. The server must either provide an atomic read-and-subscribe operation or retain all events after the returned sequence until subscription is established.

### Turn execution

1. `turn/start` carries a request id and idempotency key.
2. The user item is persisted before acknowledgement.
3. The thread submission loop starts an active turn task without blocking command intake.
4. The turn iteratively samples the model, emits typed items, admits tools, executes them, and appends ordered results.
5. Permission, ask-user, or plan review transitions the turn into explicit waiting state while submission intake remains alive.
6. `turn/completed` or aborted terminal state is persisted before its notification is published.

### Reconnect

A reconnecting client repeats snapshot plus replay from its last applied sequence. Duplicate requests and events are safe through idempotency keys and sequence-based reduction.

## 7. Agent Loop Invariants

The backend audit and kernel refactor must enforce:

1. A connection cannot own thread lifetime.
2. One thread's cancellation, workspace, permission, or tool context cannot affect another thread.
3. Submission intake remains responsive while a turn samples, executes tools, or waits for user input.
4. Turn continuation is iterative and bounded by explicit policy; it does not recurse through the Rust stack.
5. Parallel-capable tools may execute concurrently, while serial tools exclude concurrent execution where required.
6. Tool results are committed to model context in model-call order.
7. Every started item and turn receives exactly one terminal outcome.
8. Interrupt first establishes the turn's aborted state, then resolves or cancels pending requests consistently.
9. Terminal events and their reconstructed snapshot survive client reload and backend restart.
10. Commands are routed by thread id; no command is discarded merely because another thread is switching or loading.

## 8. Backpressure and Error Handling

- Request ingress, processing, and per-connection outbound queues are bounded.
- Saturated ingress returns a retryable overloaded error; it does not silently drop commands.
- High-frequency nonterminal deltas may be coalesced only when the durable snapshot can reconstruct the same logical item.
- Terminal events, approval/ask requests, errors, and request responses are never deliberately dropped.
- A persistently slow connection is disconnected with a recoverable reason and can resume by sequence.
- Protocol-version mismatch fails during `initialize` with supported-version information.
- Backend process failure is surfaced separately from transport loss; the client retries only idempotent requests automatically.
- Unknown stable events are retained/logged and trigger compatibility diagnostics rather than crashing the entire reducer.

## 9. Testing Strategy

### Phase 0 frontend gate

- Thinking, tool, stage, and final-answer rows appear without session remount.
- WebView reload reconstructs the conversation and does not stop an active turn.
- A backend-completed turn appears without waiting for session switching or process restart.
- Markdown async work cannot overwrite newer content.
- Generated protocol bindings cover every runtime event.
- Delta load does not starve the UI event loop.

### Backend audit gate

Create regression tests before structural fixes for:

- Cross-thread contamination from global cancellation/session/workspace.
- Repeated `ContinueTurn` laps and emitter lifetime.
- Commands arriving during session load/switch.
- Interrupt while waiting for permission, ask-user, or plan review.
- Terminal-event persistence and restart recovery.
- Concurrent independent threads.

### Protocol and transport gate

Run one conformance suite against in-process and WebSocket transports:

- Initialize/version negotiation.
- Start/read/resume/subscribe.
- Snapshot/replay without gaps or duplicate logical items.
- Idempotent `turn/start`.
- Slow consumer and bounded queues.
- Disconnect/reconnect during streaming and while waiting for user input.
- Unauthorized loopback connections.

### Multi-client gate

Tauri and ratatui must reconstruct the same thread from the same event journal. WinUI 3 enters only after the shared client and conformance suite are stable.

## 10. Delivery Phases

### Phase 0 — Frontend reliability repair

Repair the current Solid/WebView rendering, reload lifecycle, reducer duplication, batching, Markdown races, and protocol drift. Do not wait for backend extraction to fix user-visible P0 failures.

Phase 0 may include narrowly scoped Tauri gateway or backend lifecycle changes required to expose an existing-session snapshot and make reload recovery reliable. It must not introduce `deepx.exe`, WebSocket, ThreadManager, or the broader kernel refactor ahead of their phases.

### Phase 1 — Backend Codex-parity audit

Immediately after Phase 0, produce a verified gap matrix and failing regression tests covering thread ownership, active turns, cancellation, pending requests, tool ordering, persistence, replay, and backpressure.

### Phase 2 — Execution-kernel hardening

Introduce per-thread ownership, remove process-global execution state, replace recursive laps, formalize pending request lifecycles, and guarantee durable terminal outcomes.

### Phase 3 — Protocol and standalone app server

Stabilize generated protocol V2, create the shared client, extract `deepx.exe`, and enable authenticated loopback WebSocket without changing execution semantics.

### Phase 4 — Multiple frontends

Move Tauri fully onto the shared client, reintroduce ratatui, then add WinUI 3. No frontend receives a private backend API.

Each phase receives its own implementation plan and acceptance gate. The first implementation plan after approval of this specification is Phase 0 only.

## 11. Success Criteria

The program is successful when:

- Current missing/late-output and reload-loss failures are covered and fixed before extraction.
- `deepx.exe` owns backend state independently of WebView lifetime.
- Multiple threads execute without shared cancel/workspace/session contamination.
- Any supported client can rebuild a thread from snapshot plus ordered replay.
- Tauri reload and reconnect do not terminate or lose an active turn.
- Core method naming and DTO semantics are recognizably Codex-compatible at level B.
- DeepX-specific behavior remains explicit and does not depend on Codex-specific product subsystems.

## 12. References

- OpenAI Codex source baseline: `38b064c31b1f7464b281006316ec878ed23fea77`.
- Relevant Codex concepts: app-server JSON-RPC, Thread/Turn/Item, thread manager, submission loop, turn cancellation, ordered tool integration, generated schemas, and in-process/out-of-process client parity.
- DeepX source baseline at design approval: `1b5f4a78c2e7784d9b3a5247dceb76f6b8f65205`.
