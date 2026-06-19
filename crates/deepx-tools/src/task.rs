//! Task management: create, update, delete, list tasks.
//! Format: "- [status] T{id}: subject — description"

use super::{parse_arg, parse_opt};
use std::sync::Mutex;

static TASK_LOCK: Mutex<()> = Mutex::new(());

fn current_seed() -> String {
    crate::CURRENT_SESSION.lock().unwrap().clone().unwrap_or_default()
}

fn read_tasks() -> Vec<String> {
    let seed = current_seed();
    if seed.is_empty() { return Vec::new(); }
    let content = crate::persistence::read_memory(&seed, "tasks");
    content
        .lines()
        .filter(|l| l.starts_with("- [") && !l.trim().is_empty())
        .map(String::from)
        .collect()
}

fn write_tasks(lines: &[String]) {
    let seed = current_seed();
    if seed.is_empty() { return; }
    crate::persistence::write_memory(&seed, "tasks", &lines.join("\n"));
}

fn next_id(lines: &[String]) -> u32 {
    let mut max = 0u32;
    for l in lines {
        if let Some(rest) = l.trim_start_matches("- [").split_once("] ") {
            let body = rest.1.trim_start();
            if let Some(id_str) = body.splitn(2, ':').next() {
                if id_str.starts_with('T') {
                    if let Ok(n) = id_str.strip_prefix('T').unwrap_or("").parse::<u32>() {
                        if n > max { max = n; }
                    }
                }
            }
        }
    }
    max + 1
}

fn find_task(lines: &[String], id: u32) -> Option<usize> {
    let prefix = format!("T{}:", id);
    lines.iter().position(|l| l.trim_start().contains(&prefix))
}

fn parse_id(args: &str) -> Result<u32, String> {
    let v: serde_json::Value = serde_json::from_str(args).map_err(|e| format!("parse: {e}"))?;
    let val = v.get("id").ok_or("missing 'id'")?;
    let n = val.as_u64()
        .or_else(|| val.as_str().and_then(|s| s.parse::<u64>().ok()))
        .ok_or("'id' must be a positive integer")?;
    if n == 0 || n > u32::MAX as u64 {
        return Err("'id' must be a positive integer".into());
    }
    Ok(n as u32)
}

/// Parse stored task lines into TaskInfo structs for dashboard/status.
pub fn get_task_infos() -> Vec<deepx_proto::TaskInfo> {
    let lines = read_tasks();
    lines.iter().filter_map(|l| {
        let trimmed = l.trim_start();
        // Format: "- [status] T{id}: subject — description"
        if !trimmed.starts_with("- [") { return None; }
        let after_bracket = trimmed.split_once("] ")?.1;
        let status = &trimmed[3..trimmed.find(']')?];
        let (id_part, rest) = after_bracket.split_once(": ")?;
        let id = id_part.trim().to_string();
        let (subject, description) = rest.split_once(" — ").map_or(
            (rest.to_string(), String::new()),
            |(s, d)| (s.to_string(), d.to_string()),
        );
        Some(deepx_proto::TaskInfo { id, subject, description, status: status.to_string() })
    }).collect()
}

pub(super) fn exec_task_create(args: &str) -> String {
    let _guard = TASK_LOCK.lock().unwrap();
    let subject = parse_arg(args, "subject");
    let description = parse_arg(args, "description");

    if subject.is_empty() || subject.chars().count() > 100 {
        return "[ERROR] task_create: subject must be 1-100 chars\n[HINT] Keep the subject short and imperative, e.g. 'Add login API'".to_string();
    }
    if description.chars().count() > 200 {
        return "[ERROR] task_create: description max 200 chars\n[HINT] Write a concise 1-sentence description.".to_string();
    }

    let seed = current_seed();
    if seed.is_empty() {
        return "[ERROR] task_create: no active session. Start a conversation first.".to_string();
    }

    let mut lines = read_tasks();
    let id = next_id(&lines);
    let entry = format!("- [pending] T{}: {} — {}", id, subject, description);
    lines.push(entry);
    write_tasks(&lines);

    format!("[OK] Task T{} created [pending]: {}\nUse task_update(id={}, status=in_progress) when you start working on it.", id, subject, id)
}

pub(super) fn exec_task_update(args: &str) -> String {
    let _guard = TASK_LOCK.lock().unwrap();
    let id: u32 = match parse_id(args) {
        Ok(n) => n,
        Err(e) => return format!("[ERROR] task_update: {}", e),
    };
    let status = parse_arg(args, "status");

    if !matches!(status.as_str(), "pending" | "in_progress" | "completed" | "cancelled") {
        return "[ERROR] task_update: status must be pending, in_progress, completed, or cancelled".to_string();
    }

    let mut lines = read_tasks();
    let idx = match find_task(&lines, id) {
        Some(i) => i,
        None => return format!("[ERROR] task_update: task T{} not found. Use task_list to see task IDs.", id),
    };

    let old_markers = ["[pending]", "[in_progress]", "[completed]", "[cancelled]"];
    let new_marker = format!("[{}]", status);
    for marker in &old_markers {
        if lines[idx].contains(marker) {
            lines[idx] = lines[idx].replace(marker, &new_marker);
            break;
        }
    }
    write_tasks(&lines);
    format!("[OK] Task T{} → {}", id, status)
}

