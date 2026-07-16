//! ToolManager framework — tool registration, execution, and lifecycle.
//!
//! Submodules register handlers via `pub fn register(mgr: &mut ToolManager)`.

pub mod exec;

pub mod explore;

pub mod authorization;
mod code_delta;
pub mod execution;
pub mod file_cache;
pub mod file_mutate;
pub mod file_query;
pub mod file_shared;
pub mod file_state;
pub mod git;
pub mod runtime;
mod safety;
pub mod skill;
mod web;

pub mod ask_user;

pub mod task;

pub mod plan;

pub mod workspace;

pub mod process_inspect;
pub mod process_registry;

pub mod registration;

pub mod manager;
/// Permission engine: tool categories, levels, trusted folders.
pub mod permission;

pub mod agentfs_bridge;
pub mod audit;
pub mod auth;

pub use manager::{ToolExecMeta, ToolExecReport, ToolManager, ToolStats};
pub use safety::SafetyVerdict;

/// Return current time as "UTC+8 YYYY-MM-DD HH:MM" (matching the [timeis:] prefix convention).
pub fn now_utc8() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() + 8 * 3600;
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let (y, m, d) = deepx_types::platform::civil_from_days(days as i64);
    format!("UTC+8 {y:04}-{m:02}-{d:02} {hours:02}:{minutes:02}")
}

/// Build a JSON success response for tools that only need a status message.
/// Extra fields can be added via `extra`.
pub fn json_ok(extra: serde_json::Value) -> String {
    let mut v = serde_json::json!({"timeis": now_utc8(), "status": "ok"});
    if let Some(obj) = v.as_object_mut() {
        if let Some(ext) = extra.as_object() {
            for (k, val) in ext {
                obj.insert(k.clone(), val.clone());
            }
        }
    }
    v.to_string()
}

/// Build a JSON error response.
pub fn json_err(
    code: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> String {
    serde_json::json!({
        "timeis": now_utc8(),
        "status": "error",
        "code": code.into(),
        "message": message.into(),
        "hint": hint.into(),
    })
    .to_string()
}

/// Risk level for tool operations, replacing per-handler safety functions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRisk {
    ReadOnly,
    Write,
    Destructive,
    Administrative,
}

// ── Macro: handler! (v2 — direct Value access, no double serialization) ──

#[macro_export]
macro_rules! handler {
    ($name:ident, $exec:ident) => {
        fn $name(ctx: ToolCallCtx) -> ToolResult {
            let result = $exec(&ctx.args);
            let is_json = result.trim_start().starts_with('{');
            let success = if is_json {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                    v.get("status").and_then(|s| s.as_str()) != Some("error")
                } else {
                    false
                }
            } else {
                // Treat any failure-prefix as error; [PARTIAL] is also a failure.
                let trimmed = result.trim_start();
                !(trimmed.starts_with("[ERROR") || trimmed.starts_with("[PARTIAL"))
            };
            let content = if is_json {
                result
            } else {
                result // plain text — pass through directly, [OK]/[PARTIAL]/[ERROR] speak for themselves
            };
            ToolResult { success, content }
        }
    };
}

// ── JsonArgs trait: typed access to tool arguments ──

pub trait JsonArgs {
    fn s(&self, key: &str) -> String;
    fn s_or(&self, key: &str, default: &str) -> String;
    fn opt_bool(&self, key: &str) -> Option<bool>;
}

impl JsonArgs for serde_json::Value {
    fn s(&self, key: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default()
    }
    fn s_or(&self, key: &str, default: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| default.to_string())
    }
    fn opt_bool(&self, key: &str) -> Option<bool> {
        let val = self.get(key)?;
        val.as_bool()
            .or_else(|| val.as_str().and_then(|s| s.parse::<bool>().ok()))
    }
}

use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use deepx_types::ToolDef;

// ── Global state ──

