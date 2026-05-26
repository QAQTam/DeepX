//! Tool execution via IPC to dsx-tools subprocess.
//!
//! The IPC connection is initialised once by runner.rs after spawning dsx-tools.
//! All tool calls go through `execute_tool()` which sends a JSON-LP frame over
//! the pipe and reads the response.

use dsx_proto::{self, AgentToTools, ToolsToAgent};
use dsx_types::{SafetyLevel, TaskPhase, ToolDef};
use std::io::{BufRead, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

/// Tools whitelist that the agent exposes to the LLM.
/// Used by runner.rs and tools_spawn.rs during init/respawn.
pub const ESSENTIAL_TOOLS: &[&str] = &[
    "exec", "read_file", "write_file", "edit_file", "edit_file_diff",
    "explore", "search", "list_dir", "glance", "ask_user", "status",
    "task_create", "task_update", "task_list",
    "plan_create", "plan_update", "plan_read", "plan_list",
    "web_fetch", "web_search", "git",
    "mem_save", "mem_read", "mem_forget", "recall",
    "pitfall_save", "pitfall_guide",
];

// ── Global flags ──

/// Global auto-mode flag, read by tool wrappers.
pub static AUTO_MODE: AtomicBool = AtomicBool::new(false);

/// Global cancel flag, set when user presses Esc.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

// ── IPC state ──

struct ToolsIpcState {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
    tool_defs: Vec<ToolDef>,
}

static TOOLS_IPC: Mutex<Option<ToolsIpcState>> = Mutex::new(None);

/// Initialise or replace the tools IPC connection.
/// Called by runner after spawning dsx-tools and completing the Init/Ready handshake.
pub fn init_tools_ipc(
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
    tool_defs: Vec<ToolDef>,
) {
    if let Ok(mut guard) = TOOLS_IPC.lock() {
        *guard = Some(ToolsIpcState { reader, writer, tool_defs });
    }
}

// ── Tool definition accessors (backed by cached Init/Ready data) ──

fn with_ipc<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut ToolsIpcState) -> R,
{
    let mut guard = TOOLS_IPC.lock().ok()?;
    guard.as_mut().map(|state| f(state))
}

/// Get all available tools from the cached definitions.
pub fn all_tools() -> Vec<ToolDef> {
    with_ipc(|state| state.tool_defs.clone()).unwrap_or_default()
}

/// Get tools for a given phase (currently returns all tools — no phase filtering).
pub fn tools_for_phase(_phase: TaskPhase) -> Vec<ToolDef> {
    all_tools()
}

// ── Execute ──

/// Execute a tool synchronously via IPC to dsx-tools.
///
/// Sends an `AgentToTools::CallReq` frame and reads the response.
///
/// Returns the tool's output content, possibly with an `[OK]`, `[ERROR]` or `[FAIL]` prefix.
pub fn execute_tool(name: &str, action: &str, args: &str) -> String {
    execute_tool_with_id(name, action, args, "")
}

/// Like `execute_tool` but passes a tool_call_id for streaming exec progress.
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
    let _has_id = !tool_call_id.is_empty();

    let result = with_ipc(|state| {
        // Check cancel before proceeding
        if CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("[CANCELLED]".to_string());
        }

        // Send CallReq
        let effective_action = if action.is_empty() { name } else { action };
        let call = AgentToTools::CallReq {
            id: call_id.clone(),
            name: name.to_string(),
            action: effective_action.to_string(),
            args: args_val.clone(),
            timeout_secs: Some(60),
        };
        if dsx_proto::write_frame(&mut state.writer, &call).is_err() {
            return Err("tools IPC write failed".to_string());
        }

        // Loop: read frames, forward Progress, return on final result
        loop {
            let response: ToolsToAgent = match dsx_proto::read_frame(&mut state.reader) {
                Ok(Some(r)) => r,
                Ok(None) => return Err("tools IPC: connection closed (EOF)".to_string()),
                Err(e) => return Err(format!("tools IPC read error: {}", e)),
            };

            // Check cancel
            if CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok("[CANCELLED] Tool execution cancelled.".to_string());
            }

            // Skip Progress frames (no-op in merged architecture)
            if let ToolsToAgent::Progress { .. } = &response {
                continue;
            }

            // Final result
            return match response {
                ToolsToAgent::Result { content, .. } => Ok(content),
                ToolsToAgent::ToolResultMessage { content, .. } => Ok(content),
                ToolsToAgent::ToolError { error, .. } => Ok(format!("[ERROR] {}", error)),
                _ => Ok("[ERROR] unexpected IPC frame type (expected tool result)".to_string()),
            };
        }
    });

    match result {
        Some(Ok(content)) => content,
        Some(Err(msg)) => {
            // IPC error — reset connection so runner can reinitialize
            if msg.contains("IPC:") || msg.contains("IPC write") || msg.contains("read error") || msg.contains("EOF") {
                reset_tools_ipc();
            }
            format!("[ERROR] {}", msg)
        }
        None => "[ERROR] tools IPC not initialised — call init_tools_ipc() first".to_string(),
    }
}

// ── Safety classification (compat heuristic; real safety lives in dsx-tools/safety.rs) ──

/// Classify a tool call's safety level based on name and args.
pub fn classify_tool(name: &str, args: &str) -> (SafetyLevel, String) {
    match name {
        "file" => {
            let action = serde_json::from_str::<serde_json::Value>(args)
                .ok()
                .and_then(|v| v.get("action").and_then(|a| a.as_str()).map(|s| s.to_string()))
                .unwrap_or_default();
            match action.as_str() {
                _ => (SafetyLevel::Safe, String::new()),
            }
        }
        // Read-only tools — always safe
        n if ["read_file", "list_dir", "search", "web",
                "explore", "list_skills", "read_skill_ref"].contains(&n) => {
            (SafetyLevel::Safe, String::new())
        }
        // Write tools — safe
        n if ["write_file", "edit_file", "edit_file_diff"].contains(&n) => {
            (SafetyLevel::Safe, String::new())
        }
        _ => (SafetyLevel::Safe, String::new()),
    }
}

// ── Session seed (forwarded via Init frame at connection time) ──

/// Set current session seed for tools subprocess.
pub fn set_current_session(_seed: &str) {
    // Session seed is sent in the AgentToTools::Init frame during connection setup;
    // subsequent seeds are propagated implicitly.
}

// ── Wrap helper ──

/// Wrap raw tool output with the tool name header.
/// Tool outputs already carry their own [OK]/[ERROR]/[FAIL] prefix,
/// so we just add the tool name line.
pub fn wrap_tool_result(name: &str, raw: &str) -> String {
    format!("{}:\n{}", name, raw)
}

// ── Async exec (tokio-based, used by orchestrator paths) ──

// ── Shutdown ──

/// Send a graceful shutdown frame to the tools subprocess.
pub fn shutdown_tools() {
    with_ipc(|state| {
        let _ = dsx_proto::write_frame(&mut state.writer, &AgentToTools::Shutdown);
    });
}

/// Reset the tools IPC state (mark as disconnected). Next with_ipc call will return None.
pub fn reset_tools_ipc() {
    if let Ok(mut guard) = TOOLS_IPC.lock() {
        *guard = None;
    }
}

/// Send cancel to the tools subprocess for the current operation.
pub fn cancel_current_tool() {
    with_ipc(|state| {
        let cancel = AgentToTools::Cancel { id: None };
        let _ = dsx_proto::write_frame(&mut state.writer, &cancel);
    });
}

// ── Exec routed through tools IPC (safety checks live in dsx-tools/safety.rs) ──
