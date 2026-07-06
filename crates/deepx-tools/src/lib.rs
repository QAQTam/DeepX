//! ToolManager framework — tool registration, execution, and lifecycle.
//!
//! Submodules register handlers via `pub fn register(mgr: &mut ToolManager)`.


pub mod exec;
pub mod pty;

pub mod explore;

pub mod bridge;
pub mod file_mutate;
pub mod file_query;
pub mod file_shared;
pub mod git_tool;
mod safety;
mod web;

pub mod ask_user;

pub mod task;

pub mod plan;
pub mod sed;

pub mod workspace;

pub mod process_registry;
pub mod process_inspect;

pub mod registration;

pub mod persistence;

pub mod memory;

pub mod manager;
/// Permission engine: tool categories, levels, trusted folders.
pub mod permission;

pub mod audit;
pub mod auth;
pub mod agentfs_bridge;

pub use web::set_c7_key;
pub use web::set_bocha_key;
pub use safety::SafetyVerdict;
pub use manager::{ToolManager, ToolExecMeta, ToolExecReport, ToolStats};

/// Risk level for tool operations, replacing per-handler safety functions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRisk {
    ReadOnly,
    Write,
    Destructive,
    Administrative,
}

// ── Macro: handler! ──

#[macro_export]
macro_rules! handler {
    ($name:ident, $exec:ident) => {
        fn $name(ctx: ToolCallCtx) -> ToolResult {
            let args = match serde_json::to_string(&ctx.args) {
                Ok(a) => a,
                Err(e) => {
                    log::error!("handler {}: serialize args failed: {e}", stringify!($name));
                    return ToolResult { success: false, content: format!("[ERROR] bad arguments: {e}") };
                }
            };
            let result = $exec(&args);
            ToolResult {
                success: !result.starts_with("[ERROR"),
                content: result,
            }
        }
    };
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

/// Tools blocked in PLAN mode. Keep in sync with permission::categorize_tool.
pub const PLAN_BLOCKED: &[&str] = &[
    "file_write", "file_edit", "file_edit_diff", "file_delete", "file_move", "file_copy",
    "exec_run", "sed",
    "git_commit", "git_push", "git_add",
];

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
    if path.is_empty() { return path.to_string(); }
    let p = Path::new(path);
    if p.is_absolute() { return path.to_string(); }
    let ws = CURRENT_WORKSPACE.read().expect("CURRENT_WORKSPACE lock");
    if ws.is_empty() || *ws == "." { return path.to_string(); }
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

// ── ToolKey ──

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolKey {
    pub name: String,
    pub action: String,
}

impl ToolKey {
    pub fn new(name: impl Into<String>, action: impl Into<String>) -> Self {
        Self { name: name.into(), action: action.into() }
    }
}

impl std::fmt::Display for ToolKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.name, self.action)
    }
}

// ── ToolCallCtx ──

#[derive(Clone)]
pub struct ToolCallCtx {
    pub id: String,
    pub name: String,
    pub action: String,
    pub args: serde_json::Value,
    pub tx_progress: Option<std::sync::mpsc::Sender<(String, String)>>,
    pub timeout_secs: Option<u64>,
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
}

// ── ToolResult ──

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self { success: true, content: content.into() }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self { success: false, content: format!("[ERROR] {}", msg.into()) }
    }
    pub fn partial(msg: impl Into<String>) -> Self {
        Self { success: false, content: format!("[PARTIAL] {}", msg.into()) }
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
    val.as_bool().or_else(|| val.as_str().and_then(|s| s.parse::<bool>().ok()))
}

// ── ToolHandler ──

#[derive(Clone)]
pub struct ToolHandler {
    pub key: ToolKey,
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
                name: self.key.name.to_string(),
                description: self.description.to_string(),
                parameters: self.input_schema.clone(),
            },
        }
    }
}
