//! ToolManager: tool registration, lookup, routing, and cancellation.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::{ToolKey, ToolHandler, ToolCallCtx, CANCEL, CURRENT_SESSION, SafetyVerdict};
use dsx_proto::ToolsToAgent;

pub struct ToolManager {
    pub(crate) handlers: HashMap<ToolKey, ToolHandler>,
    allowed: Option<Vec<String>>,
    inflight_tasks: HashMap<String, Arc<AtomicBool>>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            allowed: None,
            inflight_tasks: HashMap::new(),
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
        let _ = CURRENT_SESSION.set(session_seed.to_string());
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

    pub fn handle_req(&mut self, id: String, name: &str, action: &str, args: serde_json::Value, timeout_secs: Option<u64>) -> ToolsToAgent {
        if let Some(ref allowed) = self.allowed {
            if !allowed.contains(&name.to_string()) {
                return ToolsToAgent::ToolError {
                    id, error: format!("Tool '{}' not in allowed list", name), code: "FORBIDDEN".into(),
                };
            }
        }

        let handler = match self.handlers.get(&ToolKey::new(name, action)) {
            Some(h) => h,
            None => {
                match self.handlers.iter().find(|(k, _)| k.name == name) {
                    Some((_, h)) => h,
                    None => return ToolsToAgent::ToolError {
                        id, error: format!("Unknown tool: {}/{}", name, action), code: "UNKNOWN_TOOL".into(),
                    },
                }
            }
        };

        let ctx = ToolCallCtx {
            id: id.clone(), name: name.to_string(), action: action.to_string(),
            args: args.clone(), tx_progress: None, timeout_secs,
        };
        match (handler.safety)(&ctx) {
            SafetyVerdict::Block(reason) => {
                return ToolsToAgent::ToolError { id, error: reason, code: "BLOCKED".into() };
            }
            SafetyVerdict::Allow => {}
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.inflight_tasks.insert(id.clone(), cancel_flag);

        let ctx = ToolCallCtx {
            id: id.clone(), name: name.to_string(), action: action.to_string(),
            args, tx_progress: None, timeout_secs,
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

        ToolsToAgent::ToolResultMessage {
            id, name: name.into(), action: action.into(), success, content,
        }
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
