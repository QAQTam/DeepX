//! ToolManager: tool registration, lookup, routing, and cancellation.
//!
//! Since v5: per-call execution metadata (ToolExecMeta) and cumulative
//! stats (ToolStats) are returned to the caller instead of being lost
//! to stderr. The caller (agent tools.rs) acts as a forwarding layer
//! that pushes these into UI events.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::{ToolKey, ToolHandler, ToolCallCtx, CANCEL, SafetyVerdict};

// ── Execution metadata ──

#[derive(Clone, Debug)]
pub struct ToolExecMeta {
    pub name: String,
    pub elapsed_ms: u64,
    pub output_size: usize,
    pub success: bool,
    pub args_summary: String,
}

#[derive(Clone, Debug)]
pub struct ToolExecReport {
    pub content: String,
    pub success: bool,
    pub meta: ToolExecMeta,
    pub files_affected: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolStats {
    pub calls_total: u32,
    pub failures: u32,
    pub files_read: Vec<String>,
    pub files_written: Vec<String>,
}

pub struct ToolManager {
    pub(crate) handlers: BTreeMap<ToolKey, ToolHandler>,
    allowed: Option<Vec<String>>,
    inflight_tasks: BTreeMap<String, Arc<AtomicBool>>,
    stats_total: u32,
    stats_failures: u32,
    files_read: Vec<String>,
    files_written: Vec<String>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            handlers: BTreeMap::new(),
            allowed: None,
            inflight_tasks: BTreeMap::new(),
            stats_total: 0,
            stats_failures: 0,
            files_read: Vec::new(),
            files_written: Vec::new(),
        }
    }

    pub fn register(&mut self, handler: ToolHandler) {
        self.handlers.insert(handler.key.clone(), handler);
    }

    pub fn lookup(&self, name: &str, action: &str) -> Option<&ToolHandler> {
        let key = ToolKey::new(name, action);
        self.handlers.get(&key).or_else(|| {
            if action.is_empty() {
                None
            } else {
                self.handlers.get(&ToolKey::new(name, ""))
            }
        })
    }

    pub fn apply_init(&mut self, allowed_tools: Vec<String>, session_seed: &str) {
        self.allowed = if allowed_tools.is_empty() { None } else { Some(allowed_tools) };
        crate::set_current_session(session_seed);
    }

    pub fn all_defs(&self) -> Vec<deepx_types::ToolDef> {
        let mut seen = std::collections::HashSet::new();
        let mut defs = Vec::new();
        for (key, handler) in &self.handlers {
            if seen.insert(key.name.clone()) {
                defs.push(handler.to_tool_def());
            }
        }
        defs
    }

    pub fn filtered_defs(&self) -> Vec<deepx_types::ToolDef> {
        match &self.allowed {
            Some(allowed) => self.all_defs().into_iter()
                .filter(|d| allowed.contains(&d.function.name))
                .collect(),
            None => self.all_defs(),
        }
    }

    pub fn handle_req(&mut self, id: String, name: &str, action: &str, args: serde_json::Value, timeout_secs: Option<u64>, progress_tx: Option<std::sync::mpsc::Sender<(String, String)>>) -> ToolExecReport {
        let t0 = std::time::Instant::now();

        if let Some(ref allowed) = self.allowed {
            if !allowed.contains(&name.to_string()) {
                let msg = format!("[ERROR] Tool '{}' not in allowed list", name); return ToolExecReport { success: false, content: msg.clone(), files_affected: Vec::new().clone(), meta: ToolExecMeta { name: name.to_string(), elapsed_ms: 0, output_size: msg.len(), success: false, args_summary: String::new() } };
            }
        }

        let handler = match self.handlers.get(&ToolKey::new(name, action)) {
            Some(h) => h,
            None => {
                match self.handlers.iter().find(|(k, _)| k.name == name) {
                    Some((_, h)) => h,
                    None => { let msg = format!("[ERROR] Unknown tool: {}/{}", name, action); return ToolExecReport { success: false, content: msg.clone(), files_affected: Vec::new().clone(), meta: ToolExecMeta { name: name.to_string(), elapsed_ms: 0, output_size: msg.len(), success: false, args_summary: String::new() } }; },
                }
            }
        };

        let ctx = ToolCallCtx {
            id: id.clone(), name: name.to_string(), action: action.to_string(),
            args: args.clone(), tx_progress: progress_tx.clone(), timeout_secs,
        };
        match (handler.safety)(&ctx) {
            SafetyVerdict::Block(reason) => {
                let msg = format!("[ERROR] {}", reason);
                return ToolExecReport { success: false, content: msg.clone(), files_affected: Vec::new().clone(), meta: ToolExecMeta { name: name.to_string(), elapsed_ms: 0, output_size: msg.len(), success: false, args_summary: String::new() } };
            }
            SafetyVerdict::Allow => {}
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.inflight_tasks.insert(id.clone(), cancel_flag);

        let tool_name = name.to_string();
        let audit_args = args.clone();

        let ctx = ToolCallCtx {
            id: id.clone(), name: tool_name.clone(), action: action.to_string(),
            args, tx_progress: progress_tx, timeout_secs,
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (handler.handler)(ctx)
        }));

        self.inflight_tasks.remove(&id);

        let (content, success) = match result {
            Ok(tr) => (tr.content, tr.success),
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<String>() { s.clone() }
                    else if let Some(s) = panic_info.downcast_ref::<&str>() { s.to_string() }
                    else { "unknown panic".to_string() };
                (format!("[ERROR] Tool panicked: {}", msg), false)
            }
        };

        let elapsed_ms = t0.elapsed().as_millis() as u64;
        let output_size = content.len();

        // Accumulate stats (caller retrieves via stats())
        self.stats_total += 1;
        if !success { self.stats_failures += 1; }
        let args_summary = audit_args_summary(&tool_name, &audit_args);
        let files_affected = extract_files_affected(&tool_name, &audit_args);
        if success {
            match tool_name.as_str() {
                "read_file" | "search" | "grep" | "glob" | "explore" | "list_dir" | "diff" => {
                    for f in &files_affected { if !self.files_read.contains(f) { self.files_read.push(f.clone()); } }
                }
                "write_file" | "edit_file" | "edit_file_diff" | "delete_file" | "copy_file" | "move_file" => {
                    for f in &files_affected { if !self.files_written.contains(f) { self.files_written.push(f.clone()); } }
                }
                _ => {}
            }
        }
        let meta = ToolExecMeta { name: tool_name, elapsed_ms, output_size, success, args_summary };
        ToolExecReport { success, content, meta, files_affected }
    }

    pub fn stats(&self) -> ToolStats {
        ToolStats { calls_total: self.stats_total, failures: self.stats_failures, files_read: self.files_read.clone(), files_written: self.files_written.clone() }
    }

    pub fn reset_stats(&mut self) {
        self.stats_total = 0;
        self.stats_failures = 0;
        self.files_read.clear();
        self.files_written.clear();
    }

    pub fn cancel_tool(&mut self, id: Option<&str>) {
        match id {
            Some(specific) => {
                if let Some(flag) = self.inflight_tasks.get(specific) {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            None => {
                CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                for flag in self.inflight_tasks.values() {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
    }
}

/// Extract file paths from tool args.
fn extract_files_affected(_tool_name: &str, args: &serde_json::Value) -> Vec<String> {
    let obj = match args.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };
    let mut files = Vec::new();
    if let Some(v) = obj.get("path").and_then(|v| v.as_str()) {
        files.push(v.to_string());
    }
    if let Some(arr) = obj.get("paths").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(s) = v.as_str() { files.push(s.to_string()); }
        }
    }
    for key in ["file_a", "file_b", "dest", "target"] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            files.push(v.to_string());
        }
    }
    files
}

/// Compact args summary for audit log — path and key values only.
fn audit_args_summary(_tool: &str, args: &serde_json::Value) -> String {
    let obj = match args.as_object() {
        Some(o) => o,
        None => return String::new(),
    };
    // Show path-like args first, then command, then truncate to 80 chars
    let mut parts: Vec<String> = Vec::new();
    for key in ["path", "file_a", "file_b", "dest", "target", "command", "pattern", "query", "question"] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            let short = if v.len() > 50 {
                // Show last path segment
                let seg = v.rsplit(&['/', '\\']).next().unwrap_or(v);
                format!("{key}=\"{seg}\"")
            } else {
                format!("{key}=\"{v}\"")
            };
            parts.push(short);
        }
    }
    let s = parts.join(", ");
    if s.len() > 80 {
        let end = s.floor_char_boundary(77);
        format!("{}…", &s[..end])
    } else {
        s
    }
}
