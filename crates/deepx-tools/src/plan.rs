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
use serde::{Deserialize, Serialize};
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
        let ws = crate::CURRENT_WORKSPACE
            .read()
            .expect("CURRENT_WORKSPACE lock")
            .clone();
        if !ws.is_empty() && ws != "." {
            let old_path = Path::new(&ws).join("PLAN.md");
            if old_path.exists() {
                let _ = std::fs::create_dir_all(&dir);
                if std::fs::copy(&old_path, &new_path).is_ok() {
                    log::info!(
                        "plan: migrated PLAN.md from {} to {}",
                        old_path.display(),
                        new_path.display()
                    );
                }
            }
        }
    }

    new_path
}

pub fn read_plan() -> Result<String, String> {
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

/// Session-scoped state for the first, deliberately small, autonomous-plan
/// prototype.  PLAN.md remains the human-readable plan of record; this file
/// only records execution progress, so plan review markers keep their current
/// meaning in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoalRun {
    objective: String,
    items: Vec<GoalStep>,
    next_index: usize,
    /// Set by `plan_step_complete`; consumed by the host after the current
    /// model turn has cleanly ended.
    awaiting_next_turn: bool,
    status: GoalRunStatus,
    #[serde(default)]
    paused_reason: Option<String>,
    #[serde(default)]
    auto_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoalStep {
    id: String,
    title: String,
    description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GoalRunStatus {
    Active,
    Completed,
    Paused,
    Stopped,
}

const MAX_GOAL_STEPS: usize = 12;
const MAX_GOAL_AUTO_TURNS: u32 = 24;

fn goal_path() -> Result<std::path::PathBuf, String> {
    let session = crate::runtime::context()
        .map(|ctx| ctx.active_session)
        .unwrap_or_default();
    if session.is_empty() {
        return Err("An active session is required to run an autonomous plan.".into());
    }
    Ok(deepx_types::platform::sessions_dir()
        .join(session)
        .join("goal_run.json"))
}

fn goal_authorization_path() -> Result<std::path::PathBuf, String> {
    Ok(goal_path()?.with_file_name("goal_activation.json"))
}

/// Apply an explicit desktop control without relying on model cooperation.
pub fn set_goal_action(seed: &str, action: &str) -> Result<(), String> {
    if seed.is_empty() { return Err("No active session".into()); }
    let path = deepx_types::platform::sessions_dir().join(seed).join("goal_run.json");
    let content = std::fs::read_to_string(&path).map_err(|error| format!("read goal run: {error}"))?;
    let mut run: GoalRun = serde_json::from_str(&content).map_err(|error| format!("parse goal run: {error}"))?;
    match action {
        "pause" if run.status == GoalRunStatus::Active => {
            run.status = GoalRunStatus::Paused;
            run.awaiting_next_turn = false;
            run.paused_reason = Some("用户暂停".into());
        }
        "stop" if matches!(run.status, GoalRunStatus::Active | GoalRunStatus::Paused) => {
            run.status = GoalRunStatus::Stopped;
            run.awaiting_next_turn = false;
            run.paused_reason = Some("用户停止".into());
        }
        "resume" if run.status == GoalRunStatus::Paused => return Ok(()),
        _ => return Err(format!("Goal cannot {action} from {:?}", run.status)),
    }
    let output = serde_json::to_vec_pretty(&run).map_err(|error| format!("serialize goal run: {error}"))?;
    std::fs::write(path, output).map_err(|error| format!("write goal run: {error}"))
}

/// Record a user-originated approval from the plan-review UI. This is kept
/// separate from the goal run so an LLM cannot manufacture activation merely
/// by calling `plan_activate`.
pub fn grant_goal_activation() -> Result<(), String> {
    let path = goal_authorization_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create goal directory: {error}"))?;
    }
    std::fs::write(path, br#"{"authorized":true}"#)
        .map_err(|error| format!("write goal authorization: {error}"))
}

fn consume_goal_activation() -> Result<bool, String> {
    let path = goal_authorization_path()?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("consume goal authorization: {error}")),
    }
}

