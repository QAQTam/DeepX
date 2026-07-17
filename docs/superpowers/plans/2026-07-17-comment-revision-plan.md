# DeepX-Fork 全面注释修订 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add comprehensive doc comments (`//!`, `///`, `//`) to all 12 crates in the DeepX-Fork workspace — every pub symbol, struct field, complex algorithm, and unsafe block — without changing any code behavior.

**Architecture:** Bottom-up execution by dependency order: leaf crates (deepx-skills, deepx-types) first, then protocol/config layer, then core engines, finally the orchestrator and Tauri UI. Each phase is independently verifiable with `cargo check` and `cargo test`.

**Tech Stack:** Rust edition 2024, Cargo workspace, no new dependencies.

## Global Constraints

- Zero code behavior changes — only `//!`, `///`, `//` prefix lines added
- `cargo check --workspace` must pass after every file edit
- `cargo test -p <crate>` must pass before moving to next crate
- English documentation style, matching existing convention
- No "TODO", "FIXME", or placeholder comments added
- Commit after each crate is done

---

### Phase 0: Foundation — Comment Style Guide

**Goal:** Create a reference document so all subsequent phases follow consistent style.

**Files:**
- Create: `docs/comment-style.md`

#### Task 0.1: Write comment style guide

**Interfaces:**
- Produces: `docs/comment-style.md` — the single source of truth for comment conventions used by all later tasks.

- [ ] **Step 1: Write the guide**

```markdown
# DeepX Comment Style Guide

## Module-level docs (`//!`)

Every `lib.rs` and `mod.rs` MUST start with a `//!` block:

```rust
//! crate-name — one-line summary.
//!
//! Expanded description of what this crate/module does.
//!
//! ## Key concepts (optional)
//!
//! ## Architecture (optional)
```

## Public API docs (`///`)

### Structs

```rust
/// Brief purpose.
///
/// Longer description if the struct has non-obvious state rules
/// (e.g. must call `init()` before use, not Clone, etc.).
pub struct MyStruct {
    /// What this field stores. Include constraints:
    /// "Non-empty", "Must be a valid absolute path", etc.
    pub field: Type,
}
```

### Enums

```rust
/// What this enum classifies / represents.
pub enum MyEnum {
    /// When this variant is produced and what it means.
    VariantA,
    /// When this variant is produced and what it means.
    VariantB,
}
```

### Functions

```rust
/// Brief: what this function does (imperative mood).
///
/// # Arguments
/// * `param` — description (only if non-obvious from name).
///
/// # Returns
/// Description of return value.
///
/// # Errors
/// When and why this returns an error.
///
/// # Panics
/// Conditions that cause panics (if any).
pub fn do_thing(param: &str) -> Result<Output, Error> { ... }
```

### Traits

```rust
/// What this trait abstracts.
///
/// # Implementors
/// Who should implement this, and any constraints.
pub trait MyTrait {
    /// What this method does.
    fn method(&self) -> Output;
}
```

## Inline comments (`//`)

Use `//` only when the code is non-obvious. Explain **why**, not **what**.

```rust
// State machine: Idle → Running → WaitingUser → Idle.
// We skip Running→Idle transition when tools are still executing.
if phase == LoopPhase::ToolsRunning { ... }
```

```rust
// Lock ordering: pending lock must be acquired BEFORE session lock
// to prevent deadlock with handle_cancel().
let pending = self.pending.lock().unwrap();
```

## Unsafe blocks

Every `unsafe { }` block MUST be immediately preceded by a `// SAFETY:` comment listing the invariants that make it sound.

```rust
// SAFETY: `ptr` was allocated by Vec::with_capacity(n) and we just
// verified that `i < n`, so the pointer is valid for writes and
// does not alias any other live reference.
unsafe { ptr.add(i).write(value); }
```

## Anti-patterns

