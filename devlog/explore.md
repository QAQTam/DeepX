# DeepX-Fork 项目探索报告

> **生成时间**: 2026-07-12 13:53 UTC+8  
> **版本**: v0.8.0  
> **语言**: Rust (edition 2024)  
> **构建系统**: Cargo workspace (11 crates)  
> **前端**: Tauri v2 + React + TypeScript

---

## 目录

1. [项目概述](#1-项目概述)
2. [Crate 架构全景](#2-crate-架构全景)
3. [核心数据流](#3-核心数据流)
4. [关键类型定义](#4-关键类型定义)
5. [IPC 协议设计](#5-ipc-协议设计)
6. [LLM 网关实现](#6-llm-网关实现)
7. [消息存储与状态机](#7-消息存储与状态机)
8. [主循环驱动](#8-主循环驱动)
9. [工具生态系统](#9-工具生态系统)
10. [权限与安全引擎](#10-权限与安全引擎)
11. [会话管理](#11-会话管理)
12. [配置系统](#12-配置系统)
13. [前端架构](#13-前端架构)
14. [生产力特性](#14-生产力特性)
15. [测试与质量保障](#15-测试与质量保障)
16. [抽样核验与已知偏差](#16-抽样核验与已知偏差)
17. [Crate Debug 总控看板](#17-crate-debug-总控看板)
18. [Codex 与 DeepSeek 协作规范](#18-codex-与-deepseek-协作规范)
19. [任务 Prompt 模板](#19-任务-prompt-模板)
20. [工具调用修复追踪](#20-工具调用修复追踪)

---

## 1. 项目概述

DeepX-Fork 是一个 **AI 编程代理**（类似 Claude Code），采用 **Tauri v2 桌面应用** 承载，通过 **OpenAI 兼容的 HTTP 流式 API** 与 LLM 通信。核心理念是：用户给指令 → 代理自主执行（读代码、编辑文件、运行命令、Git 操作、Web 搜索）→ 返回结果。

### 关键数据
| 项目 | 值 |
|------|-----|
| 总 Rust 代码量 | ~19,352 行（2026-07-12 按 `*.rs` 物理行抽样统计） |
| Crate 数量 | 11 |
| 工具数量 | 28 个内置注册项 + 1 个可注入的 `spawn_subagent`；部分工具通过 `action` 暴露多项能力 |
| 支持 Provider | DeepSeek, Qwen, GLM, Kimi, MiMo, MiniMax, Doubao, OpenAI 等 8+ |
| 前端组件 | `components/` 下 23 个 TSX；前端共 25 个 TSX 文件 |
| 国际化 | 中/英双语 |

---

## 2. Crate 架构全景

```
┌─────────────────────────────────────────────────┐
│                 deepx-tauri                     │  ← 桌面应用 UI
│            (Tauri v2 + React/TS)                │
├─────────────────────────────────────────────────┤
│                  deepx-msglp                     │  ← 核心消息循环
│          (Loop: stdin/stdout 事件驱动)           │
├────────┬────────┬──────────┬─────────┬─────────┤
│ gate   │ config │ message  │ tools   │ session │  ← 领域服务
│        │ prompt │ store    │ subagt  │ store   │
│        │ regist │ effect   │ bridge  │ migrate │
├────────┴────────┴──────────┴─────────┴─────────┤
│            deepx-proto (IPC 帧定义)              │  ← 通信协议
├─────────────────────────────────────────────────┤
│            deepx-types (共享类型)                 │  ← 基础层
│    message / tool_def / config / session /       │
│    provider / api_types / token / platform       │
└─────────────────────────────────────────────────┘
```

### 每个 Crate 的职责

| Crate | Rust 行数快照 | 核心职责 |
|-------|---------|---------|
| **deepx-types** | 661 | `Message`, `ToolDef`, `ConfigStore`, `SessionMeta`, `ProviderSpec`, 令牌计数 |
| **deepx-proto** | 532 | JSON-LP 帧定义 (`Ui2Agent`, `Agent2Ui`), `Redacted` 密钥保护 |
| **deepx-gate** | 1,887 | HTTP SSE 流式客户端, 重试逻辑, 工具调用解析 (XML/DSML), 内容审查 |
| **deepx-config** | 843 | `Config` 加载/保存双写 (TOML + SQLite), 系统提示词注入, Provider 注册表 |
| **deepx-session** | 1,208 | 会话 CRUD, meta.json + messages.jsonl + index.json, Turso 镜像 |
| **deepx-message** | 983 | `MessageStore` 状态机, 截断/折叠, Turn/Step 模型 |
| **deepx-msglp** | 2,746 | `Loop` 事件循环, 用户输入处理, 压缩, 权限门, 仪表板 |
| **deepx-tools** | 5,876 | 工具实现, `ToolManager` 注册/执行, 权限引擎, AgentFS 桥接 |
| **deepx-subagent** | 209 | `spawn_subagent` 工具, 子进程管理 |
| **deepx-tauri** | 1,823 | `AgentRegistry` 多会话管理, 30+ Tauri commands, 前端事件桥接 |
| **deepx-gate-testui** | 584 | 本地 HTTP mock 测试 UI |

> 行数仅用于估算维护规模，不代表复杂度或测试覆盖率；新增代码后应重新统计。

---

## 3. 核心数据流

### 完整请求生命周期

```
Frontend (React)                    Agent (Rust 子进程)                   LLM API
───────────────                    ──────────────────────                 ──────
                                                                           │
  用户输入文字                                                              │
  │                                                                        │
  ├─→ invoke("send_message")                                              │
  │   agent_bridge.rs                                                     │
  │   └─→ 写 stdin: Ui2Agent::UserInput { text }                          │
  │                                                                        │
  │                        Loop::run() 读 cmd_rx                           │
  │                        └─→ drain_pending()                            │
  │                           └─→ dispatch() → handle_user_input()        │
  │                              │                                        │
  │                              ├─→ gate::content_guard() 审查           │
  │                              ├─→ msg.push_user(text)                  │
  │                              │    └─→ Effect::None                    │
  │                              ├─→ build_context_for_gate()             │
  │                              │    (系统提示词 + 压缩摘要 +              │
  │                              │     recent 消息 + workspace 标注)       │
  │                              │                                        │
  │                              ├─→ gate::chat_stream()                  │
  │                              │    │                                   │
  │                              │    ├─→ POST /chat/completions  ────────┼→ LLM 推理
  │                              │    │   (SSE streaming)                 │   │
  │                              │    │                                   │   │
  │                              │    │  ←  StreamEvent::ContentDelta() ──┼── 流式 tokens
  │                              │    │  ←  StreamEvent::ToolCallProgress │
  │                              │    │  ←  StreamEvent::Done { msg }     │
  │                              │    │                                   │
  │                              ├─→ emit_delta(RoundDelta)  (流式预览)   │
  │                              │    │                                   │
  │                              ├─→ msg.push_assistant(assistant_msg)    │
  │                              │    └─→ Effect::None (有工具调用)        │
  │                              │    └─→ Effect::TurnComplete (纯文本)    │
  │                              │                                        │
  │                              ├─→ resolve_write_conflicts()            │
  │                              │    (同文件检测，分组串行执行)            │
  │                              │                                        │
  │                              ├─→ 并行执行工具组                        │
  │                              │    │                                   │
  │                              │    ├─→ permission::needs_permission()  │
  │                              │    │   └─→ AskUser → 等待用户确认      │
  │                              │    │                                   │
  │                              │    ├─→ bridge::execute_tool()          │
  │                              │    │   └─→ ToolManager::handle_req()  │
  │                              │    │      └─→ handler(ctx) 直接调用   │
  │                              │    │                                   │
  │                              │    ├─→ msg.push_tool_results_batch()   │
  │                              │    │    └─→ Effect::None (还有工具有待 │
  │                              │    │       完成 → 回到 gate)            │
  │                              │    │    └─→ Effect::TurnComplete       │
  │                              │    │        (所有工具完成)              │
  │                              │                                        │
  │                        Loop 写 event_tx                               │
  │                        └─→ Agent2Ui 帧 (JSON-LP)                     │
  │                                                                        │
  │  后台 reader 线程读 stdout ────────────────────────────────────────────│
  │  └─→ emit Tauri event                                                 │
  │                                                                        │
  Frontend 状态更新 ─← 显示流式内容/工具结果/                              │
```

### 关键设计决策

1. **JSON-LP over stdin/stdout**: 代理作为子进程运行，通过行分隔 JSON 帧通信。避免了 HTTP/WebSocket 的复杂性。
2. **同步 I/O + 多线程**: 代理使用 `ureq`（同步 HTTP 客户端）+ 后台 I/O 线程，而非 async/await。
3. **工具内联执行**: 工具在代理进程内直接调用（非 IPC），消除了序列化开销和进程管理复杂性。
4. **写入冲突检测**: 并行工具执行前检测同一文件的写冲突，自动将冲突工具分组串行执行。

---

## 4. 关键类型定义

### 4.1 消息类型 (`deepx-types/src/message.rs`)

```rust
// 内容块 —— 匹配 OpenAI Chat Completions API
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    
    #[serde(rename = "reasoning")]
    Reasoning { reasoning: String },
    
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: serde_json::Value },
    
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String, #[serde(default)] success: bool },
}

// 消息 —— 使用 content-block 格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub msg_id: Option<u64>,             // 单调递增，用于排序/去重
    pub role: String,                     // "system" | "user" | "assistant" | "tool"
    pub name: Option<String>,             // 区分同角色参与者
    pub content: Vec<ContentBlock>,
}

// 构造方法
impl Message {
    pub fn system(content: &str) -> Self { /* role="system" */ }
    pub fn user(content: &str) -> Self   { /* role="user" */ }
    pub fn tool(tool_call_id: &str, result: &str, success: bool) -> Self { /* role="tool" */ }
}
```

### 4.2 Provider 模型 (`deepx-types/src/provider.rs`)

```rust
// 思考参数的多样适配
#[derive(Debug, Clone, Default)]
pub enum ThinkingParamMode {
    #[default]
    OpenAi,              // thinking: {type: "enabled"}
    QwenEnableThinking,  // enable_thinking: true
    MiniMaxAdaptive,     // thinking: {type: "adaptive"} + reasoning_split: true
}

// 缓存 token 字段位置
#[derive(Debug, Clone, Default)]
pub enum CacheTokenField {
    #[default]
    PromptCacheHitTokens,  // DeepSeek: usage.prompt_cache_hit_tokens
    PromptDetailsCached,   // Qwen/GLM: usage.prompt_tokens_details.cached_tokens
    UsageCachedTokens,     // Kimi: usage.cached_tokens
    None,                  // MiMo/MiniMax: 无缓存信息
}

// 端点规范 —— 运行时自动填充 base_url + models
#[derive(Debug, Clone)]
pub struct EndpointSpec {
    pub id: String,                    // "openai" | "anthropic"
    pub display: String,               // "OpenAI-compatible"
    pub protocol: String,              // "openai"
    pub base_url: String,              // "https://api.deepseek.com"
    pub default_model: String,
    pub models: Vec<String>,
    pub models_url: Option<String>,    // GET /models endpoint
    pub user_id_mode: Option<UserSendMode>,
    pub chat_path: Option<String>,     // "/compatible-mode/v1/chat/completions" (Qwen)
    pub thinking_mode: ThinkingParamMode,
    pub cache_field: CacheTokenField,
    pub has_balance: bool,
    pub supports_thinking: bool,
    pub stateful: bool,                // CDP proxy 模式
}

// Provider 规范
#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub id: String,                    // "deepseek" | "qwen" | "glm" | ...
    pub display: String,
    pub endpoints: Vec<EndpointSpec>,  // 同一 provider 可有多端点
}
```

### 4.3 会话元数据 (`deepx-types/src/session.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionMeta {
    // ── 持久化字段 ──
    pub seed: String,                  // 唯一会话 ID
    pub created_at: u64,               // epoch 秒
    pub updated_at: u64,
    pub model: String,
    pub effort: Option<String>,        // reasoning_effort
    pub message_count: usize,
    pub turn_count: usize,             // 对话轮次
    pub last_summary: String,          // 最新压缩摘要
    pub compact_skip: usize,           // 跳过的已压缩轮次
    pub mode: u8,                      // 0=Normal, 1=Plan, 2=Code

    // ── 运行时字段 (不持久化) ──
    #[serde(skip)] pub resume_seed: Option<String>,
    #[serde(skip)] pub tokens: u64,
    #[serde(skip)] pub title: Option<String>,
    #[serde(skip)] pub from_resume: bool,
    #[serde(skip)] pub turso_backed: bool,
}
```

### 4.4 配置持久化 (`deepx-types/src/config.rs`)

```rust
// 所有字段均为 Option —— 只存储用户显式设置的值
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentConfig {
    pub provider_id: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
    pub context_limit: Option<u32>,
    pub endpoint: Option<String>,
    pub reasoning_effort: Option<String>,
    pub profiles: Option<HashMap<String, ProfileConfig>>,
    pub active_profile: Option<String>,
    pub lang: Option<String>,
    pub context7_api_key: Option<String>,
    pub subagent: Option<PersistentSubagentConfig>,
    pub compliance_enabled: Option<bool>,
    pub compliance_extra_keywords: Option<Vec<String>>,
    pub compliance_allowlist: Option<Vec<String>>,
    pub database: Option<PersistentDatabaseConfig>,   // Turso 镜像开关
    pub permission_level: Option<u8>,
    pub tokenizer_path: Option<String>,
}

// 原子写入 —— 写 .tmp 然后 rename
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn save(&self, config: &PersistentConfig) -> bool {
        let content = toml::to_string_pretty(config)?;
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &self.path)?;  // 原子替换
        true
    }
}
```

---

## 5. IPC 协议设计

### 5.1 帧格式 (`deepx-proto/src/agent_protocol.rs`)

以换行符分隔的 JSON（JSON-LP），每条消息是一行完整的 JSON。

**UI → Agent (`Ui2Agent`)**:

```rust
#[serde(tag = "type")]
pub enum Ui2Agent {
    UserInput { text: String },                    // 用户文字输入
    ToolCall { id, name, action, args },           // 用户触发的工具调用
    CreateSession,                                  // 创建新会话
    ResumeSession { seed: String },                // 恢复已有会话
    NewSession,                                     // 强制新建
    Cancel,                                         // 中断当前操作
    Shutdown,                                       // 优雅关闭
    ReloadConfig,                                   // 热重载配置
    UndoTurn { turn_id: String },                  // 撤销一轮
    Compact,                                        // 触发压缩
    LoadMoreTurns { before_turn_id, count },       // 增量加载历史
    SetMode { mode: String },                      // "normal" | "plan" | "code"
    PermissionResponse { tool_call_id, approved, trust_folder },
}
```

**Agent → UI (`Agent2Ui`)**:

```rust
pub enum Agent2Ui {
    Ready,                                           // 空闲，等待输入
    SessionCreated { seed: String },                 // 新会话已创建
    SessionRestored { seed, turns, tokens_used, ... }, // 会话已恢复
    TurnStart { turn_id, user_text },               // 一轮开始
    RoundStart { turn_id, round_num },              // 子轮次开始
    RoundDelta { turn_id, round_num, block },       // 流式增量
    RoundComplete { turn_id, round_num, blocks, ... }, // 轮次完成
    ToolResults { turn_id, round_num, results },    // 工具执行结果
    TurnEnd { turn_id, stop_reason, usage },        // 一轮结束
    ExecProgress { tool_call_id, stream, seq, chunk }, // 工具执行流式输出
    CodeDelta { lines_added, lines_removed, ... },  // 代码变更统计
    TokenInfo { turn_id, tokens_used, ... },        // token 使用统计
    Cancelled,                                       // 操作已取消
    Error { message },                               // 错误
    PermissionRequest { tool_call_id, reason, paths, ... }, // 权限请求
    CompactStart { turns_total, turns_keeping },    // 压缩开始
    CompactEnd { summary_chars, turns_compacted },   // 压缩完成
    Dashboard { hp_connected, documents, tasks, ... }, // 仪表板数据
    MoreTurns { turns, has_more },                  // 增量加载的轮次
    ToolNotice { message, level },                   // 工具通知
    ShutdownAck,                                     // 确认关闭
}
```

### 5.2 Redacted 密钥保护

```rust
pub struct Redacted(pub String);

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_empty() { f.write_str("\"\"") }
        else { f.write_str("\"***\"") }           // API 密钥永不泄漏到日志
    }
}
```

---

## 6. LLM 网关实现

### 6.1 Provider 配置与重试 (`deepx-gate/src/openai.rs`)

```rust
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_SECS: u64 = 1;
const SSE_READ_TIMEOUT: Duration = Duration::from_millis(200);

fn is_retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 503)    // 速率限制 + 服务器错误
}

fn backoff_delay(attempt: u32) -> Duration {
    let secs = BASE_DELAY_SECS * 2u64.pow(attempt.saturating_sub(1));
    Duration::from_secs(secs.min(30))    // 指数退避: 1s → 2s → 4s, 上限 30s
}

// 可取消的 sleep — 每 100ms 检查 cancel flag
fn sleep_with_cancel(delay: Duration, cancel: Option<&Arc<AtomicBool>>) -> bool {
    let start = Instant::now();
    while start.elapsed() < delay {
        if is_cancelled(cancel) { return true; }
        thread::sleep(remaining.min(Duration::from_millis(100)));
    }
    false
}
```

### 6.2 SSE 流式解析

```rust
pub enum StreamEvent {
    ContentDelta(String),        // 普通文本 token
    ReasoningDelta(String),      // 思考 token (DeepSeek R1 等)
    ToolCallProgress { id, name, args_so_far },  // 增量工具调用参数
    Done { raw_message, usage, stop_reason },    // 流结束
    UsageUpdate(UsageInfo),      // 缓存命中信息
    Error(String),               // 错误
    Retrying { attempt, max_retries, delay_secs, error },  // 重试中
}
```

### 6.3 工具调用解析 (`deepx-gate/src/tool_parser.rs`)

支持两种格式：

1. **XML/DSML** (DeepSeek 旧版):
```xml
<DSML|tool_calls>
<DSML|invoke name="exec">
<DSML|parameter name="command" string="true">ls -la</DSML|parameter>
</DSML|invoke>
</DSML|tool_calls>
```

2. **OpenAI 原生** (新版): 直接使用 `Message.content` 中的 `ToolUse` 块。

解析逻辑包含：
- `strip_fenced_code()`: 移除 markdown 代码围栏，防止示例代码被误解析
- `has_dsml()`: 快速检测 DSML 标签
- `parse_dsml_tool_calls()`: 全角/半角管道符兼容
- `parse_xml_tool_calls()`: 旧 XML 格式回退

### 6.4 内容审查 (`deepx-gate/src/guard.rs`)

```rust
const BLOCKED_PATTERNS: &[&str] = &[
    "心理咨询", "情感陪伴", "自杀", "自残",
    "密钥", "密码", "api_key",
    "色情", "赌博", "毒品",
];

const ALLOWLIST_PREFIXES: &[&str] = &[
    "research:", "academic:", "crypto:",    // 学术/加密讨论白名单
];

pub fn content_guard(input: &str) -> Result<(), String> {
    // 1. 检查白名单前缀
    // 2. NFKC 规范化 (捕获全角字符混淆攻击)
    // 3. ASCII 模式: 词边界匹配; CJK 模式: 子串匹配
}
```

---

## 7. 消息存储与状态机

### 7.1 Turn/Step 模型 (`deepx-message/src/store.rs`)

```
MessageStore
├── system_messages: Vec<Message>      ← 系统提示词
├── turns: Vec<Turn>                   ← 对话轮次
│   ├── Turn 0
│   │   ├── user: Message              ← 用户输入
│   │   └── steps: Vec<Step>
│   │       ├── Step 0
│   │       │   ├── assistant: Message  ← LLM 回复 (可含 ToolUse)
│   │       │   └── tool_results: Vec<Message>  ← 工具执行结果
│   │       ├── Step 1                  ← LLM 继续 (如果需要更多工具)
│   │       └── ...
│   └── Turn N
└── compact_skip: usize                ← 已压缩跳过的轮次数
```

### 7.2 Effect 状态机

```rust
pub enum Effect {
    None,                            // 无副作用
    CallGate { messages: Vec<Message> },  // 需要调用 LLM
    TurnComplete,                    // 本轮结束，保存快照
}
```

`push_*` 方法的返回逻辑：

| 方法 | 返回 |
|------|------|
| `push_system(msg)` | `Effect::None` |
| `push_user(text)` | `Effect::None` |
| `push_assistant(msg)` | `Effect::None` (有工具) 或 `Effect::TurnComplete` (纯文本) |
| `push_tool_result(id, result, success)` | `Effect::TurnComplete` (所有工具完成) 或 `Effect::None` |
| `push_tool_results_batch(results)` | 同上 |

### 7.3 工具结果截断与折叠

```rust
// 截断: 保留 JSON 元数据，截断 content 字段
fn truncate_tool_result(tool_name: &str, result: &str) -> String {
    // JSON: 截断 content 字段到 4000 字符
    // Plain: file_* 在新行处截断，其他在 UTF-8 边界截断
}

// 折叠: 已完成的轮次中，工具结果替换为简短标记
fn fold_completed_tool_result(tool_name: &str, result: &str) -> String {
    // 豁免: read/search 不折叠 (代码/Grep 结果必须可见)
    // 其他: 保留第一行 + "[details folded]" 标记
}
```

### 7.4 MessageStore 核心结构

```rust
pub struct MessageStore {
    seed: String,
    system_messages: Vec<Message>,
    turns: Vec<Turn>,
    cancelled: bool,
    tool_executor: Option<ToolExecutorFn>,   // Box<dyn Fn(ToolExecRequest) -> ToolExecReport + Send>
    compact_skip: usize,                     // 跳过的已压缩轮次
    next_msg_id: u64,                        // 单调递增的消息 ID
    replaying: bool,                         // from_messages 回放模式
    pending_save: Vec<Message>,              // 待刷盘的消息缓冲区
    ephemeral: bool,                         // 子代理一次性模式
}
```

---

## 8. 主循环驱动

### 8.1 Loop 结构 (`deepx-msglp/src/lib.rs`)

```rust
pub struct Loop {
    agent: AgentState,                       // 配置 + 消息存储 + 会话
    cmd_rx: mpsc::Receiver<Ui2Agent>,        // 来自 stdin 读取线程
    event_tx: mpsc::SyncSender<Agent2Ui>,    // 发往 stdout 写入线程
    cancel: CancelToken,                     // 共享中止标志
    phase: LoopPhase,                        // Idle | GateRunning | ToolsRunning
    pending_session: Option<String>,         // 繁忙期间排队的会话切换
    pending_new_session: bool,
    pending_shutdown: bool,
    pending_reload_config: bool,
    code_stats: Vec<CodeDeltaRecord>,        // 累计代码变更
    writer_dead: Arc<AtomicBool>,            // stdout 管道断开检测
    notify: NotificationThread,              // Windows toast 通知
    mode: u8,                                // 0=Normal, 1=Plan, 2=Code
    pending_permission: Option<PendingToolCall>,  // 等待用户确认的工具
    trusted_folders: TrustedFolderSet,       // 跨工作区信任目录
}
```

### 8.2 主循环运行 (`Loop::run()`)

```rust
pub fn run(&mut self) {
    self.agent.rebind_store();
    
    // 自动初始化: 如果预设了 seed，创建或恢复会话
    if let Some(seed) = resume_seed { self.handle_resume_session(&seed); }
    else if has_seed { lifecycle::create_session_with_seed(&mut self.agent); }
    
    self.emit(Agent2Ui::Ready);
    
    loop {
        self.drain_pending();             // 处理排队的命令
        
        // 处理待定的会话切换 / 新建 / 关闭
        if let Some(seed) = self.pending_session.take() { ... }
        if self.pending_new_session { ... }
        if self.pending_shutdown { break; }
        
        // 检查 stdout 管道是否断开
        if self.writer_dead.load(Ordering::SeqCst) { break; }
        
        // 阻塞等待下一条命令
        let frame = self.cmd_rx.recv()?;
        
        // catch_unwind 防止 handler panic 导致进程静默退出
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.dispatch(frame);
        }));
        if result.is_err() { break; }
    }
}
```

### 8.3 用户输入处理流程 (`handle_user_input`)

```rust
fn handle_user_input(&mut self, text: &str) {
    // 1. 合规审查
    gate::content_guard(text)?;
    
    // 2. 推入消息存储
    self.agent.msg.push_user(text);
    
    // 3. 构建 LLM 上下文
    let context = self.agent.build_context();
    
    // 4. 主循环: 交替 gate ↔ tools
    loop {
        let effect = self.agent.msg.needs_gate(context)?;
        // ── 调用 LLM ──
        gate::chat_stream(&provider, messages, tools, max_tokens, ...)?;
        
        let effect = self.agent.msg.push_assistant(assistant_msg);
        
        // ── 执行工具 ──
        let pending = self.agent.msg.pending_tools();
        let (groups, serial_after) = resolve_write_conflicts(&pending);
        
        // 并行组 0: 无冲突工具 → 并行执行
        // 串行组 1..N: 同文件冲突 → 依次执行
        // ...
        
        self.agent.msg.push_tool_results_batch(&results);
        
        if matches!(effect, Effect::TurnComplete) { break; }
    }
    
    // 5. 保存会话快照
    self.agent.msg.flush_meta(&model, &effort);
    self.emit(Agent2Ui::TurnEnd { ... });
}
```

### 8.4 写入冲突检测

```rust
fn resolve_write_conflicts(pending: &[PendingTool]) -> (Vec<Vec<usize>>, HashSet<usize>) {
    // 1. 构建 file → [工具索引] 映射
    let mut file_writers: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, tool) in pending.iter().enumerate() {
        for path in file_write_paths(&tool.name, &tool.args) {
            file_writers.entry(path).or_default().push(i);
        }
    }
    
    // 2. 构建连通组 (传递闭包)
    // 如果两个工具写入同一文件的不同路径，加入同一组
    // ...
    
    // 3. 组内第一个工具并行运行，其余串行
    // serial_groups = 需要串行的组; serial_after = 组内非首位的工具
}
```

### 8.5 对话压缩 (`handle_compact`)

```
保留最后 ~4000 token 的上下文不变
前面的轮次序列化为紧凑文本
调用 LLM (chat_sync) 生成结构化的 Markdown 摘要
摘要包含: 目标 / 重要细节 / 文件清单 / 决策日志 / 关键符号 / 工作状态 / 下一步
```

压缩模板 (`COMPACT_TEMPLATE`) 强制 LLM 输出固定结构:
```
## Objective
## Important Details
## File Inventory
## Decision Log
## Key Symbols
## Work State
## Next Move
```

---

## 9. 工具生态系统

### 9.1 工具注册架构 (`deepx-tools/src/registration.rs`)

所有工具在 `build_tool_manager()` 中一次性构造:

```rust
pub fn build_tool_manager(extra_registrars: &[ToolRegistrar]) -> ToolManager {
    let mut mgr = ToolManager::new();
    
    exec::register(&mut mgr);          // exec_run
    explore::register(&mut mgr);       // explore_scan
    web::register(&mut mgr);           // web_fetch, web_search
    file_mutate::register(&mut mgr);   // write, edit, edit_block, delete
    file_query::register(&mut mgr);    // read, list, search, diff
    task::register(&mut mgr);          // task_create, task_update, task_delete, task_list
    plan::register(&mut mgr);          // plan_create, plan_list, plan_submit
    context7::register(&mut mgr);      // context7
    ask_user::register(&mut mgr);      // ask_user
    process_inspect::register(&mut mgr); // process_check, process_wait, process_kill
    memory::register(&mut mgr);        // memory_read, memory_write, memory_clear
    git_tool::register(&mut mgr);      // git_log, git_diff, git_status, git_add, git_commit, ...
    
    // 外部注入 (如 deepx-subagent)
    for reg in extra_registrars { reg(&mut mgr); }
    
    mgr
}
```

### 9.2 工具处理器签名

```rust
pub struct ToolHandler {
    pub key: String,                              // 工具名
    pub description: &'static str,                 // LLM 可见的描述
    pub input_schema: serde_json::Value,           // JSON Schema
    pub handler: fn(ToolCallCtx) -> ToolResult,   // 实际执行函数
    pub risk: ToolRisk,                            // 风险级别
    pub default_timeout: Duration,
}

// 工具调用上下文
pub struct ToolCallCtx {
    pub id: String,
    pub name: String,
    pub action: String,                     // "file" 工具的 action 参数
    pub args: serde_json::Value,
    pub tx_progress: Option<ExecProgressSender>,  // 有界、非阻塞流式输出通道
    pub timeout_secs: Option<u64>,
}

// 工具结果
pub struct ToolResult {
    pub success: bool,
    pub content: String,
}
```

### 9.3 三阶段执行 (`ToolManager`)

```rust
// 阶段 1: 准备 (持锁) —— 验证 + 安全检查 + 注册 inflight
pub fn prepare_req(&mut self, id, name, action, args, ...) -> Result<PreparedCall, ToolExecReport> {
    // 1. allowlist 检查 (子代理限制)
    // 2. handler 查找
    // 3. SafetyPolicy::evaluate() (工作区外操作)
    // 4. 注册 inflight cancel token
}

// 阶段 2: 执行 (无锁) —— panic 保护
// 在 prepare_req 释放锁后调用 handler_fn(ctx)

// 阶段 3: 完成 (持锁) —— 统计 + 清理 inflight
pub fn finalize_req(&mut self, prepared, tool_result, elapsed_ms) -> ToolExecReport {
    // 1. 移除 inflight
    // 2. 更新统计: calls_total, failures, files_read, files_written
}
```

### 9.4 工具能力列表（含 action 展开）

下表是面向用户/模型的能力视图，不等同于 `ToolManager` 的注册 key 数量。例如 Git 实际注册为 `git`，通过 `action=log|diff|...` 分发；Web 实际注册为 `web`，通过 action 分发。文件、task、plan、memory、process 等多数能力则各自注册独立 key。调试“Unknown tool”时必须先核对 `registration.rs` 和各模块 `register()`，不能直接把下表名称当作 handler key。

| 工具名 | 分类 | 风险 | 功能 |
|--------|------|------|------|
| `read` | file_query | ReadOnly | 读取文件 |
| `list` | file_query | ReadOnly | 目录列表 |
| `search` | file_query | ReadOnly | 正则搜索 |
| `diff` | file_query | ReadOnly | 文件对比 |
| `write` | file_mutate | Destructive | 创建/覆盖文件 |
| `edit` | file_mutate | Destructive | 字符串替换 |
| `edit_block` | file_mutate | Destructive | 多行编辑 |
| `delete` | file_mutate | Destructive | 移动到回收站 |
| `exec_run` | exec | Destructive | 执行命令 (超时 + 流式输出) |
| `explore_scan` | explore | ReadOnly | 项目架构分析 |
| `web_fetch` | web | Network | HTTP 请求 |
| `web_search` | web | Network | Bing RSS 搜索 |
| `context7` | context7 | Network | Context7 文档查询 |
| `git_log` | git_tool | ReadOnly | Git log |
| `git_diff` | git_tool | ReadOnly | Git diff |
| `git_status` | git_tool | ReadOnly | Git status |
| `git_show` | git_tool | ReadOnly | Git show |
| `git_add` | git_tool | Destructive | Git add |
| `git_commit` | git_tool | Destructive | Git commit |
| `git_branch` | git_tool | Destructive | Git branch |
| `git_checkout` | git_tool | Destructive | Git checkout |
| `git_merge` | git_tool | Destructive | Git merge |
| `git_restore` | git_tool | Destructive | Git restore |
| `task_create` | task | Write | 创建任务 |
| `task_update` | task | Write | 更新任务状态 |
| `task_delete` | task | Write | 删除任务 |
| `task_list` | task | ReadOnly | 列出任务 |
| `plan_create` | plan | Write | 创建计划项 |
| `plan_list` | plan | ReadOnly | 列出计划 |
| `plan_submit` | plan | ReadOnly | 提交计划 |
| `memory_read` | memory | ReadOnly | 读取跨会话记忆 |
| `memory_write` | memory | Write | 写入记忆 |
| `memory_clear` | memory | Write | 清除记忆 |
| `ask_user` | ask_user | Administrative | 向用户提问 |
| `process_check` | process | ReadOnly | 检查后台进程 |
| `process_wait` | process | ReadOnly | 等待进程完成 |
| `process_kill` | process | Destructive | 终止进程 |
| `process_write` | process | Write | 向后台进程 stdin 写入内容 |
| `spawn_subagent` | subagent | Administrative | 生成子代理 |

### 9.5 Git 工具实现 (`deepx-tools/src/git_tool.rs`)

Git 工具通过 **libgit2** (`git2` crate) 直接调用，不执行 shell 命令:

```rust
fn open_repo(path_arg: &str) -> Result<Repository, String> {
    // 如果路径为空/不存在，从工作区目录向上搜索 .git
    Repository::discover(&start)
}

fn exec_log(args: &serde_json::Value) -> String {
    // revwalk.push_head() → set_sorting(TIME) → 迭代 commits
    // 支持 max_count, author 过滤
}

fn exec_diff(args: &serde_json::Value) -> String {
    // commit_a vs commit_b 或 HEAD vs 工作树
    // 支持 cached (staged) 模式
}
```

### 9.6 探索工具 (`deepx-tools/src/explore.rs`)

```rust
fn exec_architecture(path: &str) -> String {
    // 检测项目类型: Cargo.toml → Rust, go.mod → Go, package.json → Node
    // Rust: 解析 Cargo.toml 获取 crates + dependencies
    //       扫描 src/lib.rs 获取 pub mod 声明
    // Go: 解析 go.mod + 统计 .go 文件
    // 输出: [ARCHITECTURE] path + type + 模块图
}
```

---

## 10. 权限与安全引擎

### 10.1 工具分类

```rust
pub enum ToolCategory {
    Read,   // 只读: file_read, explore, search, git_diff/log, memory_read, process_check
    Write,  // 变更: file_write/edit/delete, git_commit, memory_write, task/plan 创建
    Exec,   // 执行: exec_run, spawn_subagent
    Net,    // 网络: web_fetch, web_search, context7
}
```

### 10.2 权限级别

| Level | 名称 | 策略 |
|-------|------|------|
| 1 | MaxLockdown | 所有操作需确认 (Read/Write/Exec/Net) |
| 2 | ReadFree | Read 自动通过；Write/Exec/Net 需确认 |
| 3 | WorkspaceFree | 工作区内的 Read/Write 自动通过；外部/Exec/Net 需确认 |
| 4 | Unrestricted | 所有操作自动通过 |

### 10.3 信任目录

```rust
pub struct TrustedFolderSet {
    seed: String,
    dirs: HashSet<PathBuf>,
}

impl TrustedFolderSet {
    pub fn load(seed: &str) -> Self;       // 从 JSON 文件加载
    pub fn trust(&mut self, dir: &Path);    // 信任新目录并持久化
    pub fn contains(&self, dir: &Path) -> bool;  // 检查是否已信任
}
```

持久化路径: `{deepx_dir}/sessions/{seed}/trusted_folders.json`

---

## 11. 会话管理

### 11.1 文件结构

```
{sessions_dir}/
├── index.json              ← 所有会话的索引 (快速列表)
└── {seed}/
    ├── meta.json           ← 会话元数据 (原子写入)
    ├── messages.jsonl      ← 消息 (追加写入)
    ├── workspace.txt       ← 工作区路径
    ├── code_stats.jsonl    ← 代码变更统计
    ├── trusted_folders.json ← 信任目录
    ├── context_stats.json  ← 上下文 token 统计
    └── sessions.db         ← Turso SQLite 镜像 (可选)
```

### 11.2 核心操作

```rust
pub struct SessionManager {
    sessions_dir: PathBuf,
    active_path: PathBuf,
    turso_enabled: AtomicBool,
    dbs: Mutex<HashMap<String, TursoBackend>>,  // per-session SQLite
}

impl SessionManager {
    pub fn global() -> &'static SessionManager;  // OnceLock 单例
    
    // CRUD
    pub fn list(&self) -> Vec<SessionMeta>;
    pub fn load(&self, seed: &str) -> Option<(SessionMeta, Vec<Message>)>;
    pub fn save_append(&self, seed, messages, model, effort, compact_skip, turn_count);
    pub fn delete(&self, seed: &str) -> bool;
    pub fn exists(&self, seed: &str) -> bool;
    pub fn generate_seed() -> String;   // 4字符随机 hex
    pub fn now_epoch() -> u64;
    
    // Turso
    pub fn set_turso_enabled(&self, enabled: bool);
}
```

### 11.3 迁移 (TOML → JSONL)

当检测到旧的 TOML 格式会话时，`migrate` 模块自动将其转换为 JSONL 格式。

---

## 12. 配置系统

### 12.1 加载优先级

```
TOML 文件 (config.toml)
    ↓ 覆盖
SQLite 数据库 (config.db) — 仅当 database.enabled=true
    ↓ 覆盖
Provider 注册表 (端点默认值)
    ↓ 覆盖
活跃 Profile (覆盖 model/base_url/max_tokens/effort)
    ↓ 覆盖
用户显式的 base_url 覆盖
```

### 12.2 双写策略

```rust
impl Config {
    pub fn save(&self) -> Result<(), String> {
        // 1. 写入 TOML (原子 rename)
        store.save(&pc)?;
        
        // 2. 双写到 SQLite (仅当 database.enabled)
        if self.database.enabled {
            ConfigDb::save_config(&json)?;
        }
    }
}
```

### 12.3 Provider 注册表 (`deepx-config/src/registry.rs`)

内置 8+ provider:

| Provider ID | 显示名 | 端点 | 特殊适配 |
|-------------|--------|------|---------|
| deepseek | DeepSeek | openai | 默认 thinking:OpenAi, cache:PromptCacheHitTokens |
| qwen | Qwen (阿里百炼) | openai | chat_path 覆盖, thinking:QwenEnableThinking, cache:PromptDetailsCached |
| glm | GLM (智谱) | openai | thinking:OpenAi, cache:PromptDetailsCached |
| kimi | Kimi (月之暗面) | openai | cache:UsageCachedTokens |
| mimo | MiMo | openai | cache:None |
| minimax | MiniMax | openai | thinking:MiniMaxAdaptive, cache:None |
| doubao | Doubao (豆包) | openai | 无特殊适配 |
| openai | OpenAI | openai | 无特殊适配 |

向后兼容: `deepseek-openai` → `provider_id=deepseek, endpoint=openai`

---

## 13. 前端架构

### 13.1 技术栈

```
deepx-tauri/
├── src/                          ← React + TypeScript 前端
│   ├── App.tsx (29K)             ← 主应用组件
│   ├── App.css (9K)              ← 全局样式
│   ├── main.tsx                  ← 入口
│   ├── components/               ← React 组件 (25+)
│   │   ├── ChatView.tsx          ← 聊天区域
│   │   ├── InputBar.tsx          ← 输入栏 (含 / 命令菜单)
│   │   ├── MessageList.tsx       ← 消息列表
│   │   ├── MessageItem.tsx       ← 单条消息渲染
│   │   ├── ThinkingBlock.tsx     ← 思考块展开/折叠
│   │   ├── ToolRow.tsx           ← 工具调用/结果行
│   │   ├── MarkdownBody.tsx      ← Markdown 渲染
│   │   ├── PermissionDialog.tsx  ← 权限确认弹窗
│   │   ├── PlanReviewPanel.tsx   ← 计划审查面板
│   │   ├── GitDiffPanel.tsx      ← Git diff 查看
│   │   ├── SettingsView.tsx      ← 设置页面 (24K, 最大组件)
│   │   ├── TokenChart.tsx        ← Token 使用图表
│   │   ├── SessionCard.tsx       ← 会话卡片
│   │   ├── StatusPanel.tsx       ← 状态面板
│   │   ├── StartupView.tsx       ← 启动/配置引导
│   │   └── ...
│   ├── lib/                      ← 工具函数
│   ├── store/                    ← 状态管理
│   ├── i18n/                     ← 国际化 (中/英)
│   └── styles/                   ← 组件样式
├── src-tauri/                    ← Rust 后端
│   └── src/
│       ├── lib.rs                ← Tauri app 入口 + 30+ commands
│       └── agent_bridge.rs       ← 多代理注册表
├── package.json
├── vite.config.ts
└── index.html
```

### 13.2 Tauri Commands (`agent_bridge.rs`)

```rust
// 通过 #[tauri::command] 暴露给前端的 30+ 命令:
cmd_send_message       // 发送用户消息
cmd_set_mode           // 切换模式 (normal/plan/code)
cmd_get_version        // 获取版本
cmd_cancel             // 取消当前操作
cmd_save_config        // 保存配置
cmd_load_config        // 加载配置
cmd_list_sessions      // 列出会话
cmd_delete_session     // 删除会话
cmd_undo_turn          // 撤销一轮
cmd_compact            // 触发压缩
cmd_resume_session     // 恢复会话
cmd_new_session        // 新建会话
cmd_load_more_turns    // 增量加载历史
cmd_get_workspace      // 获取工作区
cmd_set_workspace      // 设置工作区
cmd_close_session      // 关闭会话
cmd_read_plan          // 读取 PLAN.md
cmd_plan_action        // 执行计划操作
cmd_get_token_stats    // Token 统计
cmd_get_git_diff       // Git diff
cmd_get_git_branch     // 当前分支
cmd_list_branches      // 分支列表
cmd_switch_branch      // 切换分支
cmd_git_commit         // 提交
cmd_get_dashboard_data // 仪表板
cmd_task_action        // 任务操作
cmd_get_context_stats  // 上下文 token 统计
cmd_migrate_to_turso   // 迁移到 Turso
cmd_get_activity       // 活动数据
```

### 13.3 多代理注册表

```rust
pub struct AgentRegistry {
    agents: HashMap<String, AgentInstance>,  // seed → AgentInstance
}

pub struct AgentInstance {
    seed: String,
    stdin: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl AgentRegistry {
    // 每个会话一个子进程: 通过 stdin/stdout 管道通信
    pub fn spawn_agent(&mut self, seed, app_handle);
    pub fn send_to_agent(&self, seed, frame: Ui2Agent);
    pub fn kill_agent(&self, seed);
    pub fn shutdown_all();
}
```

---

## 14. 生产力特性

### 14.1 系统提示词 (`backend_prompt.md`)

47 行的 Markdown，定义了代理的身份和行为规范:

```
- 身份: "You are DeepX, a coding engineer like Claude Code"
- 风格: 简洁、直接、外科手术般精确
- 响应格式: 每响应用 1-3 句话 + file:line 引用
- 规则: 在编辑前理解代码库 → 修复根因而非症状 → Rust 编辑后必须 cargo check
- 完成审计: 对照实际状态验证完成情况
```

### 14.2 环境注入 (`os_env.md`)

```markdown
- **Date**: {{DATE}}
- **OS**: {{OS}}
- **Shells available**: {{SHELLS}}
- **Toolchain**: {{TOOLS}}
```

在启动时动态检测 shell 可用性和工具链版本，注入到系统提示词中。

### 14.3 Agent 模式

| 模式 | 值 | 行为 |
|------|----|------|
| Normal | 0 | 标准模式：读+写+执行 |
| Plan | 1 | 只读模式：阻止写入/执行/破坏性工具 |
| Code | 2 | 已可读写和执行；当前 bridge 层与 Normal 的差异很小 |

Agent 模式与 `permission_level` 是两个维度：模式由 bridge 的全局 `AGENT_MODE` 控制，当前仅 Plan 模式有额外阻断；权限等级由权限引擎决定某项操作是否需要用户确认。两者不能混为同一开关。

### 14.4 工作区管理

```rust
// 全局工作区路径
pub static CURRENT_WORKSPACE: RwLock<String>;

// 持久化: {deepx_dir}/sessions/{seed}/workspace.txt
// 启动时恢复: bridge::load_workspace(&seed)
```

### 14.5 文件缓存

`deepx-tools/src/file_cache.rs`: 内存中缓存最近读取的文件内容，避免重复 I/O。

### 14.6 通知 (Windows)

`deepx-msglp/src/toast_com.rs` 和 `notification.rs`: 通过 Windows COM 发送 toast 通知，在长时间运行的操作完成时提醒用户。

### 14.7 仪表板

`deepx-msglp/src/dashboard.rs`: 构建实时仪表板数据：
- `build_documents()`: 从工作区检测文档
- `build_recent_edits()`: 从 git 提取最近编辑
- `build_tasks()`: 当前活跃的任务列表

### 14.8 上下文 token 统计

每次 API 调用后，将上下文 token 分解写入 `context_stats.json`:
```json
{
  "messages": N,
  "chat_text": 1234,
  "thinking": 567,
  "tool_calls": 89,
  "tool_results": 456,
  "tools_schema": 100,
  "system_prompt": 234,
  "thinking_blocks": 3,
  "tool_call_blocks": 2
}
```

---

## 15. 测试与质量保障

### 15.1 Clippy 配置

```toml
[workspace.lints.clippy]
unwrap_used = "deny"       # 整个工作区强制: 不允许 unwrap()
string_slice = "deny"      # 整个工作区强制: 不允许字符串切片
```

特定 crate 豁免:
- `deepx-gate/Cargo.toml`: `string_slice = "allow"` (所有切片在 ASCII 模式上)
- `deepx-session/Cargo.toml`: `string_slice = "allow"` (使用 is_char_boundary 检查)

### 15.2 测试资产与当前基线

“存在测试”不代表测试当前通过。以下结果是 2026-07-12 的抽样基线：11 个 crate 均通过 `cargo check -p <crate> --all-targets`；测试只对重点 crate 做了抽样，不能推断整个 workspace 全绿。

| Crate | 测试资产 | 2026-07-12 抽样结果 |
|-------|---------|---------|
| deepx-gate | `tests/gate_test.rs`、`guard.rs`、`tool_parser.rs` | **失败**：`test_has_dsml_detection` 的期望与 wrapper-only 检测实现冲突 |
| deepx-config | `prompt.rs` 内联测试 | **通过**：3 passed |
| deepx-tools | Git/workspace 单测、CJK 与并发读取集成测试 | **失败**：`ten_parallel_reads` 使用旧式 `file/read`，当前解析为未注册的 `file_read` |
| deepx-msglp | `tests/concurrent_read_stress.rs` | 已编译，尚未在本轮形成完整运行结论 |
| deepx-tauri | 当前未发现 `#[test]` | `cargo check --all-targets` 通过，但缺少有效 Rust 测试基线 |

### 15.3 错误处理模式

```rust
// 1. anyhow::Result 用于可恢复错误
pub fn chat_stream(...) -> anyhow::Result<()>

// 2. Result<(), String> 用于配置/会话错误
pub fn save(&self) -> Result<(), String>

// 3. 工具错误通过 ToolResult 内容返回 (不改写为 Err)
ToolResult { success: false, content: "[ERROR] ..." }

// 4. panic 保护在工具执行和主循环中
std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))
```

### 15.4 发布配置

```toml
[profile.release]
opt-level = "z"      # 优化尺寸
lto = true           # 链接时优化
strip = true         # 剥离调试符号
codegen-units = 1    # 单代码生成单元 (更好的优化)
```

---

## 16. 抽样核验与已知偏差

本节记录已经通过源码或命令核对的结论，作为后续 debug 的事实基线。未列出的章节仍属于架构调查结果，进入对应 crate 时应再次以当前源码验证。

### 16.1 已确认结论

- Workspace 确有 11 个 crate，版本为 0.8.0，edition 为 2024。
- `deepx-gate` 使用同步 `ureq` 和 SSE，重试、取消及工具调用解析均位于网关层。
- `deepx-proto` 定义 JSON 行协议的 `Ui2Agent`、`Agent2Ui` 和日志脱敏类型 `Redacted`。
- 配置和会话的 Turso 后端默认 feature 已启用；关闭 feature 时相关路径会退化或返回未编译提示。
- Git 能力由 `git2`/libgit2 实现，不依赖 shell `git` 命令。
- 11 个 crate 的 `cargo check -p <crate> --all-targets` 在本次抽样中均成功。

### 16.2 已知偏差与风险

| 编号 | 范围 | 事实 | Debug 影响 |
|------|------|------|------------|
| A-01 | 规模统计 | 原报告多项行数低估，现已更新为 2026-07-12 快照 | 排期应按实际规模评估，尤其是 msglp/tools |
| A-02 | 工具命名 | 能力名、注册 key、`name + action` 合成名并不总是一致 | 权限、Plan allowlist、历史调用兼容都可能出现 Unknown tool |
| A-03 | Agent 模式 | 当前只有 Plan 在 bridge 层被额外阻断；Code 与 Normal 基本同路 | 不应把 mode 和 permission level 当成同一状态机 |
| A-04 | gate 测试 | `has_dsml` 只认 `tool_calls` wrapper，但测试也期待裸 invoke 被识别 | 必须先确定协议契约，再决定改实现还是改测试 |
| A-05 | tools 测试 | 并发测试调用旧式 `file/read`，当前注册 key 为 `read` | 需决定保留兼容层还是迁移调用方，不能只为绿测硬改 |
| A-06 | 测试分布 | types/proto/message/session/subagent/tauri 等测试较少或没有 | 上层绿测不能替代协议、持久化和边界测试 |

### 16.3 工作树保护

开始任何 crate 修复前必须执行 `git status --short` 和目标文件的 `git diff`。本次调查时 `crates/deepx-msglp/src/agent.rs` 已存在用户修改，后续窗口不得覆盖、回退或顺手格式化该修改。

---

## 17. Crate Debug 总控看板

### 17.1 推荐顺序

依赖层顺序为：

```text
deepx-types
  └─ deepx-proto
deepx-gate / deepx-config / deepx-session
  └─ deepx-message
      └─ deepx-tools
          └─ deepx-subagent
              └─ deepx-msglp
                  └─ deepx-tauri

deepx-gate-testui：随 deepx-gate 或最后单独核验
```

实际开工优先级可以先处理已有稳定失败的 `deepx-gate` 和 `deepx-tools`，但修改公共类型或协议前必须回到依赖层顺序进行影响分析。

### 17.2 状态看板

状态枚举：`未开始`、`调查中`、`待实现`、`实现中`、`待复核`、`已验收`、`阻塞`。

| Crate | 风险 | 当前基线 | 首要检查点 | 状态/窗口 |
|-------|------|----------|------------|-----------|
| deepx-types | 中 | check 通过；缺测试 | serde/TS 兼容、token 与 platform 边界 | 未开始 |
| deepx-proto | 高 | check 通过；缺测试 | 帧兼容、未知字段、密钥脱敏 | 未开始 |
| deepx-gate | 高 | check 通过；1 个单测失败 | SSE、重试/取消、DSML/XML、usage | **待实现；建议本窗口认领** |
| deepx-config | 中 | check/test 通过（3 tests） | TOML/DB 优先级、原子保存、profile 覆盖 | 未开始 |
| deepx-session | 高 | check 通过；缺测试 | JSONL/Turso 一致性、迁移、截断恢复 | 未开始 |
| deepx-message | 高 | check 通过；缺测试 | Turn/Step 状态机、不变量、截断折叠 | 未开始 |
| deepx-tools | 最高 | `cargo test -p deepx-tools`：40 passed、2 个测试受全局状态并发污染而失败；并发读取集成测试单独通过 | 名称/Schema、超时、取消、全局状态 | 调查完成；独立窗口 |
| deepx-subagent | 高 | check 通过；缺测试 | 进程生命周期、工具 allowlist、超时 | 未开始 |
| deepx-msglp | 最高 | check 通过；有用户未提交修改 | 主循环、并发、取消、权限等待、压缩 | 未开始；独立窗口 |
| deepx-tauri | 高 | check 通过；缺 Rust 测试 | 子进程桥接、多会话、事件顺序、关闭 | 未开始 |
| deepx-gate-testui | 低 | check 通过 | mock 与 gate 协议一致性 | 未开始 |

### 17.3 单 crate 完成定义

每个窗口只负责一个 crate。满足以下条件才能从“待复核”变为“已验收”：

1. 写清 crate 的输入、输出、公共接口和至少三条关键不变量。
2. 保存修复前的稳定复现命令和失败输出摘要。
3. 修改范围最小，不夹带无关重构、依赖升级或全仓格式化。
4. 正常、错误、边界路径均有测试；并发/持久化 crate 还需覆盖中断或恢复。
5. 至少运行：

   ```powershell
   cargo fmt --check
   cargo check -p <crate> --all-targets
   cargo test -p <crate>
   cargo clippy -p <crate> --all-targets
   ```

6. Codex 审查最终 diff，核对错误处理、兼容性、安全性和测试有效性。
7. 记录实际执行结果；未执行、超时和环境失败不得写成通过。

---

## 18. Codex 与 DeepSeek 协作规范

### 18.1 职责边界

| 角色 | 负责 | 不负责 |
|------|------|--------|
| Codex | 源码调查、根因假设、方案设计、任务切分、Prompt、检查点、diff 审查、最终验收 | 把未经验证的架构判断直接交给实现方 |
| DeepSeek | 在白名单文件内做最小实现、补测试、运行指定命令、如实报告结果 | 自行扩大范围、改协议、升级依赖、删除断言、宣称未运行的检查通过 |

### 18.2 一窗口一 Crate

- 每个 Codex 窗口只认领一个 crate，避免长上下文中混入其它 crate 的实现细节。
- `explore.md` 是跨窗口共享的总控文档，不等于任何窗口可以修改所有 crate。
- 公共类型、IPC 或跨 crate API 变更必须先在总控文档记录影响面，再分别交给受影响窗口。
- 当前窗口完成本报告后建议固定认领 `deepx-gate`；`deepx-tools`、`deepx-msglp` 等另开窗口。

### 18.3 标准流水线

```text
Codex 调查与复现
  → 明确契约和非目标
  → 提供最小上下文、参考片段、检查点和 Prompt
  → DeepSeek 实现并自测
  → Codex 审查 diff 与测试有效性
  → 不通过则给出定点返工 Prompt
  → 通过后更新 crate 看板
```

### 18.4 Codex 审查检查点

- 根因：修改是否解决根因，而非仅改断言或吞掉错误。
- 范围：是否只触及允许文件；是否覆盖用户已有修改。
- 契约：serde 字段、IPC 帧、工具名/action、持久化格式是否保持兼容。
- 错误：是否保留错误上下文；是否把失败伪装成成功或默认值。
- 安全：路径边界、命令执行、权限判断、密钥日志是否退化。
- 并发：锁是否跨慢操作持有；取消、超时、线程退出是否完整。
- 测试：测试能否在修复前失败、修复后通过；是否覆盖负例和边界。
- 验证：命令输出是否与代码状态一致，是否存在 warning 或未运行目标。

---

## 19. 任务 Prompt 模板

### 19.1 DeepSeek 实现 Prompt

````text
你负责 DeepX-Fork 的单 crate 受限修复，不得扩大任务范围。

【目标 crate】
{crate}

【问题与复现证据】
{错误输出、失败测试、调用链}

【已确认根因/待验证假设】
{Codex 的调查结论；明确哪些仍是假设}

【目标】
{一句话完成条件}

【非目标】
- {本任务不处理的相邻问题}
- 不升级依赖，不改 Cargo.lock
- 不进行无关重构或全仓格式化

【允许修改】
- {文件白名单}

【禁止修改】
- {公共协议、其它 crate、用户已有修改等}
- 不得删除、ignore 或弱化失败测试
- 不得通过吞错、硬编码返回值或放宽安全检查来通过测试

【关键不变量】
1. {兼容性不变量}
2. {状态/并发不变量}
3. {安全或错误处理不变量}

【相关代码片段】
```rust
{Codex 提取的最小相关代码；仅作定位，修改前仍需读取当前文件}
```

【实现要求】
1. 先用不超过 5 句话说明根因和最小方案。
2. 只修改白名单文件。
3. 新增或调整能证明根因已修复的测试，至少覆盖正常、错误和边界输入。
4. 依次实际运行：
   cargo fmt --check
   cargo check -p {crate} --all-targets
   cargo test -p {crate}
   cargo clippy -p {crate} --all-targets
5. 任何命令失败时保留关键错误，不得声称通过。

【最终输出格式】
- 根因
- 修改文件
- 行为变化
- 测试与命令结果
- 尚存风险
````

### 19.2 Codex 返工 Prompt

````text
本次实现暂不验收，只修正以下审查问题：

1. {具体问题，附文件和代码位置}
2. {违反的不变量或失败用例}

保持上一版允许/禁止范围不变。不要重写整个方案，只提交解决上述问题的最小 diff，并重新运行失败命令及 crate 全部测试。最终按“修改点、验证结果、尚存风险”输出。
````

### 19.3 Gate 首个任务的输入草案

```rust
pub fn has_dsml(content: &str) -> bool {
    // 当前实现只在 wrapper 存在时触发扫描
    let lower = content.to_lowercase();
    lower.contains("dsml") && lower.contains("tool_calls")
}

#[test]
fn test_has_dsml_detection() {
    // 当前失败：测试还要求裸 invoke 被检测
    assert!(has_dsml("use <|DSML|invoke name=\"read\">"));
}
```

在交给 DeepSeek 前，Codex 必须先回答：`has_dsml` 的契约是“检测任意 DSML 痕迹”还是“检测可执行且带 wrapper 的完整调用”。还要核对 fenced code、半角/全角分隔符、混合格式和普通文本误判。没有完成契约判断前，不允许直接修改实现或测试。

---

## 附录: 依赖关系图

```
deepx-tauri
├── deepx-msglp ────────────── 核心循环
│   ├── deepx-gate ─────────── LLM HTTP 客户端
│   │   └── deepx-types
│   ├── deepx-config ───────── 配置 + 提示词 + 注册表
│   │   ├── deepx-types
│   │   └── turso (可选)
│   ├── deepx-session ──────── 会话持久化
│   │   ├── deepx-types
│   │   └── turso (可选)
│   ├── deepx-message ──────── 对话状态机
│   │   ├── deepx-types
│   │   ├── deepx-proto
│   │   └── deepx-session
│   ├── deepx-tools ────────── 工具执行
│   │   ├── deepx-message
│   │   ├── deepx-proto
│   │   └── deepx-types
│   ├── deepx-subagent ─────── 子代理工具
│   │   ├── deepx-tools
│   │   ├── deepx-types
│   │   └── deepx-session
│   └── deepx-proto ────────── IPC 协议
│       └── deepx-types
├── deepx-config
├── deepx-session
├── deepx-tools
├── deepx-types
└── deepx-proto
```

---

## 20. 工具调用修复追踪

> 2026-07-13 源码核验。每项只由对应 crate 窗口实现；跨 crate 任务按依赖顺序交付。

| ID | 优先级 | 归属 crate / 窗口 | 修复目标 | 完成检查点 |
|----|--------|-------------------|----------|------------|
| TOOL-01 ✅ | P0 | `deepx-message` | 已按每个 `ToolResult.tool_use_id` 查找对应 `ToolUse.name` 后再截断/折叠。 | 已验证当前步骤与已完成步骤的混合 `write + read + exec_run` 回灌；`cargo test -p deepx-message`、`cargo check -p deepx-message --all-targets` 通过。 |
| TOOL-02 ✅ | P0 | `deepx-msglp` | 写冲突检测已覆盖扁平工具名 `write/edit/edit_block/delete`；后续同资源写只串行执行一次。 | `conflict` 单测验证同文件严格串行、不同文件仍可并发；`cargo test -p deepx-msglp` 通过。 |
| TOOL-03 ✅ | P1 | `deepx-tools` | 已对齐注册 key、schema、描述和错误 hint；`read` 同时接受 `path`/`paths`，移除 LLM 可见旧工具名。 | `cargo check -p deepx-tools --all-targets` 通过。 |
| TOOL-04 ✅ | P1 | `deepx-tools` | handler 默认 deadline 已注入上下文；Web/Context7 采用显式 HTTP deadline，Web body 限制 512 KiB；`exec_run` 默认 30 s，超时或 per-call cancel 会杀掉 child 并返回结构化状态。 | `exec` 的 timeout/cancel 回归用例通过；`cargo test -p deepx-tools --lib` 通过。 |
| TOOL-05 ✅ | P1 | `deepx-tools` | 测试态 `RUNTIME_CTX` 改为线程本地；所有 bridge 测试由 guard 串行保护全局 manager/mode/counter。 | 默认并行调度下 `cargo test -p deepx-tools --lib` 通过（44 tests）。 |
| TOOL-06 ✅ | P1 | `deepx-message` + `deepx-tools` | read/list/search/exec 的截断都包含总量和可执行续读指令；LLM 上下文截断按真实 tool name 处理。 | `deepx-message` 续读提示回归用例通过；`exec` 输出截断回归用例通过。 |
| TOOL-07 ✅ | P2 | `deepx-msglp` | 单轮最多 16 个 call，独立调用最多 4 worker 并发；超出调用返回带 call ID 的可恢复错误。 | `cargo check -p deepx-msglp --all-targets`、`cargo test -p deepx-msglp` 通过。 |

### 20.1 推荐执行顺序

`TOOL-01 → TOOL-02 → TOOL-03/TOOL-04/TOOL-05/TOOL-06 → TOOL-07 → EXEC-RESEARCH-01`。

先修正“LLM 看见什么”和写入竞态，再调整工具表面契约与运行治理；最后才评估执行输出流式化，避免为错误调用链增加复杂度。

### 20.2 EXEC-RESEARCH-01：Exec v2 流式路径已实现

**前置条件**：TOOL-01、02、04、05、06 全部验收。

**已实现（2026-07-13）**：在不恢复 PTY 的前提下，`exec_run` 的 stdout/stderr pipe reader 产生结构化 `ExecProgress { tool_call_id, stream, seq, chunk }`；前端按独立临时卡即时渲染，最终 `ToolResults` 原子替换临时输出。并行和串行工具路径均保留每个 call ID 与 stream 的事件归属，不再跨调用合并。

**运行保证**：保留 `Command + piped stdout/stderr`。进度通道为 256 条有界 `sync_channel`，reader 用 `try_send`，UI 慢时丢弃实时副本而继续 drain OS pipe；丢弃字节数写入最终 JSON 的 `ui_dropped_bytes`。pipe 保留 5 MiB，保留上限后仍 drain；UTF-8 跨块字符在 reader 内拼接，不向 UI 注入替换字符。最终给 LLM 的仍是 token 截断后的单份结果，不回灌实时 chunk。

**Windows/前端补充（2026-07-13）**：卡片列表使用稳定位置的 Solid `Index`，流式更新不再重建 `ToolRow` 并重置展开状态。输出解码采取 UTF-8 优先、失败时 `GetOEMCP + MultiByteToWideChar` 回退；简体中文 Windows 的 cmd/ping GBK（936）输出无需开启“Beta: 使用 Unicode UTF-8 提供全球语言支持”。

**非目标**：交互式 stdin、终端仿真、颜色/光标协议、shell 兼容层；这些属于 PTY 复杂度，不应在本研究恢复。

**验收指标**：

1. 前端在命令运行中可按 call ID 观察合并后的 stdout/stderr，最终结果不重复、不丢尾部。
2. 大输出受硬字节上限、前端合并和 LLM 上下文截断三层约束，且三层状态可区分。
3. timeout/cancel 在 Windows 下不遗留 child 或 pipe-reader 线程。
4. 同轮并行命令的进度不会串到错误 tool_call_id。

**未纳入本阶段**：Windows Job Object 的进程树终止、显式 `ExecStarted/ExecFinished` 前端事件，以及可写 stdin 的 terminal session。这些属于后续 `ExecutionService` 与 terminal API 立项，不能通过恢复 PTY 替代。

### 20.3 架构评价

当前架构的优点是进程内 handler、显式授权令牌、call ID 贯通、以及“执行期不持有 ToolManager 锁”。提升空间集中在三点：

1. **建立唯一的 Tool Invocation Contract**：注册名、schema、权限类别、冲突资源、deadline、截断/续读提示应由同一声明生成，避免描述、旧 name/action 兼容层和运行时逻辑漂移。
2. **让调度层拥有并发与生命周期**：`deepx-msglp` 应提供有界 worker、资源锁、取消和结果聚合；`deepx-tools` 仅执行单次 handler，不能各层各自补一套“parallel”。
3. **把 LLM 视图建模为显式投影**：`raw_result`、`ui_result`、`llm_context_result` 分开带版本/截断元数据，按 call ID 变换并可测试；不再依赖字符串折叠约定。

> **报告结束** — 本文档是 DeepX-Fork v0.8.0 的架构调查与 crate debug 总控基线。源码持续变化；进入每个 crate 前必须重新验证对应结论。
