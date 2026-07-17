# DeepX-Fork 全面注释修订 — 设计文档

**日期**: 2026-07-17
**版本**: 1.0
**状态**: 待审核

---

## 1. 目标

为 DeepX-Fork workspace 中 12 个 crate 的所有公开 API 和关键内部逻辑添加一致性文档注释，不改变任何代码行为。

---

## 2. 范围

### 覆盖范围 (全面注释)
- 所有 `pub` 符号：struct, enum (含 variant), trait, fn, mod, type, const
- 所有 struct 字段
- 所有非显而易见的私有函数
- 复杂算法 / 状态机转换 / 边界条件
- `unsafe` 块（需 `// SAFETY:` 注释）
- `//!` crate/mod 级模块文档

### 不覆盖
- 纯简单的 getter/setter / accessor
- 编译器自动生成的实现 (derive)
- 测试代码
- SKILL.md 文件（这些是数据文件，不是 Rust 源码）

---

## 3. 注释规范

### 3.1 模块级 (`//!`)

每个 `lib.rs` 或 `mod.rs` 必须有模块文档块，格式：

```rust
//! crate-name — 一句话定位
//!
//! 一段描述核心功能。可多段。
//!
//! ## 关键概念 (可选)
//!
//! ## 使用示例 (可选)
```

### 3.2 公开 API (`///`)

**Struct**:
```rust
/// Brief purpose (one line).
///
/// Longer description if the struct has non-obvious behavior or
/// lifecycle rules (e.g. must call `init()` before use).
#[derive(...)]
pub struct MyStruct {
    /// What this field stores, and any constraints
    /// (e.g. non-empty, must be a valid path).
    pub field_name: Type,
}
```

**Enum**:
```rust
/// What this enum represents.
pub enum Outcome {
    /// When this variant is produced and what it means.
    Continue,
    /// When this variant is produced and what it means.
    Stop,
}
```

**Function**:
```rust
/// Brief: what this function does.
///
/// Longer description of behavior, side effects, algorithm.
///
/// # Arguments
/// * `param` - What this parameter is for (if non-obvious from name).
///
/// # Returns
/// Description of the return value.
///
/// # Errors
/// When this function returns an error.
///
/// # Panics
/// Any condition that causes a panic (hopefully none).
///
/// # Safety (if unsafe)
/// Preconditions for calling this function.
pub fn do_thing(param: &str) -> Result<(), Error> { ... }
```

**Trait**:
```rust
/// What this trait abstracts.
///
/// # Implementors
/// Who should implement this trait, and any constraints.
pub trait Engine {
    /// What this method does.
    ///
    /// # Returns
    /// - `Some(Outcome)` — handled
    /// - `None` — pass to next engine
    fn try_handle(&mut self, ctx: &mut Ctx, cmd: &Cmd) -> Option<Outcome>;
}
```

### 3.3 行内注释 (`//`)

用于解释**非显而易见**的逻辑。不注释自明的代码。

```rust
// State machine: Normal → Suspended → Resumed → Complete
// Suspended turn can only be resumed via PermissionResolved.
self.suspended = Some(state);

// Defensive clone: the tree is backed by an Arena;
// we need ownership because the parent Tree borrows from the arena
// while the returned Leaf outlives it.
let leaf = tree.root().clone();
```

### 3.4 Unsafe 块

每个 `unsafe` 块必须有 `// SAFETY:` 注释，说明满足的预条件：

```rust
// SAFETY: ptr was allocated by Vec::with_capacity(n) and we just
// verified that i < n, so the pointer is valid for writes.
unsafe { ptr.add(i).write(val); }
```

### 3.5 禁止模式

- ❌ 不写 "TODO" / "FIXME" 注释（除非已有待修 bug）
- ❌ 不写 "Obvious" / "self-explanatory" 之类废话
- ❌ 不翻译代码的功能 — 注释解释**为什么**，代码说明**怎么做的**
- ❌ 不复制函数签名到注释

---

## 4. 执行策略

自底向上逐 crate，按依赖图顺序执行：