| Don't | Do |
|-------|-----|
| `/// The name field.` | `/// User's display name. Non-empty, max 64 chars.` |
| `/// Obvious helper.` | Delete the comment — it adds nothing. |
| `/// Calls foo() and returns result.` | Explain *why* foo() is called here. |
| `// TODO: add docs` | Write the docs, or leave no comment. |
| Comment in Chinese | Stick to English (existing convention). |
```

- [ ] **Step 2: Commit**

```bash
git add docs/comment-style.md
git commit -m "docs: add comment style guide"
```

---

### Phase 1: Leaf Crates

**Goal:** Document the two crates with zero internal dependencies. These are the foundation types used by everything else.

---

#### Task 1.1: deepx-types — core type definitions

**Files:**
- Modify: `crates/deepx-types/src/provider.rs` (~180L)
- Modify: `crates/deepx-types/src/message.rs` (~130L)
- Modify: `crates/deepx-types/src/config.rs` (~200L)
- Modify: `crates/deepx-types/src/session.rs` (~80L)
- Modify: `crates/deepx-types/src/tool_def.rs` (~20L)
- Modify: `crates/deepx-types/src/arg.rs` (~50L)
- Modify: `crates/deepx-types/src/platform.rs` (~80L)
- Modify: `crates/deepx-types/src/token.rs` (~70L)
- Modify: `crates/deepx-types/src/api_types.rs` (~30L)
- Modify: `crates/deepx-types/src/state.rs` (~20L)
- Modify: `crates/deepx-types/src/lib.rs` (~44L)

**Interfaces:**
- Produces: documented public types consumed by ALL other crates

- [ ] **Step 1.1a: provider.rs — annotate ProviderSpec, EndpointSpec, and supporting enums**

Read `crates/deepx-types/src/provider.rs`. For each struct and enum, add `///` comments on the type, and `///` on every field and variant. Key types:

```rust
/// Controls where the user identifier is sent in the API request.
pub enum UserSendMode {
    /// Not sent at all.
    None,
    /// Sent in the HTTP request body (JSON field).
    Body,
    /// Sent as an HTTP header.
    Header,
}

/// How the thinking/reasoning parameter is sent to the model.
pub enum ThinkingParamMode {
    /// Standard OpenAI format: {"type": "enabled"|"disabled"}.
    OpenAi,
    /// Gemini format: thought budget integer or boolean.
    Gemini,
    /// Simple boolean parameter.
    Bool,
}

/// Which field in the usage response carries the cache token count.
pub enum CacheTokenField {
    /// Default: `prompt_cache_hit_tokens`.
    PromptCacheHitTokens,
    /// Alternative: `cache_read_input_tokens` (Anthropic style).
    CacheReadInputTokens,
}

/// Configuration for a single API endpoint (e.g. "openai" for DeepSeek).
pub struct EndpointSpec {
    /// Internal identifier (e.g. "openai", "v1").
    pub id: String,
    /// Human-readable label shown in settings UI.
    pub display: String,
    /// Protocol: "openai" or "anthropic".
    pub protocol: String,
    /// Base URL for API requests (e.g. "https://api.deepseek.com").
    pub base_url: String,
    /// Default model when none selected.
    pub default_model: String,
    /// Cached list of available models, fetched from the API.
    pub models: Vec<String>,
    // ... remaining fields with similar documentation
}

/// Top-level provider definition (e.g. DeepSeek, Qwen, OpenAI).
pub struct ProviderSpec {
    /// Unique provider identifier (e.g. "deepseek", "qwen").
    pub id: String,
    /// Human-readable display name.
    pub display: String,
    /// Available endpoints for this provider.
    pub endpoints: Vec<EndpointSpec>,
}
```

- [ ] **Step 1.1b: message.rs — annotate ContentBlock, Message, ToolCall, FunctionCall**

```rust
/// A single block of content in a chat message.
///
/// Messages are composed of multiple content blocks to support
/// mixed text + tool call + tool result content in a single turn.
pub enum ContentBlock {
    /// Plain text content from the model or user.
    Text { text: String },
    /// A tool call invocation requested by the model.
    ToolCall { tool_call: ToolCall },
    /// The result of executing a tool, fed back to the model.
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

/// A complete chat message in a conversation.
pub struct Message {
    /// "system", "user", "assistant", or "tool".
    pub role: String,
    /// Content blocks forming this message.
    pub content: Vec<ContentBlock>,
    /// Optional display name (for user messages).
    pub name: Option<String>,
    /// Token usage if this message came from an API response.
    pub usage: Option<UsageInfo>,
}
```

