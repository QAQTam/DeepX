# PLAN: Daemon → Message Gateway Server → Audit-Ready Agent

## Goal

Upgrade `deepx-daemon` from a process manager to a full message gateway.
Frontend reconnects after crash/hot-reload without losing agent state.
Cross-platform transport (Unix + Windows) via TCP loopback.

Reference architectures: Codex `app-server` + OpenClaw `gateway`.

## Status (2026-07-05)

```
✅ Phase 1: frontend reader threads     ✅ Phase 3: ring buffer
✅ Phase 2: Tauri + TUI reconnect       ✅ Protocol: SessionState + BufferedEvents
✅ Phase 4: TCP loopback transport      ✅ Port file helpers
✅ Phase 5: Snapshot unification      ⏳ Phase 6: Frame type layering (optional)
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

## Phase 7: Audit Trail & Compliance

### 7.0 现状审计

**已有能力：**

| 组件 | 已有 | 位置 |
|------|------|------|
| 工具执行元数据 | `ToolExecMeta`（name, elapsed_ms, output_size, success, args_summary） | `manager.rs:17` |
| 工具统计 | `ToolStats`（calls_total, failures, files_read, files_written） | `manager.rs:34` |
| 实时审计事件 | `Agent2Ui::AuditRecord`（tool_name, result_summary, success）发送到前端 | `bridge.rs:452`, `lib.rs:1386` |
| 前端展示 | TUI `activity_log`（50 条环形缓冲），Tauri StatusPanel | `mod.rs:1251`, `StatusPanel.tsx:85` |
| 危险命令拦截 | `is_danger_command` + `classify_execution` | `safety.rs:29` |
| 审计参数摘要 | `audit_args_summary()`（截取 path/command/pattern 等关键字段） | `manager.rs:321` |

**缺口：**

| # | 缺口 | 影响 | 难度 |
|---|------|------|------|
| 1 | **无持久化存储** — AuditRecord 纯内存（TUI 50 条环形 / Tauri React state），页面刷新即丢失 | 无法满足"事后追溯"的合规要求 | **低** |
| 2 | **无用户身份** — daemon 连接无 user_id，多个前端共用同一会话无区分 | 无法确定"谁"触发了工具调用 | **中** |
| 3 | **无完整参数记录** — 仅存 args_summary（截断 80 字符），无法完整复现操作 | 无法做精确审计 | **低** |
| 4 | **exec 工具无命令记录** — safety 仅拦截已知危险模式，不记录实际执行的完整命令 | 最大风险点缺乏审计 | **低** |
| 5 | **无 prompt 注入检测** — 工具调用可能来自恶意 API 响应，当前无条件信任 | 国企合规红线 | **高** |
| 6 | **无导出能力** — 无法生成可提交的审计报告 | 审核流程无法闭环 | **低** |

### 7.1 审计持久化（gap 1+3+4，低难度，~80 行）

```rust
// deepx-tools/src/audit.rs (新增)
struct AuditEntry {
    timestamp: u64,           // UNIX 秒
    session_seed: String,     // 会话标识
    user_id: String,          // 用户标识（暂时用 frontend_conn_id）
    tool_name: String,        // 工具名
    action: String,           // 操作（read/write/exec/delete...）
    args: serde_json::Value,  // 完整参数（替换 args_summary）
    result_summary: String,   // 结果摘要（首行，120 字符）
    elapsed_ms: u64,          // 执行耗时
    success: bool,            // 成功/失败
    files_affected: Vec<String>, // 受影响的文件列表
}
```

存储方案：`sessions/{seed}/audit.jsonl` — 每行一个 JSON，增量追加。与 `messages.jsonl` 同目录，便于备份。

改动点：
- `manager.rs`: `ToolExecMeta.args` 改为存储完整 `serde_json::Value`
- `bridge.rs`: `execute_tools_parallel` 写完 AuditRecord 后追加写入 `audit.jsonl`
- `safety.rs`: exec 工具执行前写入完整命令到 audit（gap 4）

### 7.2 用户身份（gap 2，中难度，~100 行）

```
当前: daemon 连接 (TcpStream) → conn_id (usize) 
目标: daemon 连接 → user_id (String) + conn_id
```

在 `FrontendToDaemon` 协议中新增可选字段：
```rust
pub struct FrontendToDaemon {
    pub seed: String,
    #[serde(default)]
    pub user_id: String,    // 新增：由前端在握手时设置
    #[serde(flatten)]
    pub frame: Ui2Agent,
}
```

前端（Tauri/TUI）初始化时从 `ConfigStore` 读取 `user_id`（默认 `whoami::username()`），在 daemon 连接后首次 `Subscribe` 时携带。

### 7.3 Prompt 注入检测（gap 5，高难度，~200 行）

两条防线：

**A. LLM 响应完整性校验**（被动检测）
- `deepx-gate/src/openai.rs` 中 `chat_stream_openai` 返回的每个 `StreamEvent` 记录到环形缓冲区
- 当检测到 `tool_calls` 出现在非预期的流位置时（如 LLM 在 `answer` 阶段突然插入 tool_call），标记可疑

**B. 工具调用白名单**（主动拦截）
- `safety.rs` 新增 `is_tool_allowed(tool_name, args)` 
- 只允许注册过的 tool name + action 组合
- 未知 tool call → `Agent2Ui::ToolNotice { level: "error", message: "Blocked unregistered tool: X" }`

**C. 文件边界检查**（补充）
- file_mutate / file_query 工具检查目标路径是否在 workspace 内
- workspace 外路径 → SafetyVerdict::Block

### 7.4 PLAN Review 工具（新功能，~200 行）

```
PLAN.md (Git 管理)              Tauri PLAN Review 面板
───────────────────────          ──────────────────────────
## Phase 7                        [ ] Phase 7.1  审计持久化
### 7.1 审计持久化                [ ] Phase 7.2  用户身份
...                               [ ] Phase 7.3  注入检测
                                  [ ] Phase 7.4  PLAN工具
                                  [ Ask ] [ Approve ] [ Reject ]
