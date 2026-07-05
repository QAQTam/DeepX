# PLAN: DeepX — Audit-Ready Agent Platform

## Goal

v0.7.0: 告别 bug-fix 时代，引入审计链路 + OS 授权 + 合规过滤 + PLAN Review + Safety 分级 + AgentFS，从"能用的 agent"升级为"可审计的工作助手"。

## v0.4 → v0.6 回顾

```
127 commits, 其中 122 个 fix（96%）
  ├── 行为回归修复    13 次
  ├── 崩溃/栈溢出      5 次（0xc0000005）
  ├── PTY / 竞态       5 次
  ├── 通道死锁/阻塞    4 次
  ├── CJK 序列化       4 次
  └── 通用 fix        111 次
```

核心痛点：协议字段映射反复断裂。v0.6.0 `ts-rs` 自动生成 `.ts` 后回归大减。

## v0.7.0 Roadmap（一波流 — 全部 10 项同时交付原型，v0.7.1+ 只修 bug）

### 政策背景

| 条款 | 要求 | DeepX 对应 |
|------|------|-----------|
| 第 6 条 决策权限 | 区分用户授权 / 智能体自主决策 | Phase 7.2 OS PIN |
| 第 7 条 行为管控 | 可验证、可追溯 | Phase 7.1 审计 |
| 第 8 条 内生安全 | 密码防护、权限管理、行为控制 | Phase 7.2 + 7.3 + 7.5 |
| 第 9 条 供应链安全 | API 调用、扩展工具安全管理 | 工具注册白名单 |
| 第 11 条 分类分级 | 日常办公 = 低风险 | ✅ DeepX 定位一致 |
| 第 16 条 研发辅助 | 发展软件开发智能体 | ✅ DeepX 定位一致 |

《拟人化互动服务管理办法》第二条：工作助手不适用该办法。自觉对齐第八条（Phase 7.3）。

| Phase | 内容 | 难度 | 行数 |
|-------|------|------|------|
| **7.1** | 审计持久化（audit.jsonl + SHA-256 指纹） | 低 | +80 |
| **7.2** | OS PIN 授权（Windows CredUI + Linux PAM） | 中 | +120 |
| **7.3** | 合规内容过滤（system prompt + gate 关键词） | 中 | +100 |
| **7.4** | PLAN Review 工具（Tauri 审批面板） | 中 | +200 |
| **7.5** | Safety 分级（ToolRisk 四级 + 两层决策） | 中 | +120 |
| **7.6** | AgentFS 集成（`agentfs` crate：kv + tools 替代 memory + audit） | 中 | +150 |
| **7.7** | 工具 Schema 修复（多 action 独立暴露） | 低 | +30 |
| **7.8** | Daemon 心跳（Ping/Pong 健康检查） | 低 | +30 |
| **7.9** | exec 命令审计（完整命令写入 audit.jsonl） | 低 | +20 |
| **7.10** | Session 双库（JSONL + Turso Database 引擎） | 中 | +100 |
| **合计** | **10 项，v0.7.0 原型；v0.7.1+ 只修 bug** | — | **+950** |

---

### 7.0 现状审计

**已有：**

| 组件 | 位置 |
|------|------|
| `ToolExecMeta`（name, elapsed_ms, output_size, success, args_summary） | `manager.rs:17` |
| `Agent2Ui::AuditRecord` 实时推送前端 | `bridge.rs:452`, `lib.rs:1386` |
| TUI `activity_log` + Tauri `StatusPanel` | `mod.rs:1251`, `StatusPanel.tsx:85` |
| `is_danger_command` 危险命令拦截 | `safety.rs:29` |

**生命周期覆盖：**

```
用户输入 → 消息入队 → LLM请求 → LLM返回 → Tool解析 → 安全检查 → 工具执行 → 结果返回
   ❌         ❌         ❌         ❌         ❌         ⚠️         ✅         ⚠️
```

结论：只在执行点有审计，其余全无。

### 7.1 审计持久化（P0，低难度）

**不存 body，存指纹：**

```
❌ 旧 debug dump:  全量覆写，一次 100MB
✅ 新 audit.jsonl: JSONL 增量追加，一条 200 字节，SHA-256 指纹
```

```json
{"ts":1700000000,"user":"alice","tool":"exec","args_hash":"a1b2...","result":"ok","elapsed_ms":300,"files":["src/main.rs"]}
```

**变更：**
- 新增 `audit.rs`：`AuditEntry` 结构体 + `append_audit()`
- `bridge.rs`: 写完 `AuditRecord` 后调用 `append_audit()`
- `manager.rs`: `args` 存储完整 `serde_json::Value`

