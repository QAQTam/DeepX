//! Tool execution — in-process via deepx-tools::ToolManager.
//!
//! ToolManager is linked directly into the agent process, eliminating
//! IPC failures, respawn complexity, and serialization overhead.

use deepx_types::ToolDef;
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
/// `extra_registrars` allows external crates to inject tools (e.g. deepx-subagent).
pub fn init_tools(session_seed: &str, mcp_servers: &[crate::mcp_bridge::McpServerConfig], extra_registrars: &[crate::registration::ToolRegistrar]) {
    let mut mgr = crate::registration::build_tool_manager(extra_registrars);
    mgr.apply_init(vec![], session_seed);

    if !mcp_servers.is_empty() {
        if let Err(e) = crate::mcp_bridge::register_mcp_servers(&mut mgr, mcp_servers) {
            log::warn!("deepx: failed to register MCP servers: {e}");
        }
    }

    let _ = TOOL_MANAGER.set(Mutex::new(mgr));
    log::info!("deepx: tool manager inited ({} tools)", all_tools().len());
}

pub fn set_context7_key(key: &str) {
    crate::set_c7_key(key);
}

pub fn set_bocha_key(key: &str) {
    crate::set_bocha_key(key);
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
///
/// Uses three-phase locking: prepare (brief lock) → execute (no lock) → finalize (brief lock),
/// so that multiple exec calls can run their subprocesses concurrently.
pub fn execute_tool_with_id_full(name: &str, action: &str, args: &str, tool_call_id: &str, progress_tx: Option<mpsc::Sender<(String, String)>>) -> ToolExecResult {
    log::info!("[BRIDGE] execute_tool_with_id_full name={} has_progress={}", name, progress_tx.is_some());
    let t0 = std::time::Instant::now();
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

    // Phase 1: prepare (brief lock)
    let prepared = with_mgr(|mgr| {
        mgr.prepare_req(call_id.clone(), name, effective_action, args_val, Some(60), progress_tx)
    });

    let prepared = match prepared {
        Some(Ok(p)) => p,
        Some(Err(report)) => return ToolExecResult { content: report.content, success: report.success, meta: report.meta },
        None => return ToolExecResult {
            content: "[ERROR] tool manager not initialised — call init_tools() first".to_string(),
            success: false,
            meta: crate::ToolExecMeta { name: String::new(), elapsed_ms: 0, output_size: 0, success: false, args_summary: String::new() },
        },
    };

    // Phase 2: execute (no lock — parallel-safe)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (prepared.handler_fn)(prepared.ctx.clone())
    }));

    let elapsed_ms = t0.elapsed().as_millis() as u64;
    let tool_result = match result {
        Ok(tr) => tr,
        Err(panic_info) => {
            let msg = if let Some(s) = panic_info.downcast_ref::<String>() { s.clone() }
                else if let Some(s) = panic_info.downcast_ref::<&str>() { s.to_string() }
                else { "unknown panic".to_string() };
            crate::ToolResult { success: false, content: format!("[ERROR] Tool panicked: {}", msg) }
        }
    };

    // Phase 3: finalize (brief lock)
    let report = with_mgr(|mgr| {
        mgr.finalize_req(prepared, tool_result, elapsed_ms)
    });

    match report {
        Some(r) => ToolExecResult { content: r.content, success: r.success, meta: r.meta },
        None => ToolExecResult {
            content: "[ERROR] tool manager not initialised".to_string(),
            success: false,
            meta: crate::ToolExecMeta { name: name.to_string(), elapsed_ms, output_size: 0, success: false, args_summary: String::new() },
        },
    }
}

// ── Session ──

pub fn set_current_session(seed: &str) {
    crate::set_current_session(seed);
}

pub fn load_workspace(seed: &str) {
    let dir = deepx_types::platform::sessions_dir().join(seed);
    let ws = std::fs::read_to_string(dir.join("workspace.txt")).unwrap_or_default();
    let ws = ws.trim();
    let ws = if !ws.is_empty() { ws } else { "." };
    crate::set_workspace(ws);
    // Set process current directory so all relative paths in exec/linuxmod/file tools
    // resolve against the workspace root instead of the installation directory.
    if let Err(e) = std::env::set_current_dir(ws) {
        log::warn!("load_workspace: cannot cd to '{}': {e}", ws);
    }
}

pub fn set_workspace(path: &str) {
    crate::set_workspace(path);
    if let Err(e) = std::env::set_current_dir(path) {
        log::warn!("set_workspace: cannot cd to '{}': {e}", path);
    }
}

/// Execute a batch of tools in parallel (threaded).
/// Each tool gets its own thread; the Mutex serializes ToolManager access.
/// Returns (tool_call_id, ToolExecReport) pairs.
/// Simple tool executor — wraps ToolManager for deepx-message callback.
/// Uses three-phase locking for parallel safety.
pub fn execute_tool_simple(req: &deepx_message::ToolExecRequest) -> deepx_message::ToolExecReport {
    let t0 = std::time::Instant::now();

    // Phase 1: prepare
    let prepared = with_mgr(|mgr| {
        mgr.prepare_req(req.id.clone(), &req.name, "", req.args.clone(), Some(60), None)
    });

    let prepared = match prepared {
        Some(Ok(p)) => p,
        Some(Err(report)) => return deepx_message::ToolExecReport { content: report.content, success: report.success, files_affected: report.files_affected },
        None => return deepx_message::ToolExecReport { content: "[ERROR] ToolManager not initialised".into(), success: false, files_affected: Vec::new() },
    };

    // Phase 2: execute (no lock)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (prepared.handler_fn)(prepared.ctx.clone())
    }));

    let elapsed_ms = t0.elapsed().as_millis() as u64;
    let tool_result = match result {
        Ok(tr) => tr,
        Err(panic_info) => {
            let msg = if let Some(s) = panic_info.downcast_ref::<String>() { s.clone() }
                else if let Some(s) = panic_info.downcast_ref::<&str>() { s.to_string() }
                else { "unknown panic".to_string() };
            crate::ToolResult { success: false, content: format!("[ERROR] Tool panicked: {}", msg) }
        }
    };

    // Phase 3: finalize
    match with_mgr(|mgr| mgr.finalize_req(prepared, tool_result, elapsed_ms)) {
        Some(r) => deepx_message::ToolExecReport { content: r.content, success: r.success, files_affected: r.files_affected },
        None => deepx_message::ToolExecReport { content: "[ERROR] ToolManager not initialised".into(), success: false, files_affected: Vec::new() },
    }
}

