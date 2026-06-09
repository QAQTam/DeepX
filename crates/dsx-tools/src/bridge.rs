//! Tool execution — in-process via dsx-tools::ToolManager.
//!
//! ToolManager is linked directly into the agent process, eliminating
//! IPC failures, respawn complexity, and serialization overhead.

use dsx_types::ToolDef;
use std::sync::{mpsc, Mutex, OnceLock};

/// Return type for tool execution with interrupt support.
pub struct ToolExecResult {
    pub content: String,
    pub success: bool,
    pub meta: crate::ToolExecMeta,
}

// ── Global state ──

static TOOL_MANAGER: OnceLock<Mutex<crate::ToolManager>> = OnceLock::new();

/// Initialize the in-process tool manager.
/// Must be called once at startup, before any tool execution.
pub fn init_tools(session_seed: &str, mcp_servers: &[crate::mcp_bridge::McpServerConfig]) {
    let mut mgr = crate::registration::build_tool_manager();
    mgr.apply_init(vec![], session_seed);

    if !mcp_servers.is_empty() {
        if let Err(e) = crate::mcp_bridge::register_mcp_servers(&mut mgr, mcp_servers) {
            log::warn!("dsx: failed to register MCP servers: {e}");
        }
    }

    let _ = TOOL_MANAGER.set(Mutex::new(mgr));
    log::info!("dsx: tool manager inited ({} tools)", all_tools().len());
}

pub fn set_context7_key(key: &str) {
    crate::set_c7_key(key);
}

fn with_mgr<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut crate::ToolManager) -> R,
{
    let mut guard = TOOL_MANAGER.get()?.lock().ok()?;
    Some(f(&mut guard))
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
    execute_tool_with_id_full(name, action, args, tool_call_id, None).content
}

/// Execute a tool and return the full result including any interrupt request.
/// `progress_tx` is an optional channel sender; exec tools stream stdout chunks to it.
pub fn execute_tool_with_id_full(name: &str, action: &str, args: &str, tool_call_id: &str, progress_tx: Option<mpsc::Sender<String>>) -> ToolExecResult {
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

    let source = if call_id.starts_with("dsml_tc_") {
        "DSML"
    } else if call_id.starts_with("xml_tc_") {
        "XML"
    } else {
        "native"
    };
    log::info!("tool [{source}] call: {name} (id={call_id})");

    if crate::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
        return ToolExecResult {
            content: "[CANCELLED]".to_string(),
            success: false,
            meta: crate::ToolExecMeta { name: name.to_string(), elapsed_ms: 0, output_size: 0, success: false, args_summary: String::new() },
        };
    }

    let result = with_mgr(|mgr| {
        mgr.handle_req(call_id.clone(), name, effective_action, args_val, Some(60), progress_tx)
    });

    match result {
        Some(r) => ToolExecResult { content: r.content, success: r.success, meta: r.meta },
        None => ToolExecResult {
            content: "[ERROR] tool manager not initialised — call init_tools() first".to_string(),
            success: false,
            meta: crate::ToolExecMeta { name: String::new(), elapsed_ms: 0, output_size: 0, success: false, args_summary: String::new() },
        },
    }
}

// ── Session ──

pub fn set_current_session(seed: &str) {
    crate::set_current_session(seed);
}

pub fn load_workspace(seed: &str) {
    let dir = dsx_types::platform::sessions_dir().join(seed);
    let ws = std::fs::read_to_string(dir.join("workspace.txt")).unwrap_or_default();
    let ws = ws.trim();
    if !ws.is_empty() {
        crate::set_workspace(ws);
    } else {
        crate::set_workspace(".");
    }
}

pub fn set_workspace(path: &str) {
    crate::set_workspace(path);
}

