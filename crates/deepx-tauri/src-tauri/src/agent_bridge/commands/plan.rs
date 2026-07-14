//! Plan/task management, context stats, token stats, migration commands.

use tauri::{AppHandle, Emitter};
use deepx_proto::Ui2Agent;
use super::super::registry::send_to_agent;
use super::super::util::parse_plan_items;
use super::super::config::resolve_deepx_dir;

#[tauri::command]
pub fn cmd_migration_count() -> Result<String, String> {
    let count = deepx_session::SessionManager::global().count_pending_migration();
    serde_json::to_string(&serde_json::json!({ "pending": count }))
        .map_err(|e| format!("serialize: {e}"))
}

/// Migrate all pending sessions from JSONL to Turso.

#[tauri::command]
pub fn cmd_migrate_to_turso() -> Result<String, String> {
    let (sessions, messages) = deepx_session::SessionManager::global()
        .migrate_all_to_turso()
        .map_err(|e| format!("migration failed: {e}"))?;
    serde_json::to_string(&serde_json::json!({
        "sessions": sessions,
        "messages": messages,
    }))
    .map_err(|e| format!("serialize: {e}"))
}

/// Get activity log for a session: tool invocations with args + result.

#[tauri::command]
pub fn cmd_task_action(seed: String, action: String, task_id: u32) -> Result<(), String> {
    let path = resolve_deepx_dir(&seed).join("tasks.md");
    let _guard = std::sync::Mutex::new(()); // serialize access

    let mut lines: Vec<String> = if path.exists() {
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    } else {
        Vec::new()
    };

    let prefix = format!("T{}:", task_id);
    let idx = lines.iter().position(|l| l.contains(&prefix));

    match action.as_str() {
        "cancel" => {
            let idx = idx.ok_or_else(|| format!("Task T{} not found", task_id))?;
            for marker in &["[pending]", "[in_progress]", "[completed]", "[cancelled]"] {
                if lines[idx].contains(marker) {
                    lines[idx] = lines[idx].replace(marker, "[cancelled]");
                    break;
                }
            }
        }
        "delete" => {
            if let Some(idx) = idx {
                lines.remove(idx);
            }
        }
        _ => return Err(format!("Unknown action: {action}")),
    }

    std::fs::write(&path, lines.join("\n")).map_err(|e| format!("write tasks: {e}"))?;

    // Notify agent if running
    let args = serde_json::json!({"id": task_id, "status": if action == "cancel" { "cancelled" } else { "deleted" }});
    let frame = if action == "cancel" {
        Ui2Agent::ToolCall {
            id: format!("frontend_tc_{}", task_id),
            name: "task".into(),
            action: "update".into(),
            args,
        }
    } else {
        Ui2Agent::ToolCall {
            id: format!("frontend_tc_{}", task_id),
            name: "task".into(),
            action: "delete".into(),
            args,
        }
    };
    let _ = send_to_agent(&seed, frame);
    Ok(())
}

/// Get context composition stats from the agent's compact stats file.
/// Returns JSON breakdown from context_stats.json (written by the agent after compaction).

#[tauri::command]
pub fn cmd_get_context_stats(seed: String) -> Result<String, String> {
    let stats_path = deepx_types::platform::sessions_dir()
        .join(&seed)
        .join("context_stats.json");
    if stats_path.exists() {
        return Ok(std::fs::read_to_string(&stats_path).unwrap_or_default());
    }
    // No stats yet — return zeroed template
    Ok(serde_json::json!({
        "messages":0,"chat_text":0,"thinking":0,"tool_calls":0,"tool_results":0,
        "tools_schema":0,"system_prompt":0,"thinking_blocks":0,"tool_call_blocks":0
    })
    .to_string())
}

/// Get aggregated token usage stats for the last N days.
/// Returns JSON: { daily: [{date, prompt_tokens, completion_tokens, cache_hit, cache_miss, calls}], totals: {...} }

