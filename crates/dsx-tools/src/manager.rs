//! ToolManager: tool registration, lookup, routing, and cancellation.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::{ToolKey, ToolHandler, ToolCallCtx, CANCEL, SafetyVerdict, ToolResult};

pub struct ToolManager {
    pub(crate) handlers: BTreeMap<ToolKey, ToolHandler>,
    allowed: Option<Vec<String>>,
    inflight_tasks: BTreeMap<String, Arc<AtomicBool>>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            handlers: BTreeMap::new(),
            allowed: None,
            inflight_tasks: BTreeMap::new(),
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

    pub fn all_defs(&self) -> Vec<dsx_types::ToolDef> {
        let mut seen = std::collections::HashSet::new();
        let mut defs = Vec::new();
        for (key, handler) in &self.handlers {
            if seen.insert(key.name.clone()) {
                defs.push(handler.to_tool_def());
            }
        }
        defs
    }

    pub fn filtered_defs(&self) -> Vec<dsx_types::ToolDef> {
        match &self.allowed {
            Some(allowed) => self.all_defs().into_iter()
                .filter(|d| allowed.contains(&d.function.name))
                .collect(),
            None => self.all_defs(),
        }
    }

    pub fn handle_req(&mut self, id: String, name: &str, action: &str, args: serde_json::Value, timeout_secs: Option<u64>, progress_tx: Option<std::sync::mpsc::Sender<String>>) -> ToolResult {
        let t0 = std::time::Instant::now();

        if let Some(ref allowed) = self.allowed {
            if !allowed.contains(&name.to_string()) {
                return ToolResult { success: false, content: format!("[ERROR] Tool '{}' not in allowed list", name), interrupt: None };
            }
        }

        let handler = match self.handlers.get(&ToolKey::new(name, action)) {
            Some(h) => h,
            None => {
                match self.handlers.iter().find(|(k, _)| k.name == name) {
                    Some((_, h)) => h,
                    None => return ToolResult { success: false, content: format!("[ERROR] Unknown tool: {}/{}", name, action), interrupt: None },
                }
            }
        };

        let ctx = ToolCallCtx {
            id: id.clone(), name: name.to_string(), action: action.to_string(),
            args: args.clone(), tx_progress: progress_tx.clone(), timeout_secs,
        };
        match (handler.safety)(&ctx) {
            SafetyVerdict::Block(reason) => {
                return ToolResult { success: false, content: format!("[ERROR] {}", reason), interrupt: None };
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

        let (content, success, interrupt) = match result {
            Ok(tr) => (tr.content, tr.success, tr.interrupt),
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<String>() { s.clone() }
                    else if let Some(s) = panic_info.downcast_ref::<&str>() { s.to_string() }
                    else { "unknown panic".to_string() };
                (format!("[ERROR] Tool panicked: {}", msg), false, None)
            }
        };

        let elapsed_ms = t0.elapsed().as_millis() as u64;
        let output_size = content.len();

        // Audit log
        let status = if success { "OK" } else { "FAIL" };
        let args_summary = audit_args_summary(&tool_name, &audit_args);
        eprintln!("[AUDIT] {tool_name}  {status}  {elapsed_ms}ms  {output_size}chars  args={{{args_summary}}}");

        ToolResult { success, content, interrupt }
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
