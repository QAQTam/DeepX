# DeepX Companion Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port Clawd on Desk into a DeepX-owned companion client that DeepX starts automatically and that supports status/HUD, permission approval, ask_user, and plan review over a private localhost protocol.

**Architecture:** `AgentRegistry` remains the sole owner of agent stdin/stdout. A new transport-neutral `deepx-companion` crate receives filtered events from one registry dispatch path, maintains authoritative snapshots and interaction claims, and serves the independent Electron pet over authenticated localhost WebSocket. The pet retains the current renderer, themes, and assets but disables all original agent hooks, log monitors, HTTP routes, remote approval, and updater behavior.

**Tech Stack:** Rust 2024, serde, tokio, tokio-tungstenite, Tauri 2, Electron 41, Node test runner, ws.

## Global Constraints

- Preserve `AgentRegistry` as the only agent stdin/stdout owner.
- Route Tauri UI and pet responses through one atomic interaction coordinator.
- Bind only to `127.0.0.1`; use a random port, server epoch, monotonic sequence, and high-entropy token.
- A missing or crashed pet must never block the agent or disable the Tauri UI fallback.
- Keep Clawd code and current art in the separate AGPL repository with existing license notices; do not copy them into the MIT DeepX repository.
- Do not publish, push, or build a distributable installer in this phase.

---

### Task 1: Companion protocol contracts

**Files:**
- Create: `crates/deepx-proto/src/companion.rs`
- Modify: `crates/deepx-proto/src/lib.rs`
- Test: `crates/deepx-proto/src/companion.rs`

**Interfaces:**
- Produces `CompanionClientMessage`, `CompanionServerMessage`, `CompanionSnapshot`, `CompanionSession`, `CompanionInteraction`, and `CompanionCommandResult` serde DTOs.
- Uses protocol version `1`, a per-process `server_epoch`, server-global `seq`, and client `command_id`.

- [ ] Write round-trip and unknown-version failing tests for hello, snapshot, activity, permission, ask, plan, response, resolved, focus, heartbeat, and shutdown messages.
- [ ] Run `cargo test -p deepx-proto companion` and confirm failures are caused by missing DTOs.
- [ ] Implement the minimal tagged enums and payload structs; export TypeScript bindings where browser-facing types are useful.
- [ ] Re-run `cargo test -p deepx-proto companion` and `cargo test -p deepx-proto`.

### Task 2: Authoritative journal and interaction coordinator

**Files:**
- Create: `crates/deepx-companion/Cargo.toml`
- Create: `crates/deepx-companion/src/lib.rs`
- Create: `crates/deepx-companion/src/journal.rs`
- Create: `crates/deepx-companion/src/interaction.rs`
- Modify: workspace `Cargo.toml`

**Interfaces:**
- Produces `CompanionState::snapshot()`, `CompanionState::publish(event)`, and `InteractionCoordinator::{register, claim, commit, rollback, resolve}`.
- Interaction identity is `(seed, generation, kind, request_id)`; claim failure returns `AlreadyResolved` or `StaleGeneration`.

- [ ] Write failing tests for monotonically ordered events, snapshot replacement, pending interaction reconstruction, first-writer-wins, duplicate command idempotency, stale generation rejection, and rollback after failed agent write.
- [ ] Run `cargo test -p deepx-companion` and verify the intended failures.
- [ ] Implement the in-memory state and coordinator without networking.
- [ ] Re-run the crate tests and `cargo test -p deepx-proto`.

### Task 3: WebSocket hub and process supervision

**Files:**
- Create: `crates/deepx-companion/src/hub.rs`
- Create: `crates/deepx-companion/src/supervisor.rs`
- Modify: `crates/deepx-companion/src/lib.rs`

**Interfaces:**
- Produces `CompanionHub::bind_loopback`, `CompanionHubHandle::{publish, shutdown, endpoint}`, and `PetSupervisor::{start, note_exit, shutdown}`.
- The client authenticates before receiving a snapshot; maximum frame size is fixed at 1 MiB.

- [ ] Write failing integration tests for loopback binding, invalid token rejection, hello/version negotiation, snapshot-before-delta ordering, reconnect with a new snapshot, command result idempotency, and graceful shutdown.
- [ ] Implement tokio/tokio-tungstenite networking and bounded broadcast queues.
- [ ] Write failing supervisor tests using a short-lived helper process, then implement three-strike exponential restart and shutdown cleanup.
- [ ] Run `cargo test -p deepx-companion`.

