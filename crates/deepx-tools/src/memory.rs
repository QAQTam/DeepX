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
        key: "memory_read".to_string(),
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
        key: "memory_write".to_string(),
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
        key: "memory_clear".to_string(),
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
        _ => return ToolResult { success: false, content: crate::json_err("MISSING_SCOPE", "scope required", "Provide 'user' or 'project'.") },
    };
    let kv_key = format!("memory/{scope}");
    let content = if scope == "project" {
        crate::agentfs_bridge::try_kv_get(&kv_key)
            .unwrap_or_else(|| crate::persistence::read_project_memory())
    } else {
        crate::agentfs_bridge::try_kv_get(&kv_key)
            .unwrap_or_else(|| crate::persistence::read_global_memory(scope))
    };
    if content.trim().is_empty() {
        ToolResult { success: true, content: crate::json_ok(serde_json::json!({"scope":scope,"content":format!("memory/{} is empty.",scope)})) }
    } else {
        ToolResult { success: true, content: crate::json_ok(serde_json::json!({"scope":scope,"content":content})) }
    }
}

fn handle_write(ctx: ToolCallCtx) -> ToolResult {
    let scope = match ctx.args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        _ => return ToolResult { success: false, content: crate::json_err("MISSING_SCOPE", "scope required", "Provide 'user' or 'project'.") },
    };
    let entry = match ctx.args.get("entry").and_then(|v| v.as_str()) {
        Some(e) if !e.trim().is_empty() => e.trim(),
        _ => return ToolResult { success: false, content: crate::json_err("MISSING_ENTRY", "entry required", "Provide a non-empty entry string.") },
    };
    if scope == "project" {
        crate::persistence::append_project_memory(entry);
    } else {
        crate::persistence::append_global_memory(scope, entry);
    }
    let kv_key = format!("memory/{scope}");
    let full = if scope == "project" {
        crate::persistence::read_project_memory()
    } else {
        crate::persistence::read_global_memory(scope)
    };
    crate::agentfs_bridge::try_kv_set(&kv_key, &full);
    ToolResult { success: true, content: crate::json_ok(serde_json::json!({"scope":scope,"entry":entry,"content":format!("Appended to memory/{}: {}",scope,entry)})) }
}

fn handle_clear(ctx: ToolCallCtx) -> ToolResult {
    let scope = match ctx.args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        _ => return ToolResult { success: false, content: crate::json_err("MISSING_SCOPE", "scope required", "Provide 'user' or 'project'.") },
    };
    let kv_key = format!("memory/{scope}");
    if let Some(line) = ctx.args.get("line").and_then(|v| v.as_u64()) {
        let content = if scope == "project" {
            crate::persistence::read_project_memory()
        } else {
            crate::persistence::read_global_memory(scope)
        };
        let lines: Vec<&str> = content.lines().collect();
        let idx = line as usize;
        if idx < 1 || idx > lines.len() {
            return ToolResult {
                success: false,
                content: crate::json_err("LINE_OUT_OF_RANGE", &format!("line {line} out of range (1-{})", lines.len()), "Use memory_read first to see line numbers."),
            };
        }
        let removed = lines[idx - 1];
        let new_content: String = lines.iter()
            .enumerate()
            .filter(|(i, _)| *i != idx - 1)
            .map(|(_, l)| *l)
            .collect::<Vec<_>>()
            .join("\n");
        if scope == "project" {
            crate::persistence::write_project_memory(&new_content);
        } else {
            crate::persistence::write_global_memory(scope, &new_content);
        }
        crate::agentfs_bridge::try_kv_set(&kv_key, &new_content);
        ToolResult { success: true, content: crate::json_ok(serde_json::json!({"scope":scope,"line":line,"removed":removed,"content":format!("Deleted line {} from memory/{}",line,scope)})) }
    } else {
        if scope == "project" {
            crate::persistence::write_project_memory("");
        } else {
            crate::persistence::write_global_memory(scope, "");
        }
        crate::agentfs_bridge::try_kv_set(&kv_key, "");
        ToolResult { success: true, content: crate::json_ok(serde_json::json!({"scope":scope,"content":format!("Cleared memory/{}.",scope)})) }
    }
}

/// Format memory for injection into `[Environment]` block.
/// Returns (preferences_xml, project_xml) or empty strings.
pub fn format_memory_annotations() -> (String, String) {
    let user_mem = crate::persistence::read_global_memory("user");
    let project_mem = crate::persistence::read_project_memory();
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