- [ ] **Step 1.1c: config.rs — annotate PersistentConfig hierarchy**

Read and annotate `PersistentConfig`, `PersistentSubagentConfig`, `PersistentDatabaseConfig`, `ProfileConfig`, `ConfigStore`, `BalanceInfo` with field-level docs.

- [ ] **Step 1.1d: session.rs — annotate session types**

```rust
/// Activation state of a single skill within a session.
pub enum SkillSessionEntryState {
    /// Skill is loaded and active in the current session context.
    Active,
    /// Skill was previously active but has been explicitly unloaded.
    Released,
    /// Skill is being loaded (transient state).
    Loading,
}

/// Runtime tracking for one skill in a session.
pub struct SkillSessionEntry {
    /// Skill name (matches SKILL.md metadata).
    pub name: String,
    /// Current activation state.
    pub state: SkillSessionEntryState,
    /// Hash of the skill body at activation time, for change detection.
    pub content_hash: String,
}

/// Metadata for one session, stored in index.json.
pub struct SessionMeta {
    /// Short unique session identifier (8 hex chars).
    pub seed: String,
    /// User-facing session title.
    pub title: String,
    /// Unix timestamp of creation.
    pub created_at: u64,
    /// Unix timestamp of last activity.
    pub updated_at: u64,
    /// Number of turns in this session.
    pub turn_count: u32,
    /// Total input tokens consumed.
    pub total_input_tokens: u64,
    /// Total output tokens produced.
    pub total_output_tokens: u64,
}
```

- [ ] **Step 1.1e: tool_def.rs — annotate ToolDef and ToolFunction**

- [ ] **Step 1.1f: arg.rs — annotate argument parsing functions with format examples**

```rust
/// Parse a positional argument value from a string of arguments.
///
/// Format: `key value` (space-separated). Case-insensitive key match.
///
/// # Arguments
/// * `args` — The raw arguments string (space-separated key-value pairs).
/// * `key` — The argument key to look up.
///
/// # Returns
/// `Some(value)` if found and non-empty, `None` otherwise.
pub fn parse_arg(args: &str, key: &str) -> Option<String> { ... }
```

- [ ] **Step 1.1g: platform.rs — annotate path and OS utility functions**

- [ ] **Step 1.1h: token.rs — annotate tokenizer functions and TokenBreakdown**

- [ ] **Step 1.1i: api_types.rs — annotate UsageInfo fields**

- [ ] **Step 1.1j: state.rs — annotate DebugLevel**

- [ ] **Step 1.1k: lib.rs — ensure crate-level docs are complete**

- [ ] **Step 1.1l: Verify**

```bash
cargo check -p deepx-types
cargo test -p deepx-types
```

- [ ] **Step 1.1m: Commit**

```bash
git add crates/deepx-types/src/
git commit -m "docs(deepx-types): add comprehensive doc comments to all public types"
```

---

#### Task 1.2: deepx-skills — skill management

**Files:**
- Modify: `crates/deepx-skills/src/lib.rs` (~950L)

**Interfaces:**
- Produces: documented `SkillMetadata`, `SkillActivation`, `SkillCatalog`, etc. for `deepx-tools` and `deepx-msglp`

- [ ] **Step 1.2a: Annotate constants and core types**

```rust
/// Magic marker that identifies a SKILL.md file as an active skill definition.
pub const ACTIVATION_MARKER: &str = "[DEEPX_SKILL_V1]";

/// Maximum skill file size in bytes (512 KB). Larger files are rejected.
pub const MAX_SKILL_BYTES: u64 = 512 * 1024;
```

- [ ] **Step 1.2b: Annotate SkillScope, SkillMetadata, DiagnosticSeverity, SkillDiagnostic, SkillCatalog, SkillActivation, SkillResource** — each with field-level docs.

- [ ] **Step 1.2c: Annotate core functions: discover(), load(), load_named(), validate_file(), render_catalog(), render_activation(), explicit_mentions(), managed_skill_for_path()** — with behavior, arguments, return values.