/// Global cancel flag for tool execution.
///
/// Set at the start of every interrupt (Cancel, session switch, shutdown)
/// from the reader thread and the main loop. Checked by bridge before
/// executing each tool. Reset at the top of [`crate::Loop::handle_user_input`]
/// so it is per-turn, not cross-session.
pub static CANCEL: AtomicBool = AtomicBool::new(false);
pub static CURRENT_SESSION: Mutex<Option<String>> = Mutex::new(None);

pub fn set_current_session(seed: &str) {
    let mut guard = CURRENT_SESSION.lock().expect("CURRENT_SESSION lock");
    *guard = Some(seed.to_string());
}

pub static CURRENT_WORKSPACE: RwLock<String> = RwLock::new(String::new());

/// Unit tests mutate process-wide runtime state. Keep those mutations
/// deterministic even when the Rust test harness runs modules in parallel.
#[cfg(test)]
pub(crate) static TEST_RUNTIME_SERIAL: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

/// Tools blocked in PLAN mode. Keep in sync with permission::categorize_tool.
pub const PLAN_BLOCKED: &[&str] = &["edit", "edit_block", "write", "delete", "exec_run", "git"];

pub fn set_workspace(path: &str) {
    let mut ws = CURRENT_WORKSPACE.write().expect("CURRENT_WORKSPACE lock");
    *ws = path.to_string();
}

/// Resolve a path against the workspace root.
/// If the path is already absolute, return as-is.
/// If the workspace is empty or ".", return the path as-is (OS cwd resolution).
/// Otherwise, join the workspace root with the relative path.
pub fn resolve_workspace_path(path: &str) -> String {
    use std::path::Path;
    if path.is_empty() {
        return path.to_string();
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    let ws = CURRENT_WORKSPACE.read().expect("CURRENT_WORKSPACE lock");
    if ws.is_empty() || *ws == "." {
        return path.to_string();
    }
    let joined = Path::new(&*ws).join(p);
    // Normalise: strip redundant . components via iterator (e.g. D:\foo\./bar → D:\foo\bar)
    let normalized: std::path::PathBuf = joined.components().collect();
    normalized.to_string_lossy().to_string()
}

/// Convert an absolute path into a display-friendly relative path.
///
/// Strips the workspace root prefix and uses `/` separators for
/// cross-platform consistency (like Git, VS Code, etc.).
///
/// # Examples
/// ```ignore
/// // workspace = D:\project\DeepX
/// display_path("D:\\project\\DeepX\\crates\\foo\\bar.rs") → "crates/foo/bar.rs"
/// display_path("/home/user/project/src/main.rs")          → "src/main.rs"
/// ```
pub fn display_path(abs_path: &str) -> String {
    // Normalise input: strip redundant . components
    let normalized: std::path::PathBuf = std::path::Path::new(abs_path).components().collect();
    let norm_str = normalized.to_string_lossy();

    let ws = CURRENT_WORKSPACE.read().expect("CURRENT_WORKSPACE lock");
    let ws = ws.trim_end_matches(['/', '\\']);

    if !ws.is_empty() && ws != "." {
        // Case-insensitive prefix match on Windows
        let ws_lower = ws.to_lowercase();
        let p_lower = norm_str.to_lowercase();
        if p_lower.starts_with(&ws_lower) {
            let rel = &norm_str[ws.len()..].trim_start_matches(['/', '\\']);
            return rel.replace('\\', "/");
        }
    }
    // Not under workspace — return normalised path with forward slashes
    norm_str.replace('\\', "/")
}

// ── ToolCallCtx ──

/// A single non-blocking execution-output update for the frontend.
///
/// `seq` is monotonic per execution and represents the order in which pipe
/// reader threads observed chunks.  Cross-stream ordering is therefore local
/// observation order, while ordering within a stream is exact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecProgressEvent {
    pub tool_call_id: String,
    pub stream: ExecOutputStream,
    pub seq: u64,
    pub chunk: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecOutputStream {
    Stdout,
    Stderr,
}

impl ExecOutputStream {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// Bounded, lossy progress sender. Pipe readers must never wait for a slow UI.
#[derive(Clone)]
pub struct ExecProgressSender {
    tx: std::sync::mpsc::SyncSender<ExecProgressEvent>,
    dropped_bytes: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ExecProgressSender {
    pub fn try_send(&self, event: ExecProgressEvent) {
        let bytes = event.chunk.len() as u64;
        if self.tx.try_send(event).is_err() {
            self.dropped_bytes
                .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub const EXEC_PROGRESS_CHANNEL_CAPACITY: usize = 256;

pub fn bounded_exec_progress_channel() -> (
    ExecProgressSender,
    std::sync::mpsc::Receiver<ExecProgressEvent>,
) {
    let (tx, rx) = std::sync::mpsc::sync_channel(EXEC_PROGRESS_CHANNEL_CAPACITY);
    let dropped_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    (ExecProgressSender { tx, dropped_bytes }, rx)
}

#[derive(Clone)]
pub struct ToolCallCtx {
    pub id: String,
    pub name: String,
    pub action: String,
    pub args: serde_json::Value,
    pub tx_progress: Option<ExecProgressSender>,
    pub timeout_secs: Option<u64>,
    /// Per-invocation cancellation signal owned by ToolManager.
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Private, typed effects returned to the host runtime. Sharing this cell
    /// across context clones avoids parsing trusted effects from tool text.
    pub(crate) skill_effects: std::sync::Arc<std::sync::Mutex<Vec<ToolEffect>>>,
}

/// Trusted, typed state transitions emitted by tool handlers.
///
/// Keeping this wrapper generic lets the runtime add other effect families
/// without widening the textual tool-result protocol.
#[derive(Clone, Debug)]
pub enum ToolEffect {
    Skill(deepx_skills::SkillEffect),
}

impl ToolCallCtx {
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.args.get(key).and_then(|v| v.as_str())
    }
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.args.get(key).and_then(|v| v.as_u64())
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.args.get(key).and_then(|v| v.as_bool())
    }
    pub(crate) fn push_skill_effect(&self, effect: deepx_skills::SkillEffect) {
        self.skill_effects
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(ToolEffect::Skill(effect));
    }
    pub(crate) fn take_skill_effects(&self) -> Vec<ToolEffect> {
        std::mem::take(
            &mut *self
                .skill_effects
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }
}

// ── ToolResult ──

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            success: true,
            content: content.into(),
        }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            content: format!("[ERROR] {}", msg.into()),
        }
    }
    pub fn partial(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            content: format!("[PARTIAL] {}", msg.into()),
        }
    }
}

