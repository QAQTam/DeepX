//! Git tools: checkpoint, status, diff, log, commit.
//! All operations wrap `git` CLI via `std::process::Command`.

use std::process::Command;
use crate::{ToolCallCtx, ToolResult};

fn run_git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git not found: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!("git failed: {}", stderr.trim()));
    }
    Ok(stdout.trim().to_string())
}

// ── Checkpoint ──

static CHECKPOINT_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn next_checkpoint() -> u32 {
    CHECKPOINT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn handle_checkpoint(ctx: ToolCallCtx) -> ToolResult {
    let action = ctx.get_str("action").unwrap_or("save");
    match action {
        "save" => {
            let n = next_checkpoint();
            match run_git(&["stash", "push", "-m", &format!("checkpoint-{n}")]) {
                Ok(msg) if msg.contains("No local changes") =>
                    ToolResult::ok(format!("[OK] Checkpoint {n}: no changes to save")),
                Ok(_) =>
                    ToolResult::ok(format!("[OK] Checkpoint {n} saved. Use checkpoint(action=rollback, id={n}) to restore.")),
                Err(e) => ToolResult::ok(format!("[ERROR] Checkpoint save: {e}")),
            }
        }
        "rollback" => {
            let id = ctx.get_str("id").unwrap_or("0");
            let stash_ref = format!("stash@{{{id}}}");
            match run_git(&["stash", "pop", "--index", &stash_ref]) {
                Ok(_) => ToolResult::ok(format!("[OK] Rolled back to checkpoint {id}")),
                Err(e) => ToolResult::ok(format!("[ERROR] Rollback {id}: {e}")),
            }
        }
        "clear" => {
            let id = ctx.get_str("id").unwrap_or("0");
            let stash_ref = format!("stash@{{{id}}}");
            match run_git(&["stash", "drop", &stash_ref]) {
                Ok(_) => ToolResult::ok(format!("[OK] Cleared checkpoint {id}")),
                Err(e) => ToolResult::ok(format!("[ERROR] Clear checkpoint {id}: {e}")),
            }
        }
        "list" => {
            match run_git(&["stash", "list"]) {
                Ok(list) => ToolResult::ok(format!("[OK] Checkpoints:\n{}", list)),
                Err(e) => ToolResult::ok(format!("[ERROR] List: {e}")),
            }
        }
        _ => ToolResult::ok("[ERROR] checkpoint: action must be save, rollback, clear, or list".to_string()),
    }
}

// ── Status ──

fn handle_status(_ctx: ToolCallCtx) -> ToolResult {
    match run_git(&["status", "--short"]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] Working tree clean"),
        Ok(s) => ToolResult::ok(format!("[OK] git status:\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] git status: {e}")),
    }
}

// ── Diff ──

fn handle_diff(ctx: ToolCallCtx) -> ToolResult {
    let staged = ctx.get_bool("staged").unwrap_or(false);
    let mut args = vec!["diff"];
    if staged { args.push("--staged"); }
    args.push("--");
    match run_git(&args) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] No changes"),
        Ok(s) => ToolResult::ok(format!("[OK] git diff:\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] git diff: {e}")),
    }
}

// ── Log ──

fn handle_log(ctx: ToolCallCtx) -> ToolResult {
    let n = ctx.get_u64("n").unwrap_or(10).min(50);
    let n_str = n.to_string();
    match run_git(&["log", "--oneline", "-n", &n_str]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] No commits"),
        Ok(s) => ToolResult::ok(format!("[OK] git log (last {}):\n{}", n, s)),
        Err(e) => ToolResult::ok(format!("[ERROR] git log: {e}")),
    }
}

// ── Commit ──

fn handle_commit(ctx: ToolCallCtx) -> ToolResult {
    let message = ctx.get_str("message").unwrap_or("");
    if message.is_empty() {
        return ToolResult::ok("[ERROR] commit: missing 'message'".to_string());
    }
    let files = ctx.get_str("files").unwrap_or(".");
    for f in files.split(',') {
        let f = f.trim();
        if !f.is_empty() {
            if let Err(e) = run_git(&["add", f]) {
                return ToolResult::ok(format!("[ERROR] git add {f}: {e}"));
            }
        }
    }
    match run_git(&["commit", "-m", message]) {
        Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] git commit: {e}")),
    }
}

// ── Registration ──

pub fn register(manager: &mut crate::ToolManager) {
    manager.register(crate::ToolHandler {
        key: crate::ToolKey::new("git", "checkpoint"),
        description: "Save/restore/clear git stash checkpoints for self-iteration. Actions: save, rollback(id=N), clear(id=N), list.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["save", "rollback", "clear", "list"]},
                "id": {"type": "string", "description": "Checkpoint ID for rollback/clear"}
            },
            "required": ["action"]
        }),
        handler: handle_checkpoint,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(10),
    });
    manager.register(crate::ToolHandler {
        key: crate::ToolKey::new("git", "status"),
        description: "Show working tree status (git status --short).",
        input_schema: serde_json::json!({"type": "object", "properties": {}}),
        handler: handle_status,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(10),
    });
    manager.register(crate::ToolHandler {
        key: crate::ToolKey::new("git", "diff"),
        description: "Show changes (git diff). Use staged=true for staged changes.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"staged": {"type": "boolean"}}
        }),
        handler: handle_diff,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(10),
    });
    manager.register(crate::ToolHandler {
        key: crate::ToolKey::new("git", "log"),
        description: "Show commit history (git log --oneline -n N).",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"n": {"type": "integer", "description": "Number of commits (default 10, max 50)"}}
        }),
        handler: handle_log,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(10),
    });
    manager.register(crate::ToolHandler {
        key: crate::ToolKey::new("git", "commit"),
        description: "Stage files and create a commit. Use checkpoint(action=save) first.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "message": {"type": "string", "description": "Commit message"},
                "files": {"type": "string", "description": "Files to add (comma-separated, default '.')"}
            },
            "required": ["message"]
        }),
        handler: handle_commit,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}