- [ ] **Step 1.2d: Annotate change-tracking types: SkillCatalogSnapshot, SkillEffect, SkillBodyChange, content_hash(), token_count(), describe_body_change()**

- [ ] **Step 1.2e: Annotate internal helper functions**

- [ ] **Step 1.2f: Verify**

```bash
cargo check -p deepx-skills
cargo test -p deepx-skills
```

- [ ] **Step 1.2g: Commit**

```bash
git add crates/deepx-skills/src/lib.rs
git commit -m "docs(deepx-skills): add comprehensive doc comments to all public API"
```

---

### Phase 2: Protocol & Configuration Layer

---

#### Task 2.1: deepx-proto — IPC protocol definitions

**Files:**
- Modify: `crates/deepx-proto/src/agent_protocol.rs` (~940L)
- Modify: `crates/deepx-proto/src/lib.rs` (~107L)

**Interfaces:**
- Produces: documented `Ui2Agent`, `Agent2Ui`, and all protocol types consumed by `deepx-msglp` and `deepx-tauri`

- [ ] **Step 2.1a: agent_protocol.rs — annotate SessionActivity, SessionActivityState**

- [ ] **Step 2.1b: Annotate Ui2Agent — all variants with trigger conditions**

```rust
/// Commands sent from the UI (frontend/Tauri) to the agent process.
///
/// Each variant is a JSON-tagged message sent via stdin JSON-LP.
pub enum Ui2Agent {
    /// User typed a message and pressed send.
    /// Triggers: InputEngine → TurnEngine → gate → tools pipeline.
    UserInput {
        /// The raw text the user typed.
        text: String,
    },

    /// User wants to cancel the current operation.
    /// Triggers: CancelToken.set() → all engines check and abort.
    Cancel,

    /// Frontend is requesting execution of a tool (e.g. from a UI button).
    ToolCall {
        /// Unique call identifier for tracking.
        id: String,
        /// Tool name (e.g. "read", "write", "exec_run").
        name: String,
        /// Action sub-command within the tool.
        action: String,
        /// Tool arguments as a JSON value.
        args: serde_json::Value,
    },

    /// User responded to a permission dialog.
    PermissionResponse {
        /// The tool_call_id from the permission request.
        tool_call_id: String,
        /// Whether the user approved the operation.
        approved: bool,
        /// Whether to trust this folder going forward.
        trust_folder: bool,
    },
    // ... remaining variants with similar detail
}
```

- [ ] **Step 2.1c: Annotate Agent2Ui — all variants with semantics**

```rust
/// Events sent from the agent process to the frontend.
///
/// Each variant is a JSON-tagged message sent via stdout JSON-LP.
/// The frontend renders these as UI updates (streaming text, tool results,
/// permission dialogs, etc.).
pub enum Agent2Ui {
    /// A new token of streaming text from the model.
    TokenDelta {
        turn_id: String,
        content: String,
    },
    /// A tool call the model wants to execute.
    ToolCallDelta {
        turn_id: String,
        call_id: String,
        name: String,
        args: serde_json::Value,
    },
    /// The results of executing tool calls.
    ToolResults {
        turn_id: String,
        results: Vec<ToolResultDef>,
    },
    /// The agent needs user permission before executing a tool.
    PermissionRequest {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        risk: PermissionRisk,
        category: String,
        target_paths: Vec<String>,
    },
    // ... remaining variants
}
```

- [ ] **Step 2.1d: Annotate SkillInfo, SkillsStatus, SkillRuntimeInfo, ToolCallDef, ToolResultDef, FileSnapshotInfo, DocInfo, TaskInfo**

- [ ] **Step 2.1e: Annotate RoundData, TurnData, RoundBlock, RoundDeltaKind, CodeDeltaRecord, CodeDaily**

- [ ] **Step 2.1f: Annotate AskMode, AskResolution, AskQuestion, AskAnswer — the interactive questioning protocol**

- [ ] **Step 2.1g: Annotate FrontendToDaemon, DaemonToFrontend**

