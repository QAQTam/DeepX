# PLAN: Daemon → Message Gateway Server

## Goal

Upgrade `deepx-daemon` from a process manager to a full message gateway.
Frontend reconnects after crash/hot-reload without losing agent state.
Cross-platform transport (Unix + Windows) via TCP loopback.

Reference architectures: Codex `app-server` + OpenClaw `gateway`.

## Status (2026-07-05)

```
✅ Phase 1: frontend reader threads     ✅ Phase 3: ring buffer
✅ Phase 2: Tauri + TUI reconnect       ✅ Protocol: SessionState + BufferedEvents
⏸️ Windows named pipe (windows-sys 不暴露 API)
⏳ Phase 4: TCP loopback transport
⏳ Phase 5: Snapshot unification
⏳ Phase 6: Frame type layering (optional)
```

## Phase 4: TCP Loopback Transport

Replace Unix socket + Windows named pipe stub with `std::net::TcpListener`/`TcpStream`
on `127.0.0.1`. Zero new dependencies — `std::net` is cross-platform.

### 4.1 Transport rewrite (`daemon/transport.rs`)

```
删: #[cfg(unix)] pub mod unix { UnixListener, UnixStream }
删: #[cfg(windows)] pub mod win { stub }
加: TcpListener::bind("127.0.0.1:0")?  // random port
加: 端口号写入 data_dir/deepxd.port 文件
加: TcpStream::connect(port) 客户端连接
加: 4-byte LE length prefix + JSON frame 格式不变
```

### 4.2 Path → Port (`daemon/lib.rs`)

```rust
// Before
pub fn socket_path() -> PathBuf { data_dir().join("deepxd.sock") }
// After
pub fn port_path() -> PathBuf { data_dir().join("deepxd.port") }
pub fn read_port() -> Option<u16> { ... }
pub fn write_port(port: u16) { ... }
```

### 4.3 Client adapters

- `agent_bridge.rs`: `UnixStream::connect(socket_path)` → `TcpStream::connect(addr)`
- `terminalui/lib.rs`: same change
- `main_loop.rs`: remove `#[cfg(unix)]` / `#[cfg(windows)]` branches — single code path

### 4.4 Benefits

- Windows daemon works immediately (no `windows` crate needed)
- Same binary, same protocol, same port file on all platforms
- Future: swap `TcpStream` → WebSocket for browser access (no protocol change needed)

## Phase 5: Snapshot Unification

Merge `SessionState` + `BufferedEvents` into a single `Snapshot` message,
following OpenClaw's pattern. On reconnect, frontend receives one snapshot
containing full state + missed events + version counter.

### 5.1 Protocol (`agent_protocol.rs`)

```rust
// Before (two separate variants)
Agent2Ui::SessionState { seed, turns, tokens_used, context_limit, seq_id }
Agent2Ui::BufferedEvents { events, from_seq, to_seq }

// After (unified)
Agent2Ui::Snapshot {
    seed: String,
    turns: Vec<TurnData>,
    tokens_used: u32,
    context_limit: u32,
    buffered_events: Vec<Agent2Ui>,  // events missed during disconnect
    seq_id: u64,                      // current monotonic sequence
    has_more: bool,                   // ring buffer wrapped (some events lost)
}
```

### 5.2 Daemon adaptation (`frontend.rs`)

`replay_buffered()` → `build_snapshot()`: construct snapshot from current state + ring buffer.

### 5.3 Frontend adaptation

Single handler for `Snapshot` replaces separate `SessionState` + `BufferedEvents` handlers.

## Phase 6: Frame Type Layering (Optional)

Separate `Agent2Ui` into three frame types, following OpenClaw's pattern:

```rust
// Current: all in one enum
Agent2Ui { TurnStart, RoundDelta, ..., SessionState, Done, Error, ... }

// Proposed: layered
RequestFrame  { id, method, params }     // → expects ResponseFrame
ResponseFrame { id, result, error }      // ← reply to RequestFrame
EventFrame    { type, payload }          // unidirectional broadcast
```

Benefit: enables request/response correlation (e.g., `cmd_send_message` → `TurnEnd`),
simpler frontend dispatch, future extensibility.

Defer unless needed — current flat enum works fine for local use.

## Risk

| Risk | Mitigation |
|------|------------|
| TCP port collision | Use port 0 (OS assigns random free port), persist in `.port` file |
| Port file stale after crash | Daemon deletes `.port` on startup; client retries with timeout |
| Other localhost processes | Bind 127.0.0.1 (loopback only, not 0.0.0.0) |

## Estimated Effort

| Phase | Files | Lines |
|-------|-------|-------|
| 4.1 Transport rewrite | `transport.rs` | +40 / -90 |
| 4.2 Path→Port | `lib.rs` | +20 / -5 |
| 4.3 Client adapters | `agent_bridge.rs`, `lib.rs`(TUI), `main_loop.rs` | +30 / -50 |
| 5 Snapshot unification | `agent_protocol.rs`, `frontend.rs`, client adapters | +25 / -30 |
| 6 Frame layering | deferred | — |
| **Total** | **6** | **+115 / -175** |