#[tauri::command]
pub fn cmd_get_token_stats(days: u32) -> Result<String, String> {
    use std::collections::BTreeMap;
    use std::io::BufRead;

    let path = deepx_types::platform::data_dir().join("token_stats.jsonl");
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => {
            // No data yet — return empty result
            let result = serde_json::json!({
                "daily": generate_date_range(days),
                "totals": { "prompt_tokens": 0, "completion_tokens": 0, "calls": 0, "cache_hit_pct": 0.0 },
            });
            return serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"));
        }
    };
    let reader = std::io::BufReader::new(file);

    // Compute cutoff date string "YYYY-MM-DD"
    let cutoff = days_before_today(days);

    // Aggregate: date -> { prompt_tokens, completion_tokens, cache_hit, cache_miss, calls }
    let mut daily: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("read: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| format!("parse: {e}"))?;
        let date = entry["date"].as_str().unwrap_or("").to_string();
        if date < cutoff {
            continue;
        } // before range, skip

        let prompt = entry["prompt_tokens"].as_u64().unwrap_or(0);
        let completion = entry["completion_tokens"].as_u64().unwrap_or(0);
        let hit = entry["cache_hit"].as_u64().unwrap_or(0);
        let miss = entry["cache_miss"].as_u64().unwrap_or(0);

        let day = daily.entry(date).or_insert_with(|| {
            serde_json::json!({
                "prompt_tokens": 0u64,
                "completion_tokens": 0u64,
                "cache_hit": 0u64,
                "cache_miss": 0u64,
                "calls": 0u64,
            })
        });
        day["prompt_tokens"] =
            serde_json::json!(day["prompt_tokens"].as_u64().unwrap_or(0) + prompt);
        day["completion_tokens"] =
            serde_json::json!(day["completion_tokens"].as_u64().unwrap_or(0) + completion);
        day["cache_hit"] = serde_json::json!(day["cache_hit"].as_u64().unwrap_or(0) + hit);
        day["cache_miss"] = serde_json::json!(day["cache_miss"].as_u64().unwrap_or(0) + miss);
        day["calls"] = serde_json::json!(day["calls"].as_u64().unwrap_or(0) + 1);
    }

    // Build daily array sorted by date, filling gaps with zeros
    let mut daily_arr = Vec::new();
    let mut totals = serde_json::json!({
        "prompt_tokens": 0u64,
        "completion_tokens": 0u64,
        "cache_hit": 0u64,
        "cache_miss": 0u64,
        "calls": 0u64,
    });

    // Generate all dates in range
    for d in 0..days {
        let date = days_before_today(days - 1 - d); // start from cutoff, go forward
        let entry = daily.get(&date).cloned().unwrap_or_else(|| {
            serde_json::json!({
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "cache_hit": 0,
                "cache_miss": 0,
                "calls": 0,
            })
        });
        for key in &[
            "prompt_tokens",
            "completion_tokens",
            "cache_hit",
            "cache_miss",
            "calls",
        ] {
            let v = entry[key].as_u64().unwrap_or(0);
            totals[key] = serde_json::json!(totals[key].as_u64().unwrap_or(0) + v);
        }
        daily_arr.push(serde_json::json!({
            "date": date,
            "prompt_tokens": entry["prompt_tokens"],
            "completion_tokens": entry["completion_tokens"],
            "cache_hit": entry["cache_hit"],
            "cache_miss": entry["cache_miss"],
            "calls": entry["calls"],
        }));
    }

    // Compute cache hit percentage
    let total_hit = totals["cache_hit"].as_u64().unwrap_or(0);
    let total_miss = totals["cache_miss"].as_u64().unwrap_or(0);
    let total_cache = total_hit + total_miss;
    let hit_pct = if total_cache > 0 {
        (total_hit as f64 / total_cache as f64 * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };
    totals["cache_hit_pct"] = serde_json::json!(hit_pct);
    // Remove raw hit/miss from totals (keep only percentage)
    totals.as_object_mut().map(|o| {
        o.remove("cache_hit");
        o.remove("cache_miss");
    });

    let result = serde_json::json!({
        "daily": daily_arr,
        "totals": totals,
    });
    serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"))
}

/// Generate the daily array for the given range, all zeroed.
fn generate_date_range(days: u32) -> Vec<serde_json::Value> {
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
fn days_before_today(days: u32) -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs().saturating_sub((days as u64) * 86400);
    let epoch_days = total_secs / 86400;
    let (y, m, d) = deepx_types::platform::civil_from_days(epoch_days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Returns empty array if PLAN.md doesn't exist or workspace is not set.
#[tauri::command]
pub fn cmd_read_plan(seed: String) -> Result<String, String> {
    if seed.is_empty() {
        return Ok("[]".into());
    }
    let plan_path = resolve_deepx_dir(&seed).join("PLAN.md");
    let content = match std::fs::read_to_string(&plan_path) {
        Ok(c) => c,
        Err(_) => return Ok("[]".into()),
    };
    let items = parse_plan_items(&content);
    // Manual JSON serialization (avoid serde derive dependency)
    let json_items: Vec<serde_json::Value> = items
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "id": item.id,
                "title": item.title,
                "status": item.status,
                "comment": item.comment,
                "actions": item.actions,
            })
        })
        .collect();
    serde_json::to_string(&json_items).map_err(|e| format!("serialize: {e}"))
}

/// Write a plan action (approve/reject/ask) back to PLAN.md by updating
/// the checklist status marker. Format: `- [✓] P1: ...` or `- [-] P1: ... | reason`
#[tauri::command]
pub fn cmd_plan_action(
    app: AppHandle,
    seed: String,
    item_id: String,
    action: String,
    user_comment: String,
) -> Result<(), String> {
    if seed.is_empty() {
        return Err("No active session".into());
    }
    let plan_path = resolve_deepx_dir(&seed).join("PLAN.md");
    let content = std::fs::read_to_string(&plan_path).map_err(|e| format!("read PLAN.md: {e}"))?;

    // Find checklist line matching "- [ ] P1:" or "- [✓] P1:" etc.
    let mut found = false;
    let new_content: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if !found && trimmed.starts_with("- [") && trimmed.contains(&format!(" {}: ", item_id))
            {
                found = true;
                match action.as_str() {
                    "approve" => line.replacen("- [ ]", "- [✓]", 1),
                    "reject" => {
                        let base = line.replacen("- [ ]", "- [-]", 1);
                        if user_comment.is_empty() {
                            base
                        } else {
                            format!("{base} | {user_comment}")
                        }
                    }
                    "ask" => line.replacen("- [ ]", "- [?]", 1),
                    "delete" => return String::new(), // remove the line entirely
                    _ => format!("{line} | {user_comment}"),
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if !found {
        return Err(format!("Plan item '{}' not found in PLAN.md", item_id));
    }

    std::fs::write(&plan_path, new_content).map_err(|e| format!("write PLAN.md: {e}"))?;

    // Notify frontend that PLAN.md changed
    let _ = app.emit("plan-changed", serde_json::json!({"seed": seed}));

    Ok(())
}