- [ ] **Step 2.1h: lib.rs — verify crate-level docs are complete; annotate Redacted if needed**

- [ ] **Step 2.1i: Verify**

```bash
cargo check -p deepx-proto
cargo test -p deepx-proto
```

- [ ] **Step 2.1j: Commit**

```bash
git add crates/deepx-proto/src/
git commit -m "docs(deepx-proto): add comprehensive doc comments to all protocol types"
```

---

#### Task 2.2: deepx-config — configuration management

**Files:**
- Modify: `crates/deepx-config/src/config.rs`
- Modify: `crates/deepx-config/src/prompt.rs`
- Modify: `crates/deepx-config/src/config_db.rs`
- Modify: `crates/deepx-config/src/registry.rs`
- Modify: `crates/deepx-config/src/lib.rs`

**Interfaces:**
- Produces: documented `Config`, `SubagentConfig`, `DatabaseConfig`, registry functions for `deepx-gate`, `deepx-msglp`, `deepx-tauri`

- [ ] **Step 2.2a: config.rs — annotate Config struct and all sub-config types with field docs**

- [ ] **Step 2.2b: prompt.rs — annotate full_system_prompt() and related**

- [ ] **Step 2.2c: config_db.rs — add method-level docs on turso dual-write**

- [ ] **Step 2.2d: registry.rs — annotate all provider lookup functions**

- [ ] **Step 2.2e: lib.rs — ensure crate-level docs**

- [ ] **Step 2.2f: Verify & Commit**

```bash
cargo check -p deepx-config
cargo test -p deepx-config
git add crates/deepx-config/src/
git commit -m "docs(deepx-config): add comprehensive doc comments"
```

---

#### Task 2.3: deepx-session — session persistence

**Files:**
- Modify: `crates/deepx-session/src/manager.rs`
- Modify: `crates/deepx-session/src/store/mod.rs`
- Modify: `crates/deepx-session/src/store/turso_backend.rs`
- Modify: `crates/deepx-session/src/session_meta.rs`
- Modify: `crates/deepx-session/src/migrate.rs`
- Modify: `crates/deepx-session/src/lib.rs`

**Interfaces:**
- Produces: documented `SessionManager` singleton and store functions for `deepx-message`, `deepx-msglp`, `deepx-tauri`

- [ ] **Step 2.3a: manager.rs — annotate SessionManager methods**

- [ ] **Step 2.3b: store/mod.rs — annotate JSONL I/O functions with behavior contracts**

- [ ] **Step 2.3c: store/turso_backend.rs — annotate TursoBackend**

- [ ] **Step 2.3d: session_meta.rs — verify or add field docs**

- [ ] **Step 2.3e: migrate.rs — annotate run() logic**

- [ ] **Step 2.3f: lib.rs — ensure crate-level docs**

- [ ] **Step 2.3g: Verify & Commit**

```bash
cargo check -p deepx-session
cargo test -p deepx-session
git add crates/deepx-session/src/
git commit -m "docs(deepx-session): add comprehensive doc comments"
```

---

### Phase 3: Core Engine Layer

---

#### Task 3.1: deepx-gate — LLM API gateway

**Files:**
- Modify: `crates/deepx-gate/src/lib.rs`
- Modify: `crates/deepx-gate/src/openai.rs`
- Modify: `crates/deepx-gate/src/tool_parser.rs`
- Modify: `crates/deepx-gate/src/types.rs`
- Modify: `crates/deepx-gate/src/guard.rs`

**Interfaces:**
- Produces: documented `chat_stream`, `chat_sync` and helper functions for `deepx-msglp`

- [ ] **Step 3.1a: types.rs — annotate ProviderKind, ProviderConfig, StreamEvent**

- [ ] **Step 3.1b: openai.rs — annotate chat_stream_openai, chat_sync_openai and SSE parsing logic**

- [ ] **Step 3.1c: tool_parser.rs — annotate has_dsml, strip_fenced_code, parse_xml_tool_calls, parse_dsml_tool_calls, parse_tool_calls with parsing rules**

- [ ] **Step 3.1d: guard.rs — annotate content_guard security rules**

