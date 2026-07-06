//! PLAN management: parse, create, and update PLAN.md checklist items.
//!
//! Format:
//! ```markdown
//! # PLAN: Objective
//!
//! - [ ] P1: Title — Description。Deps: none。Effort: 2h
//! - [x] P2: Title — Description。Deps: P1。Effort: 4h | comment
//! - [-] P3: Title — Description。Deps: P2。Effort: 1h | rejection reason
//! ```
//!
//! Status markers: `[ ]` pending, `[✓]` approved, `[-]` rejected.

use crate::{ToolCallCtx, ToolResult};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static PLAN_LOCK: Mutex<()> = Mutex::new(());

/// Path to PLAN.md inside the `.deepx/` directory.
/// Attempts migration from old `{workspace}/PLAN.md` if the new path doesn't exist.
fn plan_path() -> std::path::PathBuf {
    let dir = crate::workspace::deepx_dir();
    let new_path = dir.join("PLAN.md");

    // One-time migration: copy old PLAN.md → .deepx/PLAN.md
    if !new_path.exists() {
        let ws = crate::CURRENT_WORKSPACE.read().expect("CURRENT_WORKSPACE lock").clone();
        if !ws.is_empty() && ws != "." {
            let old_path = Path::new(&ws).join("PLAN.md");
            if old_path.exists() {
                let _ = std::fs::create_dir_all(&dir);
                if std::fs::copy(&old_path, &new_path).is_ok() {
                    log::info!("plan: migrated PLAN.md from {} to {}", old_path.display(), new_path.display());
                }
            }
        }
    }

    new_path
}

fn read_plan() -> Result<String, String> {
    let path = plan_path();
    match std::fs::read_to_string(&path) {
        Ok(c) => Ok(c),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("read PLAN.md: {e}")),
    }
}

#[derive(serde::Serialize, Clone)]
struct PlanItem {
    id: String,
    title: String,
    description: String,
    status: String,
    deps: String,
    effort: String,
    comment: String,
}

fn parse_plan(content: &str) -> Vec<PlanItem> {
    let mut items = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [") {
            if let Some(bracket_end) = rest.find(']') {
                let status = match &rest[..bracket_end] {
                    "x" | "X" | "✓" => "approved",
                    "-" => "rejected",
                    _ => "pending",
                };
                let body = rest[bracket_end + 1..].trim();
                if let Some((id_part, remainder)) = body.split_once(": ") {
                    let id = id_part.trim().to_string();
                    let (title_desc, comment) = if let Some((td, c)) = remainder.split_once(" | ") {
                        (td.trim().to_string(), c.trim().to_string())
                    } else {
                        (remainder.trim().to_string(), String::new())
                    };
                    let (title, description) = if let Some((t, d)) = title_desc.split_once(" — ") {
                        (t.trim().to_string(), d.trim().to_string())
                    } else {
                        (title_desc.clone(), String::new())
                    };
                    let mut deps = String::new();
                    let mut effort = String::new();
                    let mut clean_desc = String::new();
                    for part in description.split("。") {
                        let p = part.trim();
                        if p.starts_with("Deps:") {
                            deps = p.strip_prefix("Deps:").unwrap_or("").trim().to_string();
                        } else if p.starts_with("Effort:") {
                            effort = p.strip_prefix("Effort:").unwrap_or("").trim().to_string();
                        } else if !p.is_empty() {
                            if !clean_desc.is_empty() { clean_desc.push_str("。"); }
                            clean_desc.push_str(p);
                        }
                    }
                    items.push(PlanItem {
                        id, title, description: clean_desc, status: status.to_string(), deps, effort, comment,
                    });
                }
            }
        }
    }
    items
}

fn next_id(items: &[PlanItem]) -> u32 {
    let mut max = 0u32;
    for item in items {
        if let Some(num) = item.id.strip_prefix('P') {
            if let Ok(n) = num.parse::<u32>() { if n > max { max = n; } }
        }
    }
    max + 1
}

// ── tool handlers ──

fn handle_plan_list(_ctx: ToolCallCtx) -> ToolResult {
    let content = match read_plan() {
        Ok(c) => c,
        Err(e) => return ToolResult { success: false, content: format!("[ERROR] {e}") },
    };
    let items = parse_plan(&content);
    let json = serde_json::to_string_pretty(&items).unwrap_or_default();
    ToolResult { success: true, content: json }
}