pub(super) fn exec_task_delete(args: &str) -> String {
    let _guard = TASK_LOCK.lock().unwrap();
    let id: u32 = match parse_id(args) {
        Ok(n) => n,
        Err(e) => return format!("[ERROR] task_delete: {}", e),
    };

    let mut lines = read_tasks();
    let idx = match find_task(&lines, id) {
        Some(i) => i,
        None => return format!("[ERROR] task_delete: task T{} not found.", id),
    };

    let removed = lines.remove(idx);
    let subject = removed
        .split(" — ").next()
        .and_then(|s| s.split(": ").nth(1))
        .unwrap_or("?");
    write_tasks(&lines);
    format!("[OK] Task T{} deleted: {}", id, subject)
}

pub(super) fn exec_task_list(args: &str) -> String {
    let filter_status = parse_opt(args, "status").unwrap_or_default();

    let lines = read_tasks();
    if lines.is_empty() {
        return "[OK] No tasks yet. Use task_create(subject=..., description=...) to create one.".to_string();
    }

    let marker = if filter_status.is_empty() { None } else { Some(format!("[{}]", filter_status)) };
    let filtered: Vec<&String> = if let Some(ref m) = marker {
        lines.iter().filter(|l| l.contains(m.as_str())).collect()
    } else {
        lines.iter().collect()
    };

    if filtered.is_empty() {
        return if filter_status.is_empty() {
            "[OK] No tasks.".to_string()
        } else {
            format!("[OK] No tasks with status '{}'.", filter_status)
        };
    }

    let icon = |s: &str| match s {
        "pending" => "○",
        "in_progress" => "●",
        "completed" => "✓",
        "cancelled" => "✗",
        _ => "?",
    };

    let mut result = format!("[OK] Tasks ({}):\n", filtered.len());
    for line in &filtered {
        let trimmed = line.trim_start_matches("- ");
        let status = if trimmed.contains("[pending]") { "pending" }
            else if trimmed.contains("[in_progress]") { "in_progress" }
            else if trimmed.contains("[completed]") { "completed" }
            else if trimmed.contains("[cancelled]") { "cancelled" }
            else { "?" };
        result.push_str(&format!("{} {}\n", icon(status), trimmed));
    }
    result
}

// ── Registration ──

use crate::{ToolHandler, ToolKey, SafetyVerdict, ToolCallCtx, ToolResult};
use std::time::Duration;

fn handle_task_create(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_task_create(&serde_json::to_string(&ctx.args).unwrap_or_default()))
}
fn handle_task_update(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_task_update(&serde_json::to_string(&ctx.args).unwrap_or_default()))
}
fn handle_task_delete(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_task_delete(&serde_json::to_string(&ctx.args).unwrap_or_default()))
}
fn handle_task_list(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_task_list(&serde_json::to_string(&ctx.args).unwrap_or_default()))
}

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("task_create", ""),
        description: "Create a tracked task. Returns a task ID (T1, T2…) for reference.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "subject": {"type": "string", "description": "Task subject, imperative form, 1-100 chars"},
                "description": {"type": "string", "description": "What needs to be done, 1-200 chars"}
            }, "required": ["subject", "description"], "additionalProperties": false
        }),
        handler: handle_task_create,
        safety: |_| SafetyVerdict::Allow,
        default_timeout: Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("task_update", ""),
        description: "Update task status by ID: pending → in_progress → completed | cancelled.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "id": {"type": "integer", "description": "Task ID (T1, T2, … — use the number only)"},
                "status": {"type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"]}
            }, "required": ["id", "status"], "additionalProperties": false
        }),
        handler: handle_task_update,
        safety: |_| SafetyVerdict::Allow,
        default_timeout: Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("task_delete", ""),
        description: "Delete a task by ID.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "id": {"type": "integer", "description": "Task ID to delete (T1, T2, … — use the number only)"}
            }, "required": ["id"], "additionalProperties": false
        }),
        handler: handle_task_delete,
        safety: |_| SafetyVerdict::Allow,
        default_timeout: Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("task_list", ""),
        description: "List tasks, optionally filtered by status.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "status": {"type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"]}
            }, "required": [], "additionalProperties": false
        }),
        handler: handle_task_list,
        safety: |_| SafetyVerdict::Allow,
        default_timeout: Duration::from_secs(15),
    });
}
