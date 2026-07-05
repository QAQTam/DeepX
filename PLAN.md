# PLAN: Daemon → Message Broker Server

## Goal

Upgrade `deepx-daemon` from a process manager to a full message broker.
Frontend reconnects after crash/hot-reload without losing agent state.

## Current State vs Target

```
BEFORE (进程管理器)                          AFTER (消息代理)
┌──────────┐  ┌──────────┐                 ┌──────────┐  ┌──────────┐
│  Tauri   │  │   TUI    │                 │  Tauri   │  │   TUI    │
│ stdin ───┼──┼── stdout │                 │ socket ──┼──┼─ socket  │
└────┬─────┘  └────┬─────┘                 └────┬─────┘  └────┬─────┘
     │              │                           │              │
     │  agent子进程   │                           │  deepxd 消息代理 │
     │  (独占stdin)  │                           │  ┌──────────┐  │
     │              │                           │  │ ring buf │  │
     ▼              ▼                           │  │ per seed │  │
  agent子进程      agent子进程                    │  └────┬─────┘  │
  (断连=丢会话)    (断连=丢会话)                   │       │        │
                                                  │  agent子进程   │
                                                  │  (server持有   │
                                                  │   stdin/stdout)│
                                                  └───────────────┘
```

## 缺口（已完成 80% 的骨架）

| # | 缺口 | 文件 | 状态 |
|---|------|------|------|
| 1 | 前端 reader 线程 | `daemon/main_loop.rs` | `FrontendManager::handle_frame` 已写好，无线程调用 |
| 2 | Windows 命名管道 | `daemon/transport.rs` | 仅有 Unix socket，Win 报 `not yet supported` |
| 3 | 事件缓冲 ring buffer | `daemon/frontend.rs` | broadcast 直写 socket，断连丢事件 |
| 4 | Session 恢复协议 | `deepx-proto/agent_protocol.rs` | 无序列号/恢复机制 |
| 5 | Tauri 重连逻辑 | `agent_bridge.rs` | daemon 不可用时 fallback 子进程 |
| 6 | TUI 重连逻辑 | `terminalui/lib.rs` | 同上 |

## Phase 1: 补全 Server 核心（gap 1+2+3）

### 1.1 前端→Server 读取线程 (`daemon/main_loop.rs`)

每个前端连接 spawn reader 线程，读 `FrontendToDaemon` 帧，调 `frontends.handle_frame()` 路由到 agent。
帧格式沿用现有 4 字节 LE 长度前缀 + JSON。

### 1.2 Windows 命名管道 (`daemon/transport/windows.rs`)

参考 Unix socket 的 `bind/accept/connect` API，用 Windows Named Pipe (`\\.\pipe\deepxd`) 实现等价功能。
`socket_path()` 已返回 `deepxd.pipe`，直接使用。

### 1.3 事件缓冲 (`daemon/frontend.rs`)

每 seed 维护 ring buffer（容量 256），`broadcast()` 先写 buffer 再写 socket。
前端断连标记 `disconnected`，重连时先发送缓冲事件（带 `seq_id`），再切换到实时流。

### 1.4 Session 恢复协议 (`deepx-proto/agent_protocol.rs`)

新增 Agent2Ui 变体：
- `SessionState { seed, turns, context_limit, seq_id }` — 重连时发送完整状态
- `BufferedEvents { events: Vec<Agent2Ui>, seq_id }` — 追赶缓冲事件
- 每个事件加 `seq_id: u64` 单调递增序列号

## Phase 2: 前端适配（gap 5+6）

### 2.1 Tauri agent_bridge 适配

- 移除 `cmd_resume_session` 的 "always send ResumeSession" hack
- Reader thread 改为读 daemon 帧（已有，需确认 seq_id 处理）
- Writer 通过 daemon 发送（已有 `try_send_via_daemon`）
- 重连时优先 daemon→fallback 子进程→fallback error

### 2.2 TUI spawn_agent_subprocess 适配

- 移除 fallback 子进程路径（或保留为 daemon 不可用时的后备）
- Reader/writer 走 daemon socket
- 重连时接收 SessionState 恢复 turns

## Phase 3: 清理 fallback 路径

- 子进程 spawn 路径标记为 deprecated
- Windows 上 daemon 可用后，移除 `CREATE_NO_WINDOW` 子进程 spawn
- `AgentRegistry` 在 Tauri 侧仅作为 daemon 不可用时的后备

## 风险

| 风险 | 缓解 |
|------|------|
| Windows 命名管道权限问题 | 仅允许当前用户连接（`PIPE_ACCESS_INBOUND` + `FILE_FLAG_FIRST_PIPE_INSTANCE`） |
| ring buffer 内存占用 | 256 事件 × ~2KB ≈ 512KB/seed，可接受 |
| 旧前端与新版 daemon 协议不兼容 | 前端先检查 `SessionState` 变体是否存在，不存在则 fallback 旧行为 |
| daemon 崩溃后所有 session 丢失 | daemon 启动时扫描 sessions_dir，重新 spawn 上次活跃的 agent |

## 估算

| Phase | 文件 | 行数 |
|-------|------|------|
| 1.1 前端 reader | 1 | +30 |
| 1.2 Windows 管道 | 2 | +60 |
| 1.3 事件缓冲 | 1 | +40 |
| 1.4 恢复协议 | 1 | +25 |
| 2.1 Tauri 适配 | 1 | +15 / -10 |
| 2.2 TUI 适配 | 1 | +15 / -10 |
| 3 清理 | 2 | -20 |
| **合计** | **9** | **~+185 / -40** |