pub fn execute_tools_parallel(
    tools: Vec<deepx_message::ToolExecRequest>,
    progress_tx: Option<&std::sync::mpsc::Sender<(String, String)>>,
    agent_tx: Option<&std::sync::mpsc::Sender<deepx_proto::Agent2Ui>>,
) -> Vec<(String, deepx_message::ToolExecReport)> {
    if tools.len() <= 1 {
        return tools.into_iter().map(|req| {
            let report = execute_tool_simple(&req);
            (req.id, report)
        }).collect();
    }

    use std::thread;

    // Phase 1: prepare all tools (serial, brief lock per tool)
    let mut prepared: Vec<(String, crate::manager::PreparedCall)> = Vec::new();
    let mut errors: Vec<(String, deepx_message::ToolExecReport)> = Vec::new();
    for req in &tools {
        match with_mgr(|mgr| mgr.prepare_req(req.id.clone(), &req.name, "", req.args.clone(), Some(60), None)) {
            Some(Ok(p)) => prepared.push((req.id.clone(), p)),
            Some(Err(report)) => {
                errors.push((req.id.clone(), deepx_message::ToolExecReport {
                    content: report.content, success: false, files_affected: Vec::new(),
                }));
            }
            None => {
                errors.push((req.id.clone(), deepx_message::ToolExecReport {
                    content: "[ERROR] ToolManager not initialised".into(), success: false, files_affected: Vec::new(),
                }));
            }
        }
    }

    // If all tools failed in prepare, just return errors
    if prepared.is_empty() {
        return errors;
    }

    // Phase 2: execute all in parallel threads (no lock)
    let handles: Vec<_> = prepared.into_iter().map(|(tc_id, pcall)| {
        let agent_tx = agent_tx.cloned();
        let _progress_tx = progress_tx.cloned();
        let req_id = tc_id.clone();
        thread::spawn(move || {
            let t0 = std::time::Instant::now();
            let (ptx, prx) = if pcall.name == "exec" {
                let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
                (Some(tx), Some(rx))
            } else { (None, None) };
            // ptx would be passed to prepare_req in a full implementation;
            // currently progress streaming is handled via the channel pair.
            drop(ptx); // close sender so rx.recv() won't block forever

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (pcall.handler_fn)(pcall.ctx.clone())
            }));

            let elapsed_ms = t0.elapsed().as_millis() as u64;
            let tool_result = match result {
                Ok(tr) => tr,
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<String>() { s.clone() }
                        else if let Some(s) = panic_info.downcast_ref::<&str>() { s.to_string() }
                        else { "unknown panic".to_string() };
                    crate::ToolResult { success: false, content: format!("[ERROR] Tool panicked: {}", msg) }
                }
            };

            // Phase 3: finalize (brief lock)
            let report = match with_mgr(|mgr| mgr.finalize_req(pcall, tool_result, elapsed_ms)) {
                Some(r) => deepx_message::ToolExecReport {
                    content: r.content, success: r.success, files_affected: r.files_affected,
                },
                None => deepx_message::ToolExecReport {
                    content: "[ERROR] ToolManager not initialised".into(),
                    success: false, files_affected: Vec::new(),
                },
            };

            // Stream exec output to UI
            if let (Some(rx), Some(atx)) = (prx, agent_tx) {
                while let Ok((_id, delta)) = rx.recv() {
                    let _ = atx.send(deepx_proto::Agent2Ui::ToolExecDelta {
                        tool_call_id: req_id.clone(), delta,
                    });
                }
            }

            (req_id, report)
        })
    }).collect();

    let mut reports: Vec<(String, deepx_message::ToolExecReport)> = handles.into_iter().map(|h| {
        h.join().unwrap_or_else(|e| {
            let msg = format!("[ERROR] tool thread panicked: {:?}",
                e.downcast_ref::<&str>().unwrap_or(&"unknown"));
            ("unknown".into(), deepx_message::ToolExecReport {
                content: msg, success: false, files_affected: Vec::new(),
            })
        })
    }).collect();
    reports.append(&mut errors);

    // Emit AuditRecord + ToolResults directly to frontend
    if let Some(atx) = agent_tx {
        let mut tool_defs = Vec::new();
        for (tc_id, report) in &reports {
            let summary = report.content.lines().next().unwrap_or(&report.content);
            let _ = atx.send(deepx_proto::Agent2Ui::AuditRecord {
                tool_name: tc_id.clone(),
                result_summary: summary.chars().take(120).collect(),
                success: report.success,
            });
            tool_defs.push(deepx_proto::ToolResultDef {
                tool_call_id: tc_id.clone(),
                output: report.content.clone(),
                success: report.success,
                file: None,
            });
        }
        if !tool_defs.is_empty() {
            let _ = atx.send(deepx_proto::Agent2Ui::ToolResults {
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

pub fn all_tasks() -> Vec<deepx_proto::TaskInfo> {
    crate::task::get_task_infos()
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
    log::info!("deepx: tool manager shut down");
}