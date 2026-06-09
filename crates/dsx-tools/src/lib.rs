//! ToolManager framework — tool registration, execution, and lifecycle.
//!
//! Submodules register handlers via `pub fn register(mgr: &mut ToolManager)`.

pub mod exec;
pub mod explore;
pub mod file_read;
pub mod file_write;
pub mod file_edit;
pub mod sed;
pub mod grep;
pub mod file_edit_diff;
pub mod file_list_dir;
pub mod file_search;
pub mod file_delete;
pub mod file_move;
pub mod file_glob;
pub mod file_diff;
pub mod file_shared;
mod safety;
mod web;
pub mod task;
pub mod registration;
pub mod persistence;
pub mod manager;
pub mod mcp_bridge;

pub use web::set_c7_key;
pub use safety::SafetyVerdict;
pub use manager::{ToolManager, ToolExecMeta, ToolExecReport, ToolStats};

/// Default tool safety check: always allow.
pub fn default_allow(_: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
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
            ToolResult::ok($exec(&args))
        }
    };
}

use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use dsx_types::ToolDef;

// ── Global state ──

pub static CANCEL: AtomicBool = AtomicBool::new(false);
pub static CURRENT_SESSION: Mutex<Option<String>> = Mutex::new(None);

pub fn set_current_session(seed: &str) {
    let mut guard = CURRENT_SESSION.lock().unwrap();
    *guard = Some(seed.to_string());
}

pub static CURRENT_WORKSPACE: OnceLock<String> = OnceLock::new();

pub fn set_workspace(path: &str) {
    let _ = CURRENT_WORKSPACE.set(path.to_string());
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

pub struct ToolCallCtx {
    pub id: String,
    pub name: String,
    pub action: String,
    pub args: serde_json::Value,
    pub tx_progress: Option<std::sync::mpsc::Sender<String>>,
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
}

// ── parse helpers ──

pub fn parse_arg(args: &str, key: &str) -> String {
    dsx_types::arg::parse_arg(args, key).unwrap_or_default()
}

pub fn parse_arg_or(args: &str, key: &str, default: &str) -> String {
    dsx_types::arg::parse_arg_or(args, key, default)
}

pub fn parse_opt(args: &str, key: &str) -> Option<String> {
    dsx_types::arg::parse_arg(args, key)
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
    pub safety: fn(&ToolCallCtx) -> SafetyVerdict,
    pub default_timeout: Duration,
}

impl ToolHandler {
    pub fn to_tool_def(&self) -> ToolDef {
        ToolDef {
            call_type: "function".into(),
            function: dsx_types::ToolFunction {
                name: self.key.name.to_string(),
                description: self.description.to_string(),
                parameters: self.input_schema.clone(),
            },
        }
    }
}