// ── parse helpers ──

pub fn parse_arg(args: &str, key: &str) -> String {
    deepx_types::arg::parse_arg(args, key).unwrap_or_default()
}

pub fn parse_arg_or(args: &str, key: &str, default: &str) -> String {
    deepx_types::arg::parse_arg_or(args, key, default)
}

pub fn parse_opt(args: &str, key: &str) -> Option<String> {
    deepx_types::arg::parse_arg(args, key)
}

pub fn parse_opt_bool(args: &str, key: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(args).ok()?;
    let val = v.get(key)?;
    val.as_bool()
        .or_else(|| val.as_str().and_then(|s| s.parse::<bool>().ok()))
}

// ── ToolHandler ──

#[derive(Clone)]
pub struct ToolHandler {
    pub key: String,
    pub description: &'static str,
    pub input_schema: serde_json::Value,
    pub handler: fn(ToolCallCtx) -> ToolResult,
    pub risk: ToolRisk,
    pub default_timeout: Duration,
}

impl ToolHandler {
    pub fn to_tool_def(&self) -> ToolDef {
        ToolDef {
            call_type: "function".into(),
            function: deepx_types::ToolFunction {
                name: self.key.clone(),
                description: self.description.to_string(),
                parameters: self.input_schema.clone(),
            },
        }
    }
}