/// Execute a batch of tools in parallel (threaded).
/// Each tool gets its own thread; the Mutex serializes ToolManager access.
/// Returns (tool_call_id, ToolExecReport) pairs.
/// Simple tool executor — wraps ToolManager::handle_req for dsx-message callback.
pub fn execute_tool_simple(req: &dsx_message::ToolExecRequest) -> dsx_message::ToolExecReport {
    let result = with_mgr(|mgr| {
        mgr.handle_req(req.id.clone(), &req.name, "", req.args.clone(), Some(60), None)
    });
    match result {
        Some(r) => dsx_message::ToolExecReport { content: r.content, success: r.success, files_affected: r.files_affected },
        None => dsx_message::ToolExecReport { content: "[ERROR] ToolManager not initialised".into(), success: false, files_affected: Vec::new() },
    }
}

pub fn execute_tools_parallel(
    tools: Vec<dsx_message::ToolExecRequest>,
    progress_tx: Option<&std::sync::mpsc::Sender<String>>,
    agent_tx: Option<&std::sync::mpsc::Sender<dsx_proto::Agent2Ui>>,
) -> Vec<(String, dsx_message::ToolExecReport)> {
    if tools.len() <= 1 {
        return tools.into_iter().map(|req| {
            let report = execute_tool_simple(&req);
            (req.id, report)
        }).collect();
    }

    use std::thread;
    let handles: Vec<_> = tools.into_iter().map(|req| {
        let agent_tx = agent_tx.cloned();
        let progress_tx = progress_tx.cloned();
        thread::spawn(move || {
            let (ptx, prx) = if req.name == "exec" {
                let (tx, rx) = std::sync::mpsc::channel();
                (Some(tx), Some(rx))
            } else { (None, None) };

            let result = with_mgr(|mgr| {
                mgr.handle_req(req.id.clone(), &req.name, "", req.args.clone(), Some(60), ptx)
            });

            let report = match result {
                Some(r) => dsx_message::ToolExecReport {
                    content: r.content, success: r.success, files_affected: Vec::new(),
                },
                None => dsx_message::ToolExecReport {
                    content: "[ERROR] ToolManager not initialised".into(),
                    success: false, files_affected: Vec::new(),
                },
            };

            // Stream exec output to UI
            if let (Some(rx), Some(atx)) = (prx, agent_tx) {
                while let Ok(delta) = rx.recv() {
                    let _ = atx.send(dsx_proto::Agent2Ui::ToolExecDelta {
                        tool_call_id: req.id.clone(), delta,
                    });
                }
            }

            (req.id, report)
        })
    }).collect();

    let reports: Vec<(String, dsx_message::ToolExecReport)> = handles.into_iter().map(|h| {
        h.join().unwrap_or_else(|e| {
            let msg = format!("[ERROR] tool thread panicked: {:?}",
                e.downcast_ref::<&str>().unwrap_or(&"unknown"));
            ("unknown".into(), dsx_message::ToolExecReport {
                content: msg, success: false, files_affected: Vec::new(),
            })
        })
    }).collect();

    // Emit AuditRecord + ToolResults directly to frontend
    if let Some(atx) = agent_tx {
        let mut tool_defs = Vec::new();
        for (tc_id, report) in &reports {
            let summary = report.content.lines().next().unwrap_or(&report.content);
            let _ = atx.send(dsx_proto::Agent2Ui::AuditRecord {
                tool_name: tc_id.clone(),
                result_summary: summary.chars().take(120).collect(),
                success: report.success,
            });
            tool_defs.push(dsx_proto::ToolResultDef {
                tool_call_id: tc_id.clone(),
                output: report.content.clone(),
                success: report.success,
                file: None,
            });
        }
        if !tool_defs.is_empty() {
            let _ = atx.send(dsx_proto::Agent2Ui::ToolResults {
                turn_id: "tool_batch".into(),
                round_num: 0,
                results: tool_defs,
            });
        }
    }

    reports
}


/// Query cumulative tool stats from ToolManager.
pub fn global_stats() -> crate::ToolStats {
    with_mgr(|mgr| mgr.stats()).unwrap_or_default()
}

pub fn files_read() -> Vec<String> {
    with_mgr(|mgr| mgr.stats().files_read).unwrap_or_default()
}

pub fn files_written() -> Vec<String> {
    with_mgr(|mgr| mgr.stats().files_written).unwrap_or_default()
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
    crate::mcp_bridge::shutdown_mcp_servers();
    log::info!("dsx: tool manager shut down");
}