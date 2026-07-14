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

#[cfg(test)]
mod tests {
    use super::*;
    use deepx_proto::Ui2Agent;

    // ── read_file_preview ──

    #[test]
    fn test_read_file_preview_basic() {
        let dir = std::env::temp_dir();
        let path = dir.join("deepx_test_read_preview.txt");
        std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();
        let result = read_file_preview(&path.to_string_lossy(), 3, 1000).unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(!result.contains("line4")); // max_lines=3
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_file_preview_char_truncation() {
        let dir = std::env::temp_dir();
        let path = dir.join("deepx_test_trunc.txt");
        std::fs::write(&path, "abcdefghijklmnopqrstuvwxyz").unwrap();
        let result = read_file_preview(&path.to_string_lossy(), 10, 10).unwrap();
        assert!(result.len() <= 10 + "… (truncated)".len() + 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_file_preview_nonexistent() {
        let result = read_file_preview("/nonexistent/deepx_test_file.txt", 5, 100);
        assert!(result.is_err());
    }

    // ── parse_plan_items ──

    #[test]
    fn test_parse_plan_items_complete() {
        let content = "- [ ] P1: 审计持久化 — 确保所有操作落盘。Deps: none。Effort: 2h\n- [✓] P2: 重构模块";

        let items = parse_plan_items(content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "P1");
        assert_eq!(items[0].title, "审计持久化");
        assert_eq!(items[0].status, "pending");

        assert_eq!(items[1].id, "P2");
        assert_eq!(items[1].title, "重构模块");
        assert_eq!(items[1].status, "approved");
    }

    #[test]
    fn test_parse_plan_items_with_comment() {
        let content = "- [-] P3: 已废弃 — Deps: P1。Effort: 1h | 不再需要";
        let items = parse_plan_items(content);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "rejected");
        assert_eq!(items[0].comment, "不再需要");
    }

    #[test]
    fn test_parse_plan_items_ask_status() {
        let content = "- [?] P4: 需求确认 — Deps: none。Effort: 0.5h";
        let items = parse_plan_items(content);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "ask");
    }

    #[test]
    fn test_parse_plan_items_empty() {
        let items = parse_plan_items("");
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_plan_items_ignores_non_checklist() {
        let content = "# 标题\n这是一段描述\n- [ ] P1: 有效项 — Deps: none。Effort: 1h";
        let items = parse_plan_items(content);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "P1");
    }

    // ── agent2ui_event_name_for_ui ──

    #[test]
    fn test_agent2ui_event_names() {
        assert_eq!(
            agent2ui_event_name_for_ui(&Ui2Agent::UserInput { text: "hi".into() }),
            "user_input"
        );
        assert_eq!(agent2ui_event_name_for_ui(&Ui2Agent::Cancel), "cancel");
        assert_eq!(agent2ui_event_name_for_ui(&Ui2Agent::CreateSession), "create_session");
        assert_eq!(agent2ui_event_name_for_ui(&Ui2Agent::Shutdown), "shutdown");
        assert_eq!(agent2ui_event_name_for_ui(&Ui2Agent::Compact), "compact");
        assert_eq!(
            agent2ui_event_name_for_ui(&Ui2Agent::ResumeSession { seed: "abc".into() }),
            "resume_session"
        );
    }

    #[test]
    fn test_agent2ui_event_name_unknown() {
        // AskResponse is not listed → should be "unknown"
        assert_eq!(
            agent2ui_event_name_for_ui(&Ui2Agent::AskResponse {
                ask_id: "q1".into(),
                answers: vec![]
            }),
            "unknown"
        );
    }

    // ── days_before_today / chrono ──

    #[test]
    fn test_days_before_today_format() {
        let date = days_before_today(0);
        // Should be YYYY-MM-DD format
        assert_eq!(date.len(), 10);
        assert_eq!(date.chars().nth(4), Some('-'));
        assert_eq!(date.chars().nth(7), Some('-'));
    }

    #[test]
    fn test_chrono_local_date_simple() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let date = chrono_local_date_from_epoch(1704067200);
        assert_eq!(date, "2024-01-01");
    }

    // ── generate_date_range ──

    #[test]
    fn test_generate_date_range_count() {
        let arr = generate_date_range(3);
        assert_eq!(arr.len(), 3);
        for entry in &arr {
            assert!(entry.get("date").is_some());
            assert!(entry.get("prompt_tokens").is_some());
        }
    }
}