fn read_goal_run() -> Result<Option<GoalRun>, String> {
    let path = goal_path()?;
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .map(Some)
            .map_err(|error| format!("parse goal run: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read goal run: {error}")),
    }
}

fn write_goal_run(run: &GoalRun) -> Result<(), String> {
    let path = goal_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create goal directory: {error}"))?;
    }
    let content =
        serde_json::to_vec_pretty(run).map_err(|error| format!("serialize goal run: {error}"))?;
    std::fs::write(path, content).map_err(|error| format!("write goal run: {error}"))
}

fn goal_prompt(run: &GoalRun, step: &GoalStep) -> String {
    format!(
        "[自动执行计划 / 目标：{}]\n\n继续执行 {}：{}\n{}\n\n完成此步骤后，必须调用 plan_step_complete(id=\"{}\", summary=\"…\")。如果遇到无法自行安全解决的阻塞，调用 plan_goal_stop(reason=\"…\") 或 ask_user；不要跳过步骤，也不要在本 turn 提前开始下一步骤。",
        run.objective, step.id, step.title, step.description, step.id
    )
}

/// Consume one queued autonomous-plan continuation.  The loop calls this
/// only after a normal `TurnComplete`, which ensures a cancelled/failed turn
/// can never advance the plan.
pub fn take_pending_goal_prompt() -> Option<String> {
    let _lock = PLAN_LOCK.lock().ok()?;
    let mut run = read_goal_run().ok()??;
    if run.status != GoalRunStatus::Active || !run.awaiting_next_turn {
        return None;
    }
    if run.auto_turns >= MAX_GOAL_AUTO_TURNS {
        run.status = GoalRunStatus::Paused;
        run.paused_reason = Some(format!("已达到连续执行上限（{MAX_GOAL_AUTO_TURNS} 回合）"));
        run.awaiting_next_turn = false;
        let _ = write_goal_run(&run);
        return None;
    }
    let step = run.items.get(run.next_index)?.clone();
    run.awaiting_next_turn = false;
    run.auto_turns += 1;
    if write_goal_run(&run).is_err() {
        return None;
    }
    Some(goal_prompt(&run, &step))
}

/// A real user message is an interruption point. Preserve the current item,
/// but prevent the host from injecting a later item until the user resumes.
pub fn pause_goal_for_interruption() {
    let Ok(_lock) = PLAN_LOCK.lock() else { return; };
    let Ok(Some(mut run)) = read_goal_run() else { return; };
    if run.status == GoalRunStatus::Active {
        run.status = GoalRunStatus::Paused;
        run.awaiting_next_turn = false;
        run.paused_reason = Some("用户临时插话".into());
        let _ = write_goal_run(&run);
    }
}

/// Resume the current item as a synthetic user directive.
pub fn resume_goal_prompt() -> Result<Option<String>, String> {
    let _lock = PLAN_LOCK.lock().map_err(|_| "goal state lock poisoned".to_string())?;
    let Some(mut run) = read_goal_run()? else { return Ok(None); };
    if run.status != GoalRunStatus::Paused {
        return Ok(None);
    }
    let Some(step) = run.items.get(run.next_index).cloned() else { return Ok(None); };
    run.status = GoalRunStatus::Active;
    run.paused_reason = None;
    write_goal_run(&run)?;
    Ok(Some(goal_prompt(&run, &step)))
}

