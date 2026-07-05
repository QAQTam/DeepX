//! Cross-session memory: user preferences & project context.
//!
//! Two scopes:
//!   - `user`  — user preferences, conventions, personal style
//!   - `project` — project-specific facts, architecture decisions
//!
//! Persisted to `<data_dir>/memory/{scope}.md`, surviving session restarts.

use crate::{ToolCallCtx, ToolResult, ToolRisk};

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("memory", "read"),
        description: "Read cross-session memory. Returns persisted preferences or project facts. \
            Scope: 'user' (preferences, conventions) or 'project' (architecture, decisions). \
            Call at session start to restore context.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "scope": {"type": "string", "enum": ["user", "project"], "description": "Which memory to read."}
            },
            "required": ["scope"],
            "additionalProperties": false
        }),
        handler: handle_read,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(10),
    });

    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("memory", "write"),
        description: "Append one entry to cross-session memory. \
            Entries persist across sessions. Memory is a flat list; \
            each call appends exactly one line. Use '-' prefix for list items. \
            Example: memory write scope=user entry='- use anyhow for error handling' \
            To update an existing entry, use memory clear with line=N first then rewrite.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "scope": {"type": "string", "enum": ["user", "project"], "description": "'user'=preferences/conventions, 'project'=project facts/decisions"},
                "entry": {"type": "string", "description": "Single line to append. Use '-' prefix for bullet items. Do not embed newlines."}
            },
            "required": ["scope", "entry"],
            "additionalProperties": false
        }),
        handler: handle_write,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(10),
    });

    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("memory", "clear"),
        description: "Delete memory entries. If line=N is given, removes only that line (1-based). \
            If omitted, clears all entries in the scope. \
            Use memory read first to see current entries and their line numbers.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "scope": {"type": "string", "enum": ["user", "project"], "description": "Which scope to clear."},
                "line": {"type": "integer", "description": "Line number to delete (1-based). Omit to clear ALL entries."}
            },
            "required": ["scope"],
            "additionalProperties": false
        }),
        handler: handle_clear,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(10),
    });
}

fn handle_read(ctx: ToolCallCtx) -> ToolResult {
    let scope = match ctx.args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        _ => return ToolResult { success: false, content: "[ERROR] memory read: scope required".into() },
    };
    // Try AgentFS kv first, fall back to JSON file
    let kv_key = format!("memory/{scope}");
    let content = crate::agentfs_bridge::try_kv_get(&kv_key)
        .unwrap_or_else(|| crate::persistence::read_global_memory(scope));
    if content.trim().is_empty() {
        ToolResult { success: true, content: format!("[OK] memory/{scope} is empty.") }
    } else {
        ToolResult {
            success: true,
            content: format!("[OK] memory/{scope}:\n{content}"),
        }
    }
}

fn handle_write(ctx: ToolCallCtx) -> ToolResult {
    let scope = match ctx.args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        _ => return ToolResult { success: false, content: "[ERROR] memory write: scope required".into() },
    };
    let entry = match ctx.args.get("entry").and_then(|v| v.as_str()) {
        Some(e) if !e.trim().is_empty() => e.trim(),
        _ => return ToolResult { success: false, content: "[ERROR] memory write: entry required".into() },
    };
    crate::persistence::append_global_memory(scope, entry);
    // Mirror to AgentFS kv store (best-effort)
    let kv_key = format!("memory/{scope}");
    let full = crate::persistence::read_global_memory(scope);
    crate::agentfs_bridge::try_kv_set(&kv_key, &full);
    ToolResult { success: true, content: format!("[OK] Appended to memory/{scope}: {entry}") }
}

fn handle_clear(ctx: ToolCallCtx) -> ToolResult {
    let scope = match ctx.args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        _ => return ToolResult { success: false, content: "[ERROR] memory clear: scope required".into() },
    };
    let kv_key = format!("memory/{scope}");
    if let Some(line) = ctx.args.get("line").and_then(|v| v.as_u64()) {
        let content = crate::persistence::read_global_memory(scope);
        let lines: Vec<&str> = content.lines().collect();
        let idx = line as usize;
        if idx < 1 || idx > lines.len() {
            return ToolResult {
                success: false,
                content: format!("[ERROR] memory clear: line {line} out of range (1-{})", lines.len()),
            };
        }
        let removed = lines[idx - 1];
        let new_content: String = lines.iter()
            .enumerate()
            .filter(|(i, _)| *i != idx - 1)
            .map(|(_, l)| *l)
            .collect::<Vec<_>>()
            .join("\n");
        crate::persistence::write_global_memory(scope, &new_content);
        // Mirror to AgentFS
        crate::agentfs_bridge::try_kv_set(&kv_key, &new_content);
        ToolResult { success: true, content: format!("[OK] Deleted line {line} from memory/{scope}: {removed}") }
    } else {
        crate::persistence::write_global_memory(scope, "");
        // Mirror to AgentFS
        crate::agentfs_bridge::try_kv_set(&kv_key, "");
        ToolResult { success: true, content: format!("[OK] Cleared memory/{scope}.") }
    }
}

/// Format memory for injection into `[Environment]` block.
/// Returns (preferences_xml, project_xml) or empty strings.
pub fn format_memory_annotations() -> (String, String) {
    let user_mem = crate::persistence::read_global_memory("user");
    let project_mem = crate::persistence::read_global_memory("project");
    let prefs = if user_mem.trim().is_empty() {
        String::new()
    } else {
        format!("<preferences>\n{}\n</preferences>", user_mem.trim())
    };
    let proj = if project_mem.trim().is_empty() {
        String::new()
    } else {
        format!("<project_context>\n{}\n</project_context>", project_mem.trim())
    };
    (prefs, proj)
}