- [ ] **Step 3.1e: lib.rs — verify existing docs are complete**

- [ ] **Step 3.1f: Verify & Commit**

```bash
cargo check -p deepx-gate
cargo test -p deepx-gate
git add crates/deepx-gate/src/
git commit -m "docs(deepx-gate): add comprehensive doc comments"
```

---

#### Task 3.2: deepx-message — message store with state machine

**Files:**
- Modify: `crates/deepx-message/src/store.rs`
- Modify: `crates/deepx-message/src/effect.rs`
- Modify: `crates/deepx-message/src/lib.rs`

**Interfaces:**
- Produces: documented `MessageStore`, `Step`, `Turn`, `Effect`, `ToolExecRequest`, `ToolExecReport` for `deepx-tools` and `deepx-msglp`

- [ ] **Step 3.2a: store.rs — annotate Step, Turn, MessageStore with state machine documentation**

- [ ] **Step 3.2b: effect.rs — annotate Effect enum, PendingTool, ToolExecRequest, ToolExecReport**

- [ ] **Step 3.2c: lib.rs — add crate-level module docs**

- [ ] **Step 3.2d: Verify & Commit**

```bash
cargo check -p deepx-message
cargo test -p deepx-message
git add crates/deepx-message/src/
git commit -m "docs(deepx-message): add comprehensive doc comments"
```

---

#### Task 3.3: deepx-tools — tool execution engine

**Files:**
- Modify: `crates/deepx-tools/src/lib.rs`
- Modify: `crates/deepx-tools/src/authorization.rs`
- Modify: `crates/deepx-tools/src/permission.rs`
- Modify: `crates/deepx-tools/src/execution.rs`
- Modify: `crates/deepx-tools/src/manager.rs`
- Modify: `crates/deepx-tools/src/registration.rs`
- Modify: `crates/deepx-tools/src/runtime.rs`
- Modify: `crates/deepx-tools/src/ask_user.rs`
- Modify: `crates/deepx-tools/src/audit.rs`
- Modify: `crates/deepx-tools/src/file_query.rs`
- Modify: `crates/deepx-tools/src/file_mutate.rs`
- Modify: `crates/deepx-tools/src/file_cache.rs`
- Modify: `crates/deepx-tools/src/file_state.rs`
- Modify: `crates/deepx-tools/src/file_shared.rs`
- Modify: `crates/deepx-tools/src/exec.rs`
- Modify: `crates/deepx-tools/src/git.rs`
- Modify: `crates/deepx-tools/src/explore.rs`
- Modify: `crates/deepx-tools/src/plan.rs`
- Modify: `crates/deepx-tools/src/task.rs`
- Modify: `crates/deepx-tools/src/workspace.rs`
- Modify: `crates/deepx-tools/src/skill.rs`
- Modify: `crates/deepx-tools/src/process_inspect.rs`
- Modify: `crates/deepx-tools/src/process_registry.rs`
- Modify: `crates/deepx-tools/src/agentfs_bridge.rs`
- Modify: `crates/deepx-tools/src/auth.rs`

**Interfaces:**
- Produces: documented tool system consumed by `deepx-msglp` and `deepx-subagent`

- [ ] **Step 3.3a: lib.rs — annotate ToolRisk, JsonArgs trait, ToolHandler, ToolCallCtx, ToolEffect, ToolResult, ExecProgressEvent, ExecOutputStream, ExecProgressSender**

- [ ] **Step 3.3b: authorization.rs — annotate ToolInvocation, AuthorizedToolCall, Admission, PermissionChallenge, ApprovalError, admit()**

- [ ] **Step 3.3c: permission.rs — annotate ToolCategory, PermissionRisk, categorize_tool(), PermissionLevel, PermissionDecision, TrustedFolderSet, needs_permission(), extract_target_paths()**

- [ ] **Step 3.3d: execution.rs — annotate ToolExecResult, execute_authorized(), execute_with_context()**

- [ ] **Step 3.3e: manager.rs — annotate ToolManager singleton and methods**

- [ ] **Step 3.3f: registration.rs — annotate tool registration helpers**