### Task 4: Tauri registry integration

**Files:**
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/registry.rs`
- Create: `crates/deepx-tauri/src-tauri/src/agent_bridge/interaction.rs`
- Create: `crates/deepx-tauri/src-tauri/src/agent_bridge/companion_host.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs`

**Interfaces:**
- One `dispatch_agent_event(seed, generation, Agent2Ui)` path updates activity, replay, pending interactions, Tauri events, and Companion events.
- Existing permission/ask/plan Tauri commands and Companion commands call the same coordinator-backed host functions.

- [ ] Write failing tests for Agent2Ui-to-companion filtering and state mapping, including thinking, tools, compact, waiting-user, completed, error, and disconnected states.
- [ ] Extract coordinator-backed response functions and prove simultaneous Tauri/pet responses write exactly one `Ui2Agent` frame.
- [ ] Initialize the hub after `AgentRegistry`, start the configured pet executable, focus the Tauri window/session on `focus_session`, and shut down the hub/pet on application exit.
- [ ] Run `cargo test -p deepx-proto`, `cargo test -p deepx-companion`, and `cargo check -p deepx-tauri --tests`.

### Task 5: DeepX Pet client and state/HUD path (A)

**Files:**
- Create: `E:/clawd-on-desk/src/deepx-companion-client.js`
- Create: `E:/clawd-on-desk/src/deepx-event-adapter.js`
- Modify: `E:/clawd-on-desk/src/main.js`
- Modify: `E:/clawd-on-desk/package.json`

**Interfaces:**
- Client consumes endpoint/token-file/parent-pid launch arguments and emits normalized session snapshots and events.
- Adapter maps DeepX activity to existing `updateSession()` states without starting the original HTTP server or monitors.

- [ ] Write failing Node tests for authentication options, snapshot replacement, stale sequence rejection, reconnect, DeepX state mapping, and parent shutdown.
- [ ] Implement the client and adapter, then add a DeepX-only bootstrap path that skips server, hooks, agent monitors, remote bridges, mobile bridge, and updater.
- [ ] Rename product/app id/user-data namespace to DeepX Pet while retaining license and asset notices.
- [ ] Run the new tests plus state, HUD, renderer, theme, drag, and mini-mode suites.

### Task 6: Permission, ask_user, and plan review (B/C)

**Files:**
- Create: `E:/clawd-on-desk/src/deepx-interaction-presenter.js`
- Modify: `E:/clawd-on-desk/src/permission.js`
- Modify: `E:/clawd-on-desk/src/main.js`

**Interfaces:**
- Presenter accepts protocol-neutral permission/ask/plan payloads and returns `interaction_response` commands.
- `interaction_resolved` closes matching bubbles regardless of which UI won.

- [ ] Write failing tests for permission allow/deny, batch ask answers/dismiss, plan approve/reject feedback, duplicate clicks, remote resolution, and disconnect-without-decision.
- [ ] Extract reusable bubble presentation from HTTP response ownership and connect it to the WebSocket client.
- [ ] Verify DND/hidden behavior never invents an approval and Tauri remains the fallback.
- [ ] Run focused interaction tests and all non-environment-blocked Clawd tests.

### Task 7: Hardening and acceptance audit

**Files:**
- Modify only files required by failing acceptance tests.

- [ ] Add static tests proving DeepX mode cannot start hook installers, log monitors, HTTP state/permission routes, remote approval, PWA, or updater.
- [ ] Run Rust gates: `cargo test -p deepx-proto`, `cargo test -p deepx-companion`, `cargo check -p deepx-tauri --tests`, `cargo fmt --check`, and `git diff --check`.
- [ ] Run Clawd gates: new Companion tests, `npm test`, `npm run build`, and `git diff --check`; classify any pre-existing Electron-runtime failures separately.
- [ ] Manually launch DeepX with `DEEPX_PET_EXE`, verify status/HUD, permission, ask_user, plan review, pet crash/restart, DeepX shutdown, transparent drag, multi-display placement, mini mode, and sounds.
- [ ] Verify both repositories have no unintended hooks/config writes, no orphan processes, no copied AGPL assets in DeepX, and no push/release artifacts.