```
阶段 0: 注释规范 (本文档)
阶段 1: deepx-skills → deepx-types      (叶子 crate, 零依赖)
阶段 2: deepx-proto → deepx-config → deepx-session  (协议/配置)
阶段 3: deepx-gate → deepx-message → deepx-tools → deepx-subagent  (核心引擎)
阶段 4: deepx-msglp → deepx-tauri → deepx-gate-testui  (编排/UI)
```

### 每阶段检查点
1. `cargo check --workspace` — 确保编译通过
2. `cargo test -p <crate>` — 相关测试通过
3. 人工抽查 — 确认注释风格一致

---

## 5. 各 Crate 详细任务

### 阶段 1: 叶子层

#### deepx-skills (1 文件, ~950L)
- `SkillScope` — 各 variant 含义
- `SkillMetadata` — 各字段含义
- `DiagnosticSeverity` — 各 variant
- `SkillCatalog`, `SkillActivation`, `SkillResource` — 字段文档
- `discover()` — 扫描逻辑说明
- `load_named()`, `load()` — 加载流程
- `render_catalog()`, `render_activation()` — 渲染规则
- `explicit_mentions()` — 匹配规则
- `SkillCatalogSnapshot`, `SkillEffect`, `SkillBodyChange` — 字段
- 内部辅助函数 — 关键逻辑说明

#### deepx-types (9 文件)
- `provider.rs` — `ProviderSpec`, `EndpointSpec`, `UserSendMode`, `ThinkingParamMode`, `CacheTokenField` 完整字段文档
- `message.rs` — `ContentBlock` 各 variant, `Message`, `ToolCall`, `FunctionCall`
- `config.rs` — `PersistentConfig`, `PersistentSubagentConfig`, `PersistentDatabaseConfig`, `ProfileConfig`, `ConfigStore`, `BalanceInfo`
- `session.rs` — `SessionMeta`, `SkillSessionEntry`, `SkillSessionEntryState`, `SkillSessionStateV2`
- `tool_def.rs` — `ToolDef`, `ToolFunction`
- `arg.rs` — 各 parse 函数的参数格式说明
- `platform.rs` — 各路径函数的语义
- `token.rs` — `init_tokenizer`, `count_tokens`, `TokenBreakdown`
- `api_types.rs` — `UsageInfo`
- `state.rs` — `DebugLevel`

### 阶段 2: 协议/配置层

#### deepx-proto (1 文件, ~940L)
- `agent_protocol.rs` 全部类型
- `Ui2Agent` 各 variant — 带触发条件
- `Agent2Ui` 各 variant — 带含义 + 前端如何消费
- `SessionActivity`, `SessionActivityState` — 生命周期
- `RoundBlock`, `RoundData`, `TurnData` — 协议版本说明
- `AskMode`, `AskQuestion`, `AskAnswer` — 交互协议
- `Redacted` (在 lib.rs) — 已有文档，确认完整

#### deepx-config (4 文件)
- `config.rs` — `Config`, `SubagentConfig`, `DatabaseConfig` 字段
- `prompt.rs` — `full_system_prompt()` 构建逻辑
- `config_db.rs` — turso 双写逻辑已有好文档，补充方法级
- `registry.rs` — `all_providers()` 等查找函数

#### deepx-session (5 文件)
- `manager.rs` — `SessionManager` singleton 已有好文档，补充各方法
- `store/mod.rs` — JSonL 文件 I/O 函数的行为说明（原子性、错误处理）
- `store/turso_backend.rs` — `TursoBackend` 字段
- `session_meta.rs` — `SessionMeta`（可引用 deepx-types 中的定义）
- `migrate.rs` — `run()` 迁移逻辑

### 阶段 3: 核心引擎层

#### deepx-gate (4 文件)
- `lib.rs` — `chat_stream`, `chat_sync` 已有文档, 确认完整
- `openai.rs` — `chat_stream_openai` 和 `chat_sync_openai` 的 SSE 解析流程
- `tool_parser.rs` — XML/DSML 解析算法
- `types.rs` — `ProviderConfig`, `ProviderKind`, `StreamEvent`
- `guard.rs` — `content_guard` 安全规则