```

**格式扩展** — PLAN.md 每个任务项支持 YAML front-matter 风格元数据注释：

```markdown
<!-- meta: { id: "P7.1", status: "approved", approved_by: "主管", approved_at: 1700000000 } -->
### 7.1 审计持久化（gap 1+3+4，低难度，~80 行）
```

**前端实现**：
- `parse_plan_md()` — 解析 PLAN.md，提取 `###` 标题为任务项，`<!-- meta: ... -->` 为元数据
- `PlanReviewPanel.tsx` — 新组件，两栏布局（左：Markdown 预览，右：任务列表 + 审批操作）
- `cmd_plan_action` — 新 Tauri command，写入审批状态到 PLAN.md

**审阅流程**：
1. 开发者写完 PLAN.md → `git push`
2. 主管打开 Tauri → PLAN Review 面板 → 逐项点 Approve/Reject/Ask
3. 审批结果自动写回 PLAN.md 注释 → `git commit`
4. 开发者看到每个条目的审批状态，Reject 的条目可重新提交

### 7.5 AgentFS 集成（可选加速器）

AgentFS 三层 API 与当前架构的对应：

| AgentFS API | DeepX 替代 | 收益 |
|---|---|---|
| `fs.readFile/writeFile` | `read_file`/`write_file` 工具 | 自动审计 + 沙箱隔离 |
| `kv.set/get` | `memory` 工具（文件存储） | 结构化查询 + 快照 |
| `toolcall` 时间线 | `audit.jsonl`（我们自己实现） | SQL 查询审计历史 |

**引入路径**：
1. `cargo add agentfs-sdk`（纯 Rust SDK，不依赖 FUSE）
2. `read_file`/`write_file` 内部切换为 AgentFS 读写（对外接口不变）
3. 审计存储从 `audit.jsonl` 升级为 AgentFS `toolcall` 表
4. Windows 上无需 FUSE mount，纯 API 路径即可

**不引入的风险**：无。AgentFS 底层是 Turso（SQLite 兼容纯 Rust），即使将来移除 AgentFS SDK，数据仍在 SQLite 文件中可读。

### 7.6 工作量评估

| 项 | 难度 | 行数 | 文件 |
|----|------|------|------|
| 7.1 审计持久化 | 低 | +80 | `audit.rs`(新), `bridge.rs`, `manager.rs` |
| 7.2 用户身份 | 中 | +100 | `agent_protocol.rs`, `frontend.rs`, `config.rs` |
| 7.3 注入检测 | 高 | +200 | `safety.rs`, `openai.rs`, `file_*.rs` |
| 7.4 PLAN Review | 中 | +200 | `PlanReviewPanel.tsx`(新), `agent_bridge.rs` |
| 7.5 AgentFS | 中 | +150 | `Cargo.toml`, `file_*.rs`, `audit.rs` |
| **合计** | — | **+730** | **10** |

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
