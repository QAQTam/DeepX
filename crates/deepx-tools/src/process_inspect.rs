//! Process inspection tools — check / wait / kill tracked processes.
//!
//! Registered under the `check_process`, `wait_process`, `kill_process` names.
//! These let the LLM inspect long-running exec/subagent processes that
//! hit their timeout, instead of blindly retrying or killing.

use crate::{ToolCallCtx, ToolResult, process_registry::ProcessRegistry};

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("process", "check"),
        description: "Process management: check, wait, kill. Use check to inspect a running background process (exec or subagent). \
            Returns status, elapsed time, and the last output tail. \
            Use when a previous exec/subagent timed out — you can peek at whether it's \
            still making progress or has stalled.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Process ID returned by the timed-out exec/subagent call."}
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        handler: handle_check,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(10),
    });

    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("process", "wait"),
        description: "Wait for a background process to complete, with a timeout. \
            Returns the final output when the process exits, or a snapshot if timeout is reached. \
            Use when you want to give a previously-timed-out process more time to finish.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Process ID to wait for."},
                "timeout_secs": {"type": "integer", "description": "Max seconds to wait. Default 120."}
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        handler: handle_wait,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(180),
    });

    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("process","kill"),
        description: "Kill a process",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Process ID to kill."}
            },
            "required": ["id"],
            "additionalProperties": false
        }),
        handler: handle_kill,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}

fn handle_check(ctx: ToolCallCtx) -> ToolResult {
    let id: u32 = match ctx.args.get("id").and_then(|v| v.as_u64()) {
        Some(v) if v <= u32::MAX as u64 => v as u32,
        _ => return ToolResult { success: false, content: "[ERROR] check_process: id required".into() },
    };

    match ProcessRegistry::get_info(id) {
        Some(info) => ToolResult {
            success: true,
            content: format!("[OK]\n{}", serde_json::to_string_pretty(&info).unwrap_or_else(|_| format!("{:?}", info))),
        },
        None => ToolResult {
            success: false,
            content: format!("[ERROR] check_process: process {id} not found (may have already exited and been cleaned up)"),
        },
    }
}

fn handle_wait(ctx: ToolCallCtx) -> ToolResult {
    let id: u32 = match ctx.args.get("id").and_then(|v| v.as_u64()) {
        Some(v) if v <= u32::MAX as u64 => v as u32,
        _ => return ToolResult { success: false, content: "[ERROR] wait_process: id required".into() },
    };
    let timeout_secs: u64 = ctx.args.get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(120);

    match ProcessRegistry::wait_for(id, timeout_secs) {
        Some(info) => ToolResult {
            success: true,
            content: format!("[OK]\n{}", serde_json::to_string_pretty(&info).unwrap_or_else(|_| format!("{:?}", info))),
        },
        None => ToolResult {
            success: false,
            content: format!("[ERROR] wait_process: process {id} not found"),
        },
    }
}

fn handle_kill(ctx: ToolCallCtx) -> ToolResult {
    let id: u32 = match ctx.args.get("id").and_then(|v| v.as_u64()) {
        Some(v) if v <= u32::MAX as u64 => v as u32,
        _ => return ToolResult { success: false, content: "[ERROR] kill_process: id required".into() },
    };

    if ProcessRegistry::kill(id) {
        ToolResult {
            success: true,
            content: format!("[OK] Process {id} killed."),
        }
    } else {
        ToolResult {
            success: false,
            content: format!("[ERROR] kill_process: process {id} not found or already exited"),
        }
    }
}
