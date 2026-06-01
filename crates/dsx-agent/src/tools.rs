//! Tool execution — in-process via dsx-tools::ToolManager.
//!
//! ToolManager is linked directly into the agent process, eliminating
//! IPC failures, respawn complexity, and serialization overhead.

use dsx_proto::ToolsToAgent;
use dsx_types::ToolDef;
use std::sync::Mutex;

// ── Global state ──

static TOOL_MANAGER: Mutex<Option<dsx_tools::ToolManager>> = Mutex::new(None);

/// Initialize the in-process tool manager.
/// Must be called once at startup, before any tool execution.
pub fn init_tools(session_seed: &str) {
    let mut mgr = dsx_tools::registration::build_tool_manager();
    mgr.apply_init(vec![], session_seed);
    if let Ok(mut guard) = TOOL_MANAGER.lock() {
        *guard = Some(mgr);
    }
    // Context7 key: set from config if available (override may come later)
    log::info!("dsx: tool manager inited ({} tools)", all_tools().len());
}

pub fn set_context7_key(key: &str) {
    dsx_tools::set_c7_key(key);
}

fn with_mgr<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut dsx_tools::ToolManager) -> R,
{
    let mut guard = TOOL_MANAGER.lock().ok()?;
    guard.as_mut().map(|mgr| f(mgr))
}

// ── Tool definition accessors ──

pub fn all_tools() -> Vec<ToolDef> {
    with_mgr(|mgr| mgr.all_defs()).unwrap_or_default()
}

// ── Execute ──

pub fn execute_tool(name: &str, action: &str, args: &str) -> String {
    execute_tool_with_id(name, action, args, "")
}

pub fn execute_tool_with_id(name: &str, action: &str, args: &str, tool_call_id: &str) -> String {
    let args_val: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let call_id = if tool_call_id.is_empty() {
        format!("agent_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0))
    } else {
        tool_call_id.to_string()
    };
    let effective_action = if action.is_empty() { name } else { action };

    // Check cancel before proceeding
    if dsx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
        return "[CANCELLED]".to_string();
    }

    let result = with_mgr(|mgr| {
        mgr.handle_req(call_id, name, effective_action, args_val, Some(60))
    });

    match result {
        Some(ToolsToAgent::ToolResultMessage { content, .. }) => {
            content
        }
        Some(ToolsToAgent::ToolError { error, .. }) => {
            format!("[ERROR] {}", error)
        }
        Some(_) => "[ERROR] unexpected response from tool manager".to_string(),
        None => "[ERROR] tool manager not initialised — call init_tools() first".to_string(),
    }
}

// ── Session ──

pub fn set_current_session(seed: &str) {
    dsx_tools::set_current_session(seed);
}

pub fn load_workspace(seed: &str) {
    let dir = dsx_types::platform::sessions_dir().join(seed);
    let ws = std::fs::read_to_string(dir.join("workspace.txt")).unwrap_or_default();
    let ws = ws.trim();
    if !ws.is_empty() {
        dsx_tools::set_workspace(ws);
    } else {
        dsx_tools::set_workspace(".");
    }
}

pub fn set_workspace(path: &str) {
    dsx_tools::set_workspace(path);
}

// ── Wrap helper ──

pub fn wrap_tool_result(name: &str, raw: &str) -> String {
    format!("{}:\n{}", name, raw)
}

// ── Cancel ──

pub fn cancel_current_tool() {
    with_mgr(|mgr| mgr.cancel_tool(None));
}

// ── Shutdown ──

pub fn shutdown_tools() {
    // No subprocess to kill — just clear the manager
    if let Ok(mut guard) = TOOL_MANAGER.lock() {
        *guard = None;
    }
}

