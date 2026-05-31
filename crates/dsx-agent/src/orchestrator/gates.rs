//! Gate functions: explore-before-read, re-read, health checks.
use crate::agent::AgentState;

use dsx_types::arg::{tool_action, parse_file_arg, parse_cmd_arg};
use super::tracker::last_assistant_content;

/// Explore gate: enforce explore-before-read, intent, stale-edit blocking.
pub fn explore_gate(state: &mut AgentState, tool_name: &str, tc_id: &str, args: &str) -> bool {
    let action = tool_action(args);
    let is_read = tool_name == "read_file";
    let is_write = tool_name == "write_file" || tool_name == "edit_file";
    let is_edit = tool_name == "edit_file";
    let is_exec = tool_name == "exec" && (action == "execute" || action == "run");
    let is_explore = tool_name == "explore" || (tool_name == "exec" && action == "explore");
    if is_read || is_write {
        if !state.has_explored {
            let _ = state.ctx.push_tool_result(tc_id, &format!("[ERROR] '{}' blocked: you haven't explored the project yet.\n[HINT] Call explore() first.", tool_name));
            return true;
        }
        if is_write {
            if let Some(ref path) = parse_file_arg(args) {
                let declared = last_assistant_mentions(state, path);
                if !declared {
                    state.turn_annotations.push(format!("[intent] write to '{}' was NOT declared in assistant reasoning \u{2014} consider requiring declaration", path));
                }
            }
        }
        if is_edit && state.turns_since_last_read >= 4 {
            let _ = state.ctx.push_tool_result(tc_id, &format!("[ERROR] 'file edit' blocked: {} turns since last read. Context may be stale.\n[HINT] Call read_file() first.", state.turns_since_last_read));
            return true;
        }
    }
    if is_exec {
        if !state.has_explored {
            let _ = state.ctx.push_tool_result(tc_id, &format!("[ERROR] 'exec execute' blocked: you haven't explored yet.\n[HINT] Call explore() first."));
            return true;
        }
        let cmd = parse_cmd_arg(args).unwrap_or_else(|| "?".into());
        if last_assistant_content(state).is_empty() {
            state.turn_annotations.push(format!("[exec] '{}' — next time, say what you're running so the log captures it.", cmd.chars().take(60).collect::<String>()));
        }
        if let Some((tool_match, cmd_summary)) = detect_tool_equivalent(&cmd) {
            state.turn_annotations.push(format!("[exec] '{}' looks like {}() — if {}() is insufficient, tell us why.", cmd_summary, tool_match, tool_match));
        }
        for written in &state.files_written_this_turn {
            let written_matches = cmd.contains(written)
                || std::path::absolute(written).ok()
                    .map(|a| cmd.contains(a.to_string_lossy().as_ref()))
                    .unwrap_or(false);
            if written_matches && classify_path(written) != PathTrust::Trusted {
                let _ = state.ctx.push_tool_result(tc_id, &format!("[ERROR] 'exec' blocked: '{}' was written this turn.\n[HINT] Explain what the script does and run it NEXT turn.", written));
                return true;
            }
        }
    }
    if is_explore {
        // explore() marks the project as explored
        state.has_explored = true;
    }
    false
}

/// Re-read gate: after file write/edit, block ALL other tools until the
/// written/edited file is re-read to prevent context hallucination.
pub fn re_read_gate(state: &mut AgentState, tool_name: &str, tc_id: &str, args: &str) -> bool {
    let required_path = match &state.re_read_required {
        Some(p) => p.clone(),
        None => return false,
    };
    let is_read = tool_name == "read_file";
    let is_same_file = parse_file_arg(args)
        .map_or(false, |p| p == required_path);
    if is_read && is_same_file {
        state.re_read_required = None;
        return false;
    }
    let _ = state.ctx.push_tool_result(tc_id, &format!(
        "[ERROR] '{}' blocked: must re-read '{}' after write/edit to prevent hallucination.\n[HINT] Call read_file(path=\"{}\") first.",
        tool_name, required_path, required_path
    ));
    true
}

fn last_assistant_mentions(state: &AgentState, path: &str) -> bool {
    if let Some(last) = state.ctx.to_vec().iter().rev().find(|m| m.role == "assistant" && !m.content.is_empty()) {
        last.content.iter().any(|b| matches!(b, dsx_types::ContentBlock::Text { text } if text.contains(path)))
    } else {
        false
    }
}

// ── Tool-equivalent detection ──

/// Detect if an exec command duplicates an existing tool. Returns (tool_name, cmd_summary).
pub fn detect_tool_equivalent(cmd: &str) -> Option<(String, String)> {
    let cmd = cmd.trim();
    // cat/head/tail/bat/less → read_file
    if let Some(rest) = cmd.strip_prefix("cat ").or_else(|| cmd.strip_prefix("head "))
        .or_else(|| cmd.strip_prefix("tail ")).or_else(|| cmd.strip_prefix("bat "))
        .or_else(|| cmd.strip_prefix("less "))
    {
        let target = rest.trim().split_whitespace().next().unwrap_or("?");
        if target.contains('.') {
            return Some(("read_file".into(), format!("cat {}", target)));
        }
    }
    // sed -i → edit_file
    if cmd.starts_with("sed ") && cmd.contains(" -i") {
        let target = cmd.split_whitespace().last().unwrap_or("?");
        if target.contains('.') {
            return Some(("edit_file".into(), format!("sed -i ... {}", target)));
        }
    }
    // grep/rg → search
    if let Some(rest) = cmd.strip_prefix("grep ").or_else(|| cmd.strip_prefix("rg ")) {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 && parts.last().unwrap_or(&"").contains('.') {
            return Some(("search".into(), format!("grep {} {}", parts[0], parts.last().unwrap())));
        }
    }
    // tee file / >> file / > file → write_file (append)
    if cmd.contains("tee ") || cmd.contains(">> ") || cmd.contains("> ") {
        let pos = cmd.find("> ").or_else(|| cmd.find(">> ")).or_else(|| cmd.find("tee "));
        if let Some(p) = pos {
            let rest = &cmd[p..];
            let target = rest.split_whitespace().nth(1).unwrap_or("?");
            if target.contains('.') {
                return Some(("write_file".into(), format!("redirect → {}", target)));
            }
        }
    }
    None
}

// ── Path trust levels ──

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PathTrust {
    /// /tmp/, /var/tmp/ — no gate, no sandbox, write+exec freely
    Trusted,
    /// src/**/*.rs, Cargo.toml — full intent gate + sandbox
    HighStake,
}

pub fn classify_path(path: &str) -> PathTrust {
    let normalized = std::path::Path::new(path)
        .components()
        .collect::<std::path::PathBuf>()
        .to_string_lossy()
        .to_string();

    // For absolute paths, check directly.
    if normalized.starts_with('/') {
        if normalized.starts_with("/tmp/") || normalized.starts_with("/var/tmp/")
            || normalized.starts_with("/exam_sctrip/") || normalized.starts_with("/exam/")
        {
            return PathTrust::Trusted;
        }
    } else if normalized.starts_with("..") {
        // Parent-relative — resolve to absolute for trust check.
        if let Ok(abs) = std::path::absolute(&normalized) {
            let abs = abs.to_string_lossy();
            if abs.starts_with("/tmp/") || abs.starts_with("/var/tmp/") {
                return PathTrust::Trusted;
            }
        }
    }
    // Relative paths under the project dir are always HighStake.
    PathTrust::HighStake
}