/// Read-only session status for the desktop Goal strip.
pub fn goal_status_json(seed: &str) -> Result<String, String> {
    if seed.is_empty() { return Ok("null".into()); }
    let path = deepx_types::platform::sessions_dir().join(seed).join("goal_run.json");
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("null".into()),
        Err(error) => return Err(format!("read goal run: {error}")),
    };
    let run: GoalRun = serde_json::from_str(&content).map_err(|error| format!("parse goal run: {error}"))?;
    let current = run.items.get(run.next_index);
    serde_json::to_string(&serde_json::json!({
        "objective": run.objective,
        "status": run.status,
        "current_id": current.map(|step| &step.id),
        "current_title": current.map(|step| &step.title),
        "completed": run.next_index,
        "total": run.items.len(),
        "paused_reason": run.paused_reason,
        "auto_turns": run.auto_turns,
        "max_auto_turns": MAX_GOAL_AUTO_TURNS,
    })).map_err(|error| format!("serialize goal status: {error}"))
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
                    let (title, description) = if let Some((t, d)) = title_desc.split_once(" — ")
                    {
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
                            if !clean_desc.is_empty() {
                                clean_desc.push_str("。");
                            }
                            clean_desc.push_str(p);
                        }
                    }
                    items.push(PlanItem {
                        id,
                        title,
                        description: clean_desc,
                        status: status.to_string(),
                        deps,
                        effort,
                        comment,
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
            if let Ok(n) = num.parse::<u32>() {
                if n > max {
                    max = n;
                }
            }
        }
    }
    max + 1
}

// ── tool handlers ──

fn handle_plan_list(_ctx: ToolCallCtx) -> ToolResult {
    let content = match read_plan() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                content: crate::json_err("READ_FAILED", &e, "Check PLAN.md permissions."),
            };
        }
    };
    let items = parse_plan(&content);
    let json = serde_json::to_string(&items).unwrap_or_default();
    ToolResult {
        success: true,
        content: crate::json_ok(serde_json::json!({"items": items, "content": json})),
    }
}

fn handle_plan_create(ctx: ToolCallCtx) -> ToolResult {
    let title = ctx.get_str("title").unwrap_or("");
    let description = ctx.get_str("description").unwrap_or("");
    if title.is_empty() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "MISSING_TITLE",
                "title is required",
                "Provide a short title for this plan item.",
            ),
        };
    }

    let _lock = PLAN_LOCK.lock();
    let path = plan_path();
    let _ = crate::workspace::ensure_deepx_dir();

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let items = parse_plan(&content);
    let id = next_id(&items);

    let deps = ctx.get_str("deps").unwrap_or("none");
    let effort = ctx.get_str("effort").unwrap_or("");
    let mut meta_parts = vec![format!("Deps: {deps}")];
    if !effort.is_empty() {
        meta_parts.push(format!("Effort: {effort}"));
    }

    let line = format!(
        "\n- [ ] P{id}: {title} — {description}。{}。\n",
        meta_parts.join("。")
    );

    let mut file = match std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            return ToolResult {
                success: false,
                content: crate::json_err(
                    "WRITE_FAILED",
                    &format!("open PLAN.md: {e}"),
                    "Check permissions.",
                ),
            };
        }
    };
    if let Err(e) = file.write_all(line.as_bytes()) {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "WRITE_FAILED",
                &format!("write PLAN.md: {e}"),
                "Check disk space.",
            ),
        };
    }
    file.flush().ok();

    ToolResult {
        success: true,
        content: crate::json_ok(
            serde_json::json!({"plan_id": format!("P{id}"), "title": title, "content": format!("Plan item P{id} created: {title}")}),
        ),
    }
}