- [ ] **Step 3.3g: runtime.rs — annotate RuntimeContext, set_context(), clear_context(), set_mode(), init_tools()**

- [ ] **Step 3.3h: ask_user.rs — annotate NormalizedAskMode, NormalizedAskQuestion, NormalizedAsk, AskUserError, normalize_ask_user()**

- [ ] **Step 3.3i: audit.rs — annotate AuditEntry, append_audit(), hash_args(), maybe_log_exec()**

- [ ] **Step 3.3j: Tool modules — for each tool module (file_query, file_mutate, file_cache, file_state, exec, git, explore, plan, task, workspace, skill, process_inspect, process_registry), annotate the register() function and key handler functions**

- [ ] **Step 3.3k: agentfs_bridge.rs and auth.rs — annotate bridge and auth functions**

- [ ] **Step 3.3l: Verify & Commit**

```bash
cargo check -p deepx-tools
cargo test -p deepx-tools
git add crates/deepx-tools/src/
git commit -m "docs(deepx-tools): add comprehensive doc comments to all tool modules"
```

---

#### Task 3.4: deepx-subagent — sub-agent spawning

**Files:**
- Modify: `crates/deepx-subagent/src/lib.rs` (~389L)

**Interfaces:**
- Produces: documented subagent tool for `deepx-msglp`

- [ ] **Step 3.4a: Annotate handle_spawn_subagent() with process lifecycle details**

- [ ] **Step 3.4b: Verify & Commit**

```bash
cargo check -p deepx-subagent
cargo test -p deepx-subagent
git add crates/deepx-subagent/src/lib.rs
git commit -m "docs(deepx-subagent): add comprehensive doc comments"
```

---

### Phase 4: Orchestrator & UI Layer

---

#### Task 4.1: deepx-msglp — message loop driver (Ring architecture)

**Files:**
- Modify: `crates/deepx-msglp/src/agent.rs`
- Modify: `crates/deepx-msglp/src/lifecycle.rs`
- Modify: `crates/deepx-msglp/src/logger.rs`
- Modify: `crates/deepx-msglp/src/skill_context.rs`
- Modify: `crates/deepx-msglp/src/conflict.rs`
- Modify: `crates/deepx-msglp/src/dashboard.rs`
- Modify: `crates/deepx-msglp/src/notification.rs`
- Modify: `crates/deepx-msglp/src/toast_com.rs`
- Modify: `crates/deepx-msglp/src/util.rs`
- Modify: `crates/deepx-msglp/src/new/loop_core.rs`
- Modify: `crates/deepx-msglp/src/new/engine_tool.rs`
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs`
- Modify: `crates/deepx-msglp/src/new/engine_session.rs`
- Modify: `crates/deepx-msglp/src/new/engine_input.rs`
- Modify: `crates/deepx-msglp/src/new/engine_compact.rs`
- Modify: `crates/deepx-msglp/src/new/engine_misc.rs`
- Modify: `crates/deepx-msglp/src/new/types.rs`
- Modify: `crates/deepx-msglp/src/new/engine.rs`
- Modify: `crates/deepx-msglp/src/new/mod.rs`
- Modify: `crates/deepx-msglp/src/lib.rs`

**Interfaces:**
- Produces: documented agent orchestrator for `deepx-tauri`

- [ ] **Step 4.1a: agent.rs — annotate AgentState fields, PendingApproval, TurnResumeState**

- [ ] **Step 4.1b: lifecycle.rs — annotate init_session(), create_session(), create_session_with_seed()**

- [ ] **Step 4.1c: logger.rs — annotate init_agent_logger()**

- [ ] **Step 4.1d: skill_context.rs — annotate SkillRuntimeState, SkillRuntimeInfo, SkillTurnSnapshot, SkillContextManager**

- [ ] **Step 4.1e: conflict.rs — annotate conflict detection logic**

- [ ] **Step 4.1f: dashboard.rs — annotate build_documents(), build_recent_edits(), build_tasks()**

- [ ] **Step 4.1g: notification.rs — annotate NotifyMessage**

- [ ] **Step 4.1h: toast_com.rs — annotate COM notification functions (Windows)**

- [ ] **Step 4.1i: util.rs — annotate utility functions**

- [ ] **Step 4.1j: loop_core.rs — add internal method comments for complex dispatch logic, state transitions, and panic recovery**

- [ ] **Step 4.1k: engine_tool.rs — annotate BatchAdmission, PermissionDisposition, and internal methods**

- [ ] **Step 4.1l: engine_turn.rs — annotate ResumeReason, internal turn lifecycle methods**

- [ ] **Step 4.1m: engine_session.rs — annotate session lifecycle methods**

- [ ] **Step 4.1n: engine_input.rs — annotate handle_user_input**

- [ ] **Step 4.1o: engine_compact.rs — annotate two-step async compact flow**

- [ ] **Step 4.1p: engine_misc.rs — annotate miscellaneous command handlers**

- [ ] **Step 4.1q: Verify & Commit**

```bash
cargo check -p deepx-msglp
cargo test -p deepx-msglp
git add crates/deepx-msglp/src/
git commit -m "docs(deepx-msglp): add comprehensive doc comments to all modules"
```

---

#### Task 4.2: deepx-tauri — Tauri desktop bridge

**Files:**
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/mod.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/registry.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/activity.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/platform.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/util.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/mod.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/session.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/config.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/plan.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/git.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/permission.rs`