#### deepx-message (3 文件)
- `store.rs` — `Step`, `Turn`, `MessageStore` 状态机 + 方法
- `effect.rs` — `Effect`, `PendingTool`, `ToolExecRequest`, `ToolExecReport`
- `lib.rs` — 模块级文档

#### deepx-tools (30+ 文件)
- `lib.rs` — `ToolRisk`, `ToolHandler`, `ToolResult`, `ToolCallCtx`, `ToolEffect` 等核心类型
- `authorization.rs` — 权限模型 (已有部分文档，补全)
- `permission.rs` — 风险分类和信任文件夹
- `execution.rs` — 授权执行流程
- `manager.rs` — ToolManager 单例
- `registration.rs` — 工具注册
- 每个工具模块 (`file_query.rs`, `file_mutate.rs`, `git.rs`, `exec.rs`, `explore.rs`, `plan.rs`, `task.rs` 等) — `register()` 函数 + 各 handler 函数
- `runtime.rs` — 运行时上下文
- `ask_user.rs` — 交互式确认协议
- `audit.rs` — 审计日志
- `workspace.rs`, `file_cache.rs`, `file_state.rs`, `file_shared.rs`
- `process_inspect.rs`, `process_registry.rs`
- `agentfs_bridge.rs` — 桥接层
- `auth.rs` — 认证

#### deepx-subagent (1 文件)
- `register()` — 工具注册（已有好文档）
- `handle_spawn_subagent()` — 子进程生命周期

### 阶段 4: 编排/UI 层

#### deepx-msglp (12 文件)
- `agent.rs` — `AgentState` 字段, `PendingApproval`, `TurnResumeState`
- `lifecycle.rs` — `init_session`, `create_session`, `create_session_with_seed`
- `logger.rs` — `init_agent_logger`
- `skill_context.rs` — `SkillContextManager` 字段 + 方法
- `conflict.rs` — 冲突检测
- `dashboard.rs` — 仪表板构建
- `notification.rs` — `NotifyMessage`
- `toast_com.rs` (Windows) — COM 通知
- `new/loop_core.rs` — 私有方法 (1156L 中大量内部函数)
- `new/engine_tool.rs` — `BatchAdmission`, `PermissionDisposition` 等
- `new/engine_turn.rs` — `ResumeReason`, 继续/挂起逻辑
- `new/engine_session.rs` — session 生命周期管理
- `new/engine_input.rs` — 输入处理
- `new/engine_compact.rs` — 上下文摘要
- `new/engine_misc.rs` — 杂项命令
- `new/types.rs` — 已有好文档，确认完整
- `util.rs` — 工具函数

#### deepx-tauri (12 文件)
- `agent_bridge/commands/session.rs` — 15+ cmd 函数
- `agent_bridge/commands/config.rs` — 15+ cmd 函数
- `agent_bridge/commands/plan.rs` — 8 cmd 函数
- `agent_bridge/commands/git.rs` — 6 cmd 函数
- `agent_bridge/commands/permission.rs` — 4 cmd 函数
- `agent_bridge/registry.rs` — 已有好文档，补方法级
- `agent_bridge/activity.rs` — `SessionActivityTracker`
- `agent_bridge/platform.rs` — 平台检测
- `agent_bridge/util.rs` — 工具函数
- `lib.rs` — `run()` 入口

#### deepx-gate-testui
- 测试工具文档

---

## 6. 风险与约束

| 风险 | 缓解 |
|------|------|
| 意外改动代码逻辑 | 每次修改后 `cargo check`；只添加 `///` / `//!` / `//` 前缀行 |
| 注释与代码不同步 | 注释描述的是"行为意图"，不复制实现细节 |
| 编译失败 (格式/语法) | 每 crate 改完立即 `cargo check` |
| 工作量大导致半途而废 | 分阶段，每阶段独立可交付 |

---

## 7. 完成标准

- [ ] `cargo check --workspace` 零错误零警告
- [ ] `cargo test --workspace` 全部通过
- [ ] 所有 `pub` 符号有 `///` 文档
- [ ] 所有 crate `lib.rs` 有 `//!` crate 级文档
- [ ] 所有 `unsafe` 块有 `// SAFETY:` 注释
- [ ] 复杂算法/状态机有行内注释
