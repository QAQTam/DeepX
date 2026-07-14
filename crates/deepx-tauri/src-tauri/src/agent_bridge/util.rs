//! Zero-dependency helper functions used across agent_bridge modules.

use deepx_proto::Ui2Agent;

/// Read a file preview: up to `max_lines` lines or `max_chars` characters.
pub fn read_file_preview(
    path: &str,
    max_lines: usize,
    max_chars: usize,
) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let reader = BufReader::new(file);
    let mut result = String::new();
    let mut line_count = 0;
    for line in reader.lines() {
        if line_count >= max_lines {
            break;
        }
        let line = line.map_err(|e| format!("read: {e}"))?;
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&line);
        line_count += 1;
        if result.chars().count() >= max_chars {
            // Truncate at char boundary
            let end = result.floor_char_boundary(max_chars);
            result.truncate(end);
            result.push_str("\n… (truncated)");
            break;
        }
    }
    Ok(result)
}

/// Convert UNIX epoch seconds to a local date string (YYYY-MM-DD).
pub fn chrono_local_date_from_epoch(epoch_secs: u64) -> String {
    let total_days = (epoch_secs / 86400) as i64;
    let (y, m, d) = deepx_types::platform::civil_from_days(total_days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Generate a daily array for the given range, all zeroed.
pub fn generate_date_range(days: u32) -> Vec<serde_json::Value> {
    let mut arr = Vec::new();
    for d in 0..days {
        let date = days_before_today(days - 1 - d);
        arr.push(serde_json::json!({
            "date": date,
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "cache_hit": 0,
            "cache_miss": 0,
            "calls": 0,
        }));
    }
    arr
}

/// Compute the date string `days` days before today (UTC-based).
pub fn days_before_today(days: u32) -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs().saturating_sub((days as u64) * 86400);
    let epoch_days = total_secs / 86400;
    let (y, m, d) = deepx_types::platform::civil_from_days(epoch_days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse PLAN.md checklist format into structured items.
/// Format: `- [ ] P1: Title — Description。Deps: ...。Effort: ... | comment`
pub struct PlanItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub comment: String,
    pub actions: Vec<String>,
}

pub fn parse_plan_items(content: &str) -> Vec<PlanItem> {
    let mut items = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- [") {
            continue;
        }

        // Extract status marker
        let status = if let Some(bracket_end) = trimmed.find("] ") {
            let inner = &trimmed[3..bracket_end];
            match inner {
                "✓" | "x" | "X" => "approved",
                "-" => "rejected",
                "?" => "ask",
                _ => "pending",
            }
        } else {
            continue;
        };

        // Extract body after "] "
        let body = match trimmed.split_once("] ") {
            Some((_, b)) => b,
            None => continue,
        };

        // Split: "P1: Title — Description。Deps: ...。Effort: ... | comment"
        let (id, rest) = match body.split_once(": ") {
            Some((i, r)) => (i.trim().to_string(), r.trim()),
            None => continue,
        };

        // Extract title (before ' — ')
        let (title, tail) = match rest.split_once(" — ") {
            Some((t, r)) => (t.trim().to_string(), r.to_string()),
            None => (rest.to_string(), String::new()),
        };

        // Extract comment (after last '|')
        let (_description, comment) = if let Some(pos) = tail.rfind(" | ") {
            (
                tail[..pos].trim().to_string(),
                tail[pos + 3..].trim().to_string(),
            )
        } else {
            (tail, String::new())
        };

        items.push(PlanItem {
            id,
            title,
            status: status.to_string(),
            comment: comment.clone(),
            actions: if comment.is_empty() {
                Vec::new()
            } else {
                vec![comment]
            },
        });
    }
    items
}

/// Map a Ui2Agent variant to a short name for logging.
pub fn agent2ui_event_name_for_ui(event: &Ui2Agent) -> &'static str {
    match event {
        Ui2Agent::UserInput { .. } => "user_input",
        Ui2Agent::ToolCall { .. } => "tool_call",
        Ui2Agent::CreateSession => "create_session",
        Ui2Agent::Cancel => "cancel",
        Ui2Agent::Shutdown => "shutdown",
        Ui2Agent::ReloadConfig => "reload_config",
        Ui2Agent::UndoTurn { .. } => "undo_turn",
        Ui2Agent::Compact => "compact",
        Ui2Agent::ResumeSession { .. } => "resume_session",
        Ui2Agent::NewSession => "new_session",
        Ui2Agent::LoadMoreTurns { .. } => "load_more_turns",
        _ => "unknown",
    }
}