**Interfaces:**
- Produces: documented Tauri command bridge

- [ ] **Step 4.2a: commands/session.rs — annotate all cmd_* functions with purpose, parameters, return values**

```rust
/// Send a user message to the agent for a specific session.
///
/// This is the primary input path: frontend user types a message,
/// Tauri invokes this command, which writes a `Ui2Agent::UserInput`
/// frame to the agent subprocess stdin.
///
/// # Arguments
/// * `seed` — Session identifier (8 hex chars).
/// * `text` — The raw message text from the user.
///
/// # Returns
/// `Ok(())` if the message was successfully sent to the agent process.
/// `Err(String)` if the session doesn't exist or the agent process is dead.
pub fn cmd_send_message(seed: String, text: String) -> Result<(), String> { ... }
```

- [ ] **Step 4.2b: commands/config.rs — annotate all cmd_* functions**

- [ ] **Step 4.2c: commands/plan.rs — annotate all cmd_* functions**

- [ ] **Step 4.2d: commands/git.rs — annotate all cmd_* functions**

- [ ] **Step 4.2e: commands/permission.rs — annotate all cmd_* functions**

- [ ] **Step 4.2f: registry.rs — annotate AgentRegistry methods**

- [ ] **Step 4.2g: activity.rs — annotate SessionActivityTracker**

- [ ] **Step 4.2h: platform.rs — annotate cache_system_path(), detect_os_info()**

- [ ] **Step 4.2i: util.rs — annotate utility functions**

- [ ] **Step 4.2j: Verify & Commit**

```bash
cargo check -p deepx-tauri
cargo test -p deepx-tauri
git add crates/deepx-tauri/src-tauri/src/
git commit -m "docs(deepx-tauri): add comprehensive doc comments to bridge commands"
```

---

#### Task 4.3: deepx-gate-testui — test utility

**Files:**
- Modify: `crates/deepx-gate-testui/src/` (all .rs files)

**Interfaces:**
- Produces: documented test UI (internal tool, low priority)

- [ ] **Step 4.3a: Annotate test UI modules with brief docs**

- [ ] **Step 4.3b: Verify & Commit**

```bash
cargo check -p deepx-gate-testui
cargo test -p deepx-gate-testui
git add crates/deepx-gate-testui/src/
git commit -m "docs(deepx-gate-testui): add doc comments"
```

---

### Phase 5: Final Audit

- [ ] **Step 5.1: Workspace-wide check**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 5.2: Unsafe audit — grep for all `unsafe` blocks and verify each has `// SAFETY:`**

```bash
rg "unsafe\s*\{" crates/ --type rust -l | xargs rg "SAFETY:" --type rust
```

- [ ] **Step 5.3: Clippy check**

```bash
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 5.4: Final commit**

```bash
git add -A
git commit -m "docs: complete comprehensive comment revision across all crates"
```