fn handle_plan_create(ctx: ToolCallCtx) -> ToolResult {
    let title = ctx.get_str("title").unwrap_or("");
    let description = ctx.get_str("description").unwrap_or("");
    if title.is_empty() {
        return ToolResult { success: false, content: "[ERROR] 'title' is required".into() };
    }

    let _lock = PLAN_LOCK.lock();
    let path = plan_path();
    let _ = crate::workspace::ensure_deepx_dir(); // ensure .deepx/ and trash/ exist

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let items = parse_plan(&content);
    let id = next_id(&items);

    let deps = ctx.get_str("deps").unwrap_or("none");
    let effort = ctx.get_str("effort").unwrap_or("");
    let mut meta_parts = vec![format!("Deps: {deps}")];
    if !effort.is_empty() { meta_parts.push(format!("Effort: {effort}")); }

    let line = format!("\n- [ ] P{id}: {title} — {description}。{}。\n",
        meta_parts.join("。"));

    let mut file = match std::fs::OpenOptions::new().append(true).create(true)
        .open(&path) {
        Ok(f) => f,
        Err(e) => return ToolResult { success: false, content: format!("open PLAN.md: {e}") },
    };
    if let Err(e) = file.write_all(line.as_bytes()) {
        return ToolResult { success: false, content: format!("write PLAN.md: {e}") };
    }
    file.flush().ok();

    ToolResult { success: true, content: format!("Plan item P{id} created: {title}") }
}

fn handle_plan_update(ctx: ToolCallCtx) -> ToolResult {
    let id = ctx.get_str("id").unwrap_or("");
    let status = ctx.get_str("status").unwrap_or("");
    let comment = ctx.get_str("comment").unwrap_or("");
    if id.is_empty() || status.is_empty() {
        return ToolResult { success: false, content: "[ERROR] 'id' and 'status' are required".into() };
    }

    let _lock = PLAN_LOCK.lock();
    let path = plan_path();
    let _ = crate::workspace::ensure_deepx_dir();

    let content = std::fs::read_to_string(&path).unwrap_or_default();

    let mut found = false;
    let lines: Vec<String> = content.lines().map(|l| {
        let trimmed = l.trim();
        if trimmed.starts_with("- [") && trimmed.contains(&format!(" {id}: ")) && !found {
            found = true;
            let new_marker = match status {
                "approved" => "- [✓]",
                "rejected" => "- [-]",
                _ => "- [ ]",
            };
            let base = l.replacen("- [", new_marker, 1);
            if comment.is_empty() { base } else { format!("{base} | {comment}") }
        } else {
            l.to_string()
        }
    }).collect();

    if !found {
        return ToolResult { success: false, content: format!("[ERROR] Plan item '{id}' not found in PLAN.md") };
    }

    if let Err(e) = std::fs::write(&path, lines.join("\n") + "\n") {
        return ToolResult { success: false, content: format!("write PLAN.md: {e}") };
    }

    ToolResult { success: true, content: format!("Plan item {id} updated to {status}") }
}

// ── registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    use std::time::Duration;
    use crate::{ToolHandler, ToolKey, ToolRisk};

    mgr.register(ToolHandler {
        key: ToolKey::new("plan_list", ""),
        description: "List all plan items from PLAN.md with status, dependencies, and effort estimates.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {}, "additionalProperties": false
        }),
        handler: handle_plan_list,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(10),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("plan_create", ""),
        description: "Add a new item to PLAN.md. Returns the assigned plan ID.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "title": {"type": "string", "description": "Short title for this plan item"},
                "description": {"type": "string", "description": "What this step involves, including acceptance criteria"},
                "deps": {"type": "string", "description": "Comma-separated IDs this depends on, or 'none'"},
                "effort": {"type": "string", "description": "Estimated effort, e.g. '2h' or '1d'"}
            }, "required": ["title", "description"], "additionalProperties": false
        }),
        handler: handle_plan_create,
        risk: ToolRisk::Write,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("plan_update", ""),
        description: "Update the status of a PLAN.md item (approved, rejected, or pending).",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "id": {"type": "string", "description": "Plan item ID, e.g. 'P1'"},
                "status": {"type": "string", "enum": ["pending", "approved", "rejected"]},
                "comment": {"type": "string", "description": "Optional reason for the status change"}
            }, "required": ["id", "status"], "additionalProperties": false
        }),
        handler: handle_plan_update,
        risk: ToolRisk::Write,
        default_timeout: Duration::from_secs(15),
    });
}