### 7.2 OS PIN 授权（P1，中难度）

| 平台 | API | 备注 |
|------|-----|------|
| Windows 10+ | `CredUIPromptForWindowsCredentials` | 政府版可用 |
| Linux | PAM `pam_authenticate` | 3 行 |

两阶段：会话级（agent 启动验证一次）→ 操作级（高危工具执行前弹框）

### 7.3 合规内容过滤（P1，中难度）

**A. System prompt 层：** 拒绝情感陪伴、心理咨询、诱导性询问

**B. Gate 层关键词预检（~50 行）：**

```rust
// deepx-gate/src/guard.rs
const BLOCKED: &[&str] = &["心理咨询", "情感陪伴", "自杀", "自残", "密钥", "密码", "token", "api_key"];
fn content_guard(input: &str) -> Result<(), String> { ... }
```

调用点：`handle_user_input` → `content_guard(&text)?`

### 7.4 PLAN Review 工具（P1，中难度）

Tauri 新组件 `PlanReviewPanel.tsx`：解析 PLAN.md → 逐条 Approve/Reject/Ask → 写回 HTML 注释元数据。

### 7.5 Safety 分级（P1，中难度，~120 行）

**A. 四级风险分类：**

```rust
pub enum ToolRisk {
    ReadOnly,       // read, list, search, diff, explore, web_fetch, git_log/status
    Write,          // write, edit, edit_diff, move, copy
    Destructive,    // delete, exec, git add/commit
    Administrative, // process kill, memory global_write
}
```

每个 handler 注册时标注 `risk: ToolRisk::Write`，不再写 `safety: fn`。

**B. 两层决策：**

```
层 1: 工作区边界
  ├─ ReadOnly       → Allow
  ├─ Write          → 工作区内 Allow，工作区外 RequireAuth
  ├─ Destructive    → 工作区内 RequireAuth，工作区外 Block
  └─ Administrative → 一律 RequireAuth

层 2: 风险默认
  ├─ ReadOnly       → Allow
  ├─ Write          → Allow
  ├─ Destructive    → RequireAuth
  └─ Administrative → RequireAuth
```

**C. 29 个 handler 风险映射：**