#[allow(dead_code)] // kept for frontend cmd_update_plan via admin server
fn handle_plan_update(ctx: ToolCallCtx) -> ToolResult {
    let id = ctx.get_str("id").unwrap_or("");
    let status = ctx.get_str("status").unwrap_or("");
    let comment = ctx.get_str("comment").unwrap_or("");
    if id.is_empty() || status.is_empty() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "MISSING_PARAM",
                "id and status are required",
                "Provide the plan ID and new status.",
            ),
        };
    }

    let _lock = PLAN_LOCK.lock();
    let path = plan_path();
    let _ = crate::workspace::ensure_deepx_dir();

    let content = std::fs::read_to_string(&path).unwrap_or_default();

    let mut found = false;
    let lines: Vec<String> = content
        .lines()
        .map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with("- [") && trimmed.contains(&format!(" {id}: ")) && !found {
                found = true;
                let new_marker = match status {
                    "approved" => "- [✓]",
                    "rejected" => "- [-]",
                    _ => "- [ ]",
                };
                let base = l.replacen("- [", new_marker, 1);
                if comment.is_empty() {
                    base
                } else {
                    format!("{base} | {comment}")
                }
            } else {
                l.to_string()
            }
        })
        .collect();

    if !found {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "NOT_FOUND",
                &format!("Plan item '{id}' not found"),
                "Check the plan ID with plan_list.",
            ),
        };
    }

    if let Err(e) = std::fs::write(&path, lines.join("\n") + "\n") {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "WRITE_FAILED",
                &format!("write PLAN.md: {e}"),
                "Check permissions.",
            ),
        };
    }

    ToolResult {
        success: true,
        content: crate::json_ok(
            serde_json::json!({"plan_id": id, "status": status, "content": format!("Plan item {id} updated to {status}")}),
        ),
    }
}

fn handle_plan_submit(_ctx: ToolCallCtx) -> ToolResult {
    let content = match read_plan() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                content: crate::json_err("READ_FAILED", &e, "Check PLAN.md permissions."),
            };
        }
    };
    if content.trim().is_empty() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "EMPTY",
                "PLAN.md is empty",
                "Use plan_create to add items first.",
            ),
        };
    }
    ToolResult {
        success: true,
        content: crate::json_ok(
            serde_json::json!({"content": format!("Plan submitted for review.\n\n{}", content)}),
        ),
    }
}

fn handle_plan_activate(ctx: ToolCallCtx) -> ToolResult {
    let objective = ctx.get_str("objective").unwrap_or("").trim();
    if objective.is_empty() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "MISSING_OBJECTIVE",
                "objective is required",
                "State the concrete outcome this autonomous plan should achieve.",
            ),
        };
    }

    let _lock = match PLAN_LOCK.lock() {
        Ok(lock) => lock,
        Err(error) => error.into_inner(),
    };
    if !goal_authorization_path().is_ok_and(|path| path.exists()) {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "GOAL_NOT_AUTHORIZED",
                "autonomous plan activation requires explicit user approval",
                "Ask the user to select '以目标模式执行' in the plan review panel, then resume this plan.",
            ),
        };
    }
    let items = match read_plan() {
        Ok(content) => parse_plan(&content)
            .into_iter()
            .filter(|item| item.status != "rejected")
            .map(|item| GoalStep {
                id: item.id,
                title: item.title,
                description: item.description,
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            return ToolResult {
                success: false,
                content: crate::json_err("READ_FAILED", &error, "Check PLAN.md permissions."),
            };
        }
    };
    if items.is_empty() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "EMPTY_PLAN",
                "PLAN.md has no runnable items",
                "Create and submit at least one non-rejected plan item first.",
            ),
        };
    }
    if items.len() > MAX_GOAL_STEPS {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "GOAL_STEP_LIMIT",
                &format!("autonomous plans support at most {MAX_GOAL_STEPS} steps"),
                "Split the plan into a smaller independently reviewable goal.",
            ),
        };
    }
    if matches!(
        read_goal_run(),
        Ok(Some(GoalRun {
            status: GoalRunStatus::Active,
            ..
        }))
    ) {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "GOAL_ALREADY_ACTIVE",
                "an autonomous plan is already active",
                "Finish it with plan_step_complete or stop it with plan_goal_stop first.",
            ),
        };
    }

    let run = GoalRun {
        objective: objective.to_string(),
        items,
        next_index: 0,
        awaiting_next_turn: false,
        status: GoalRunStatus::Active,
        paused_reason: None,
        auto_turns: 0,
    };
    let first = run.items.first().expect("non-empty goal items");
    if let Err(error) = write_goal_run(&run) {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "WRITE_FAILED",
                &error,
                "Check the active session directory.",
            ),
        };
    }
    if let Err(error) = consume_goal_activation() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "AUTHORIZATION_FAILED",
                &error,
                "Stop and ask the user to approve Goal mode again.",
            ),
        };
    }
    ToolResult {
        success: true,
        content: crate::json_ok(serde_json::json!({
            "objective": objective,
            "status": "active",
            "current_step": first.id,
            "content": format!("Autonomous plan activated. Execute {}: {} now. When it is truly complete, call plan_step_complete with id={}. Do not start the next item in this turn.", first.id, first.title, first.id),
        })),
    }
}

