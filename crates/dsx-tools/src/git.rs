//! Git tools wrapped via `git` CLI.
//!
//! checkpoint / status / diff / log / commit
//! push / pull / fetch / branch / checkout / merge
//! stash / reset / restore / remote / init

use std::process::Command;
use crate::{ToolCallCtx, ToolResult};

fn run_git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git not found: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let msg = if stdout.is_empty() { stderr } else { format!("{}\n{}", stderr, stdout) };
        return Err(msg);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ── Checkpoint ──

static CHECKPOINT_N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn next_cp() -> u32 {
    CHECKPOINT_N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn handle_checkpoint(ctx: ToolCallCtx) -> ToolResult {
    let action = ctx.get_str("action").unwrap_or("save");
    match action {
        "save" => {
            let n = next_cp();
            match run_git(&["stash", "push", "-m", &format!("cp-{n}")]) {
                Ok(m) if m.contains("No local changes") => ToolResult::ok(format!("[OK] cp {n}: no changes")),
                Ok(_) => ToolResult::ok(format!("[OK] Checkpoint {n} saved. Rollback: git_checkpoint(action=rollback, id={n})")),
                Err(e) => ToolResult::ok(format!("[ERROR] cp save: {e}")),
            }
        }
        "rollback" => {
            let id = ctx.get_str("id").unwrap_or("0");
            match run_git(&["stash", "pop", "--index", &format!("stash@{{{id}}}")]) {
                Ok(_) => ToolResult::ok(format!("[OK] Rolled back to checkpoint {id}")),
                Err(e) => ToolResult::ok(format!("[ERROR] rollback {id}: {e}")),
            }
        }
        "clear" => {
            let id = ctx.get_str("id").unwrap_or("0");
            match run_git(&["stash", "drop", &format!("stash@{{{id}}}")]) {
                Ok(_) => ToolResult::ok(format!("[OK] Cleared checkpoint {id}")),
                Err(e) => ToolResult::ok(format!("[ERROR] clear {id}: {e}")),
            }
        }
        "list" => {
            match run_git(&["stash", "list"]) {
                Ok(l) => ToolResult::ok(format!("[OK] Checkpoints:\n{}", if l.is_empty() { "(none)" } else { &l })),
                Err(e) => ToolResult::ok(format!("[ERROR] list: {e}")),
            }
        }
        _ => ToolResult::ok("[ERROR] action: save | rollback | clear | list"),
    }
}

// ── Status / Diff / Log ──

fn handle_status(_ctx: ToolCallCtx) -> ToolResult {
    match run_git(&["status", "--short"]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] clean"),
        Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

fn handle_diff(ctx: ToolCallCtx) -> ToolResult {
    let staged = ctx.get_bool("staged").unwrap_or(false);
    let mut args = vec!["diff"];
    if staged { args.push("--staged"); }
    match run_git(&args) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] no changes"),
        Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

fn handle_log(ctx: ToolCallCtx) -> ToolResult {
    let n = ctx.get_u64("n").unwrap_or(10).min(50);
    let n = n.to_string();
    match run_git(&["log", "--oneline", "-n", &n]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] no commits"),
        Ok(s) => ToolResult::ok(format!("[OK] last {}:\n{}", n, s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Commit ──

fn handle_commit(ctx: ToolCallCtx) -> ToolResult {
    let msg = ctx.get_str("message").unwrap_or("");
    if msg.is_empty() { return ToolResult::ok("[ERROR] missing message"); }
    let files = ctx.get_str("files").unwrap_or(".");
    for f in files.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Err(e) = run_git(&["add", f]) {
            return ToolResult::ok(format!("[ERROR] add {}: {e}", f));
        }
    }
    match run_git(&["commit", "-m", msg]) {
        Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Push / Pull / Fetch ──

fn handle_push(ctx: ToolCallCtx) -> ToolResult {
    let remote = ctx.get_str("remote").unwrap_or("origin");
    let branch = ctx.get_str("branch").unwrap_or("");
    let mut args = vec!["push", remote];
    if !branch.is_empty() { args.push(branch); }
    match run_git(&args) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] pushed"),
        Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

fn handle_pull(ctx: ToolCallCtx) -> ToolResult {
    let rebase = ctx.get_bool("rebase").unwrap_or(false);
    let mut args = vec!["pull"];
    if rebase { args.push("--rebase"); }
    match run_git(&args) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] up to date"),
        Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

fn handle_fetch(_ctx: ToolCallCtx) -> ToolResult {
    match run_git(&["fetch", "--all", "--prune"]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] no updates"),
        Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Branch ──

fn handle_branch(ctx: ToolCallCtx) -> ToolResult {
    let action = ctx.get_str("action").unwrap_or("list");
    match action {
        "list" => {
            match run_git(&["branch", "-a"]) {
                Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        "create" => {
            let name = ctx.get_str("name").unwrap_or("");
            if name.is_empty() { return ToolResult::ok("[ERROR] missing name"); }
            match run_git(&["checkout", "-b", name]) {
                Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        "switch" => {
            let name = ctx.get_str("name").unwrap_or("");
            if name.is_empty() { return ToolResult::ok("[ERROR] missing name"); }
            match run_git(&["checkout", name]) {
                Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        "delete" => {
            let name = ctx.get_str("name").unwrap_or("");
            if name.is_empty() { return ToolResult::ok("[ERROR] missing name"); }
            match run_git(&["branch", "-d", name]) {
                Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        _ => ToolResult::ok("[ERROR] action: list | create | switch | delete"),
    }
}

// ── Checkout ──

fn handle_checkout(ctx: ToolCallCtx) -> ToolResult {
    let target = ctx.get_str("target").unwrap_or("");
    if target.is_empty() { return ToolResult::ok("[ERROR] missing target (branch or file)"); }
    match run_git(&["checkout", target]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK]"),
        Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Reset ──

fn handle_reset(ctx: ToolCallCtx) -> ToolResult {
    let mode = ctx.get_str("mode").unwrap_or("mixed");
    let target = ctx.get_str("target").unwrap_or("HEAD");
    let mut args = vec!["reset"];
    match mode {
        "soft" => args.push("--soft"),
        "mixed" => {}
        "hard" => args.push("--hard"),
        _ => return ToolResult::ok("[ERROR] mode: soft | mixed | hard"),
    }
    args.push(target);
    match run_git(&args) {
        Ok(s) if s.is_empty() => ToolResult::ok(format!("[OK] reset {mode} �?{target}")),
        Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Stash ──

fn handle_stash(ctx: ToolCallCtx) -> ToolResult {
    let action = ctx.get_str("action").unwrap_or("save");
    match action {
        "save" => {
            let msg = ctx.get_str("message").unwrap_or("");
            let mut args = vec!["stash", "push"];
            if !msg.is_empty() { args.push("-m"); args.push(msg); }
            match run_git(&args) {
                Ok(s) if s.is_empty() => ToolResult::ok("[OK] stashed"),
                Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        "pop" => {
            match run_git(&["stash", "pop"]) {
                Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        "list" => {
            match run_git(&["stash", "list"]) {
                Ok(s) if s.is_empty() => ToolResult::ok("[OK] no stashes"),
                Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
                Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
            }
        }
        _ => ToolResult::ok("[ERROR] action: save | pop | list"),
    }
}

// ── Restore / Remote / Init ──

fn handle_restore(ctx: ToolCallCtx) -> ToolResult {
    let file = ctx.get_str("file").unwrap_or("");
    if file.is_empty() { return ToolResult::ok("[ERROR] missing file"); }
    let staged = ctx.get_bool("staged").unwrap_or(false);
    let mut args = vec!["restore"];
    if staged { args.push("--staged"); }
    args.push(file);
    match run_git(&args) {
        Ok(_) => ToolResult::ok(format!("[OK] restored {}", file)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

fn handle_remote(_ctx: ToolCallCtx) -> ToolResult {
    match run_git(&["remote", "-v"]) {
        Ok(s) if s.is_empty() => ToolResult::ok("[OK] no remotes"),
        Ok(s) => ToolResult::ok(format!("[OK]\n{}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

fn handle_init(ctx: ToolCallCtx) -> ToolResult {
    let branch = ctx.get_str("branch").unwrap_or("main");
    match run_git(&["init", "-b", branch]) {
        Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Merge ──

fn handle_merge(ctx: ToolCallCtx) -> ToolResult {
    let branch = ctx.get_str("branch").unwrap_or("");
    if branch.is_empty() { return ToolResult::ok("[ERROR] missing branch"); }
    match run_git(&["merge", branch]) {
        Ok(s) => ToolResult::ok(format!("[OK] {}", s)),
        Err(e) => ToolResult::ok(format!("[ERROR] {e}")),
    }
}

// ── Registration ──

macro_rules! reg {
    ($mgr:expr, $name:literal, $desc:literal, $schema:expr, $handler:expr, $safety:expr) => {
        $mgr.register(crate::ToolHandler {
            key: crate::ToolKey::new($name, ""),
            description: $desc,
            input_schema: $schema,
            handler: $handler,
            safety: $safety,
            default_timeout: std::time::Duration::from_secs(15),
        });
    };
}

pub fn register(manager: &mut crate::ToolManager) {
    let allow = |_: &ToolCallCtx| crate::SafetyVerdict::Allow;

    reg!(manager, "git_checkpoint", "Save/restore git stash checkpoint. action=save|rollback|clear|list, id=N",
        serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["save","rollback","clear","list"]},"id":{"type":"string"}},"required":["action"]}),
        handle_checkpoint, allow);
    reg!(manager, "git_status", "Show git status (git status --short)",
        serde_json::json!({"type":"object","properties":{}}), handle_status, allow);
    reg!(manager, "git_diff", "Show changes (git diff). staged=true for staged",
        serde_json::json!({"type":"object","properties":{"staged":{"type":"boolean"}}}), handle_diff, allow);
    reg!(manager, "git_log", "Show commit history (git log --oneline -n N, default 10)",
        serde_json::json!({"type":"object","properties":{"n":{"type":"integer"}}}), handle_log, allow);
    reg!(manager, "git_commit", "Stage files and commit. message required.",
        serde_json::json!({"type":"object","properties":{"message":{"type":"string"},"files":{"type":"string","description":"comma-separated, default '.'"}},"required":["message"]}),
        handle_commit, allow);
    reg!(manager, "git_push", "Push to remote (default origin).",
        serde_json::json!({"type":"object","properties":{"remote":{"type":"string"},"branch":{"type":"string"}}}), handle_push, allow);
    reg!(manager, "git_pull", "Pull from remote. rebase=true for rebase.",
        serde_json::json!({"type":"object","properties":{"rebase":{"type":"boolean"}}}), handle_pull, allow);
    reg!(manager, "git_fetch", "Fetch all remotes with prune.",
        serde_json::json!({"type":"object","properties":{}}), handle_fetch, allow);
    reg!(manager, "git_branch", "Manage branches. action=list|create|switch|delete, name required for create/switch/delete.",
        serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["list","create","switch","delete"]},"name":{"type":"string"}},"required":["action"]}),
        handle_branch, allow);
    reg!(manager, "git_checkout", "Checkout branch or restore file. target=file or branch name.",
        serde_json::json!({"type":"object","properties":{"target":{"type":"string"}},"required":["target"]}),
        handle_checkout, allow);
    reg!(manager, "git_reset", "Reset HEAD. mode=soft|mixed|hard, target default HEAD.",
        serde_json::json!({"type":"object","properties":{"mode":{"type":"string","enum":["soft","mixed","hard"]},"target":{"type":"string"}},"required":["mode"]}),
        handle_reset, allow);
    reg!(manager, "git_stash", "Stash changes. action=save|pop|list.",
        serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["save","pop","list"]},"message":{"type":"string"}},"required":["action"]}),
        handle_stash, allow);
    reg!(manager, "git_restore", "Restore file (git restore). file required.",
        serde_json::json!({"type":"object","properties":{"file":{"type":"string"},"staged":{"type":"boolean"}},"required":["file"]}),
        handle_restore, allow);
    reg!(manager, "git_remote", "List git remotes.",
        serde_json::json!({"type":"object","properties":{}}), handle_remote, allow);
    reg!(manager, "git_init", "Initialize a new git repo (git init).",
        serde_json::json!({"type":"object","properties":{"branch":{"type":"string","description":"default branch name, default: main"}}}),
        handle_init, allow);
    reg!(manager, "git_merge", "Merge a branch into current (git merge branch).",
        serde_json::json!({"type":"object","properties":{"branch":{"type":"string"}},"required":["branch"]}),
        handle_merge, allow);
}