| 工具 | 风险 | 工作区内 | 工作区外 |
|------|------|---------|---------|
| file/read, list, search, diff | Read | Allow | Allow |
| explore/scan | Read | Allow | Allow |
| web/fetch, search | Read | Allow | Allow |
| git/log, diff, status, show | Read | Allow | Allow |
| file/write, edit, edit_diff | Write | Allow | PIN |
| file/move, copy | Write | Allow | PIN |
| memory/read, write | Write | Allow | PIN |
| task/* | Write | Allow | PIN |
| ask_user | Write | Allow | PIN |
| file/delete | Destructive | PIN | Block |
| exec/run | Destructive | PIN | Block |
| git/add, commit | Destructive | PIN | Block |
| process/kill | Admin | PIN | PIN |
| memory/global_write | Admin | PIN | PIN |

**SafetyVerdict 精简为三种：**

```rust
pub enum SafetyVerdict {
    Allow,
    RequireAuth { reason: String },
    Block(String),
}
```

改动点：
- `lib.rs`: `ToolRisk` 枚举 + `ToolHandler.risk` 替换 `safety`（+20 行）
- `safety.rs` → `SafetyPolicy` + `evaluate()`（+80 行）
- `manager.rs` `handler.safety` → `POLICY.evaluate(handler.risk, ...)`（-1 +1 行）
- 29 个注册点 `safety:` → `risk:`（每个 -1 +1 行）

### 7.6 AgentFS 集成（P2，中难度）

| AgentFS API | DeepX 替代 | 收益 |
|---|---|---|
| `fs.readFile/writeFile` | `read_file`/`write_file` | 自动审计 + 沙箱 |
| `kv.set/get` | `memory` 工具 | 结构化查询 |
| `toolcall` 时间线 | `audit.jsonl` | SQL 查询审计 |

底层 Turso（SQLite 兼容纯 Rust），不引入风险。

### 7.7 工具 Schema 修复（P0，低难度）

`all_defs()` 多 action 合并 schema → 改为每个 `(name, action)` 独立暴露 `{name}_{action}`。

### 7.8 Daemon 心跳（P1，低难度）

```
frontend 每 10 秒 → Ui2Agent::Ping → daemon → Agent2Ui::Pong
3 次无响应 → 触发重连 + Snapshot
```

### 7.9 exec 命令审计（P1，低难度）

`exec.rs` 入口处追加 `append_audit()` 写入完整命令字符串。

### 7.10 Session 双库（P2，中难度，~100 行）

**目标：** `deepx-session` 增加 Turso Database 后端（`turso` crate），与 JSONL 并行写入，逐步迁移。

**策略：JSONL 为主，Turso 为镜像。** 每条写入先过 JSONL（已有逻辑不变），成功后 mirror 到 Turso local `.db`。

**A. 依赖：**

```toml
# crates/deepx-session/Cargo.toml
[features]
turso-backend = ["dep:turso", "dep:tokio"]

[dependencies]
turso = { version = "0.12", optional = true }
tokio = { version = "1", features = ["rt"], optional = true }
```

**B. 新增 `store/turso_backend.rs`：**

```rust
pub struct TursoBackend {
    db: turso::Database,
}

impl TursoBackend {
    pub fn open(path: &str) -> Result<Self>;
    pub fn init_tables(&self) -> Result<()>;
    pub fn upsert_meta(&self, seed: &str, meta: &SessionMeta) -> Result<()>;
    pub fn insert_message(&self, seed: &str, msg: &Message) -> Result<()>;
    pub fn load_messages(&self, seed: &str) -> Result<Vec<Message>>;
    pub fn load_meta(&self, seed: &str) -> Result<Option<SessionMeta>>;
    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>>;
    pub fn delete_session(&self, seed: &str) -> Result<()>;
}
```

> async → sync 桥接：`TursoBackend` 内部用 `tokio::runtime::Runtime::new().block_on(...)` 桥接。

**C. SessionManager 修改：**

```rust
pub struct SessionManager {
    data_dir: PathBuf,
    db: Option<TursoBackend>,  // NEW: optional Turso backend
}
```

`init()` 签名增加 `db_url: Option<&str>`，`save_append` / `save_full` / `update_meta` / `delete` 在每个 JSONL 写入后追加：

```rust
if let Some(db) = &self.db {
    let _ = db.upsert_meta(seed, meta);  // best-effort, never fail JSONL
}
```

**D. Config 扩展：**

```toml
# config.toml
[database]
url = "data/sessions.db"   # 本地 Turso 文件；生产用 Turso Cloud URL
enabled = true
```

**E. 迁移路径：**

```
v0.7.0:  JSONL (primary) + Turso local .db (mirror)
v0.8.0:  Turso 达到同等完整性 → JSONL 降级为 backup
v0.9.0:  移除 JSONL, Turso 唯一存储
```

改动点：
- `deepx-session/Cargo.toml`: +5 行 (features + turso + tokio)
- `deepx-session/src/store/turso_backend.rs`: +60 行 (新建)
- `deepx-session/src/manager.rs`: +20 行 (Option<TursoBackend> 整合)
- `deepx-config/src/config.rs`: +15 行 (database section)

## 工作量

| Phase | 难度 | 行数 | 文件 |
|-------|------|------|------|
| 7.1 审计持久化 | 低 | +80 | `audit.rs`(新), `bridge.rs`, `manager.rs` |
| 7.2 OS PIN 授权 | 中 | +120 | `auth.rs`(新), `safety.rs`, `Cargo.toml` |
| 7.3 合规过滤 | 中 | +100 | `guard.rs`(新), `lib.rs`(msglp), `config.rs` |
| 7.4 PLAN Review | 中 | +200 | `PlanReviewPanel.tsx`(新), `agent_bridge.rs` |
| 7.5 Safety 分级 | 中 | +120 | `safety.rs`, `manager.rs`, `lib.rs`(tools), 29 注册点 |
| 7.6 AgentFS | 中 | +150 | `agentfs_bridge.rs`(新), `memory.rs`, `bridge.rs`, `Cargo.toml` |
| 7.7 工具 Schema | 低 | +30 | `manager.rs` |
| 7.8 Daemon 心跳 | 低 | +30 | `agent_protocol.rs`, `main_loop.rs` |
| 7.9 exec 命令审计 | 低 | +20 | `exec.rs` |
| 7.10 Session 双库 | 中 | +100 | `deepx-session/Cargo.toml`, `store/turso_backend.rs`(新), `manager.rs`, `config.rs` |
| **合计** | — | **+950** | **14** |

## Risk

| Risk | 缓解 |
|------|------|
| PIN 弹框 headless 不可用 | SSH session 回退 token 文件 |
| `windows` crate 编译慢 | feature flag 隔离 |
| 合规关键词误杀 | 整词匹配 + 白名单 |
| audit.jsonl 增长 | 上限 10MB，超出触发压缩 |