fn handle_plan_step_complete(ctx: ToolCallCtx) -> ToolResult {
    let id = ctx.get_str("id").unwrap_or("").trim();
    let summary = ctx.get_str("summary").unwrap_or("").trim();
    if id.is_empty() || summary.is_empty() {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "MISSING_PARAM",
                "id and summary are required",
                "Use the active plan ID and a concise, evidence-based completion summary.",
            ),
        };
    }
    let _lock = match PLAN_LOCK.lock() {
        Ok(lock) => lock,
        Err(error) => error.into_inner(),
    };
    let mut run = match read_goal_run() {
        Ok(Some(run)) if run.status == GoalRunStatus::Active => run,
        Ok(_) => {
            return ToolResult {
                success: false,
                content: crate::json_err(
                    "NO_ACTIVE_GOAL",
                    "no autonomous plan is active",
                    "Call plan_activate after submitting a plan.",
                ),
            };
        }
        Err(error) => {
            return ToolResult {
                success: false,
                content: crate::json_err("READ_FAILED", &error, "Check the session state."),
            };
        }
    };
    let current = match run.items.get(run.next_index) {
        Some(step) => step,
        None => {
            return ToolResult {
                success: false,
                content: crate::json_err(
                    "INVALID_GOAL_STATE",
                    "active goal has no current step",
                    "Stop and reactivate the plan.",
                ),
            };
        }
    };
    if current.id != id {
        return ToolResult {
            success: false,
            content: crate::json_err(
                "OUT_OF_ORDER_STEP",
                &format!("{id} cannot complete before {}", current.id),
                "Complete only the current plan step, or stop the autonomous plan.",
            ),
        };
    }
    run.next_index += 1;
    let next = run.items.get(run.next_index).cloned();
    if next.is_some() {
        run.awaiting_next_turn = true;
    } else {
        run.status = GoalRunStatus::Completed;
        run.awaiting_next_turn = false;
    }
    if let Err(error) = write_goal_run(&run) {
        return ToolResult {
            success: false,
            content: crate::json_err("WRITE_FAILED", &error, "Check the session state directory."),
        };
    }
    let content = match next {
        Some(step) => format!(
            "{} completed: {}. End this turn cleanly. The host will inject {} as a new user turn only after this turn completes.",
            id, summary, step.id
        ),
        None => format!("{} completed: {}. All autonomous plan items are complete; provide the final result now.", id, summary),
    };
    ToolResult {
        success: true,
        content: crate::json_ok(
            serde_json::json!({"completed_step": id, "status": run.status, "content": content}),
        ),
    }
}

fn handle_plan_goal_stop(ctx: ToolCallCtx) -> ToolResult {
    let reason = ctx.get_str("reason").unwrap_or("stopped by agent").trim();
    let _lock = match PLAN_LOCK.lock() {
        Ok(lock) => lock,
        Err(error) => error.into_inner(),
    };
    let mut run = match read_goal_run() {
        Ok(Some(run)) if matches!(run.status, GoalRunStatus::Active | GoalRunStatus::Paused) => run,
        Ok(_) => {
            return ToolResult {
                success: false,
                content: crate::json_err(
                    "NO_ACTIVE_GOAL",
                    "no autonomous plan is active",
                    "There is nothing to stop.",
                ),
            }
        }
        Err(error) => {
            return ToolResult {
                success: false,
                content: crate::json_err("READ_FAILED", &error, "Check the session state."),
            }
        }
    };
    run.status = GoalRunStatus::Stopped;
    run.awaiting_next_turn = false;
    match write_goal_run(&run) {
        Ok(()) => ToolResult {
            success: true,
            content: crate::json_ok(
                serde_json::json!({"status":"stopped", "content": format!("Autonomous plan stopped: {reason}")}),
            ),
        },
        Err(error) => ToolResult {
            success: false,
            content: crate::json_err("WRITE_FAILED", &error, "Check the session state directory."),
        },
    }
}

// ── registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    use crate::{ToolHandler, ToolRisk};
    use std::time::Duration;

    mgr.register(ToolHandler {
        key: "plan_list".to_string(),
        description:
            "List all plan items from PLAN.md with status, dependencies, and effort estimates.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {}, "additionalProperties": false
        }),
        handler: handle_plan_list,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(10),
    });

    mgr.register(ToolHandler {
        key: "plan_create".to_string(),
        description: "Add a new item to PLAN.md. Returns the assigned plan ID. Each item must be concrete and verifiable.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "title": {"type": "string", "description": "Short title for this plan item"},
                "description": {"type": "string", "description": "What this step involves, including specific acceptance criteria (e.g. 'Cargo check passes', 'test passes'). Must NOT be vague like 'improve UX'."},
                "deps": {"type": "string", "description": "Comma-separated IDs this depends on, or 'none'"},
                "effort": {"type": "string", "description": "Estimated effort, e.g. '2h' or '1d'"}
            }, "required": ["title", "description", "deps", "effort"], "additionalProperties": false
        }),
        handler: handle_plan_create,
        risk: ToolRisk::Write,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: "plan_submit".to_string(),
        description: "Submit the current PLAN.md for user review. Call this after all plan_create calls are done. Returns the full plan content.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {}, "additionalProperties": false
        }),
        handler: handle_plan_submit,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: "plan_activate".to_string(),
        description: "Activate the submitted PLAN.md as a session-scoped autonomous goal. It starts P1 now; each later item is injected by the host as a new user turn only after plan_step_complete confirms the current item.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "objective": {"type": "string", "description": "The concrete outcome the complete plan must achieve"}
            }, "required": ["objective"], "additionalProperties": false
        }),
        handler: handle_plan_activate,
        risk: ToolRisk::Write,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: "plan_step_complete".to_string(),
        description: "Mark only the current autonomous plan item complete, with evidence. The host queues the next plan item as a new user turn after this turn ends; never start it yourself in the current turn.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "id": {"type": "string", "description": "Current plan item ID, e.g. P1"},
                "summary": {"type": "string", "description": "Concise evidence-based completion summary"}
            }, "required": ["id", "summary"], "additionalProperties": false
        }),
        handler: handle_plan_step_complete,
        risk: ToolRisk::Write,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: "plan_goal_stop".to_string(),
        description: "Stop the active autonomous plan without modifying PLAN.md. Use when blocked or when user direction is required.",
        input_schema: serde_json::json!({
            "type": "object", "properties": {
                "reason": {"type": "string", "description": "Why execution was stopped"}
            }, "required": ["reason"], "additionalProperties": false
        }),
        handler: handle_plan_goal_stop,
        risk: ToolRisk::Write,
        default_timeout: Duration::from_secs(15),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomous_prompt_is_a_single_step_user_directive() {
        let run = GoalRun {
            objective: "ship the verified prototype".into(),
            items: vec![GoalStep {
                id: "P2".into(),
                title: "verify the result".into(),
                description: "Run the targeted test suite.".into(),
            }],
            next_index: 0,
            awaiting_next_turn: true,
            status: GoalRunStatus::Active,
            paused_reason: None,
            auto_turns: 0,
        };
        let prompt = goal_prompt(&run, &run.items[0]);

        assert!(prompt.contains("继续执行 P2：verify the result"));
        assert!(prompt.contains("plan_step_complete(id=\"P2\""));
        assert!(prompt.contains("不要跳过步骤"));
    }
}
