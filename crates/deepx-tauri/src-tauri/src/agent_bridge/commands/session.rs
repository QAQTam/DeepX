//! Session lifecycle commands: send message, create/resume/close session,
//! cancel, set mode, dashboard, activity, undo, compact, load more turns.

use super::super::registry::{AgentRegistry, ensure_agent, send_to_agent};
use super::super::util::read_file_preview;
use deepx_proto::Ui2Agent;

/// prepending their content to the user text.
#[tauri::command]
pub fn cmd_send_message(
    seed: String,
    text: String,
    files: Option<Vec<String>>,
) -> Result<(), String> {
    let files = files.unwrap_or_default();
    log::info!(
        "[REGISTRY] cmd_send_message seed={}: {}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        &text[..text.floor_char_boundary(50)]
    );
    ensure_agent(&seed)?;

    let full_text = if files.is_empty() {
        text
    } else {
        let mut parts = Vec::new();
        parts.push("[Files]".to_string());
        for path in &files {
            match read_file_preview(path, 10, 1000) {
                Ok(preview) => {
                    parts.push(format!("\n{path}:\n{preview}"));
                }
                Err(e) => {
                    parts.push(format!("\n{path}: [ERROR: {e}]"));
                }
            }
        }
        parts.push(format!("\n\n[Message]\n{text}"));
        parts.join("")
    };

    send_to_agent(&seed, Ui2Agent::UserInput { text: full_text })
}

/// Set the agent's operating mode (Normal, Plan, Code).

#[tauri::command]
pub fn cmd_set_mode(seed: String, mode: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_set_mode seed={} mode={mode}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    send_to_agent(&seed, Ui2Agent::SetMode { mode })
}

/// Send user's response to a permission request dialog.

#[tauri::command]
pub fn cmd_resume_session(seed: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_resume_session seed={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    deepx_session::SessionManager::global().set_active_seed(&seed);
    ensure_agent(&seed)?;
    Ok(())
}

#[tauri::command]
pub fn cmd_replay_session_events(seed: String) -> Result<Vec<serde_json::Value>, String> {
    let registry = AgentRegistry::get()
        .lock()
        .map_err(|error| format!("registry lock: {error}"))?;
    registry.replay_events(&seed)
}

/// Create a new session with a pre-generated seed.

#[tauri::command]
pub fn cmd_new_session() -> Result<String, String> {
    let seed = deepx_session::SessionManager::generate_seed();
    log::info!(
        "[REGISTRY] cmd_new_session seed={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    deepx_session::SessionManager::global().clear_active();
    {
        let mut registry = AgentRegistry::get()
            .lock()
            .map_err(|e| format!("lock: {e}"))?;
        registry.spawn_new(&seed)?;
    }
    Ok(seed)
}

/// Cancel the current operation for the given session.

#[tauri::command]
pub fn cmd_cancel(seed: String) -> Result<(), String> {
    send_to_agent(&seed, Ui2Agent::Cancel)
}

/// Save configuration and reload all agents.

#[tauri::command]
pub fn cmd_get_activity(seed: String) -> Result<String, String> {
    let mgr = deepx_session::SessionManager::global();
    let (_meta, messages) = mgr
        .load(&seed)
        .ok_or_else(|| "session not found".to_string())?;

    // Build a map: tool_use_id → (name, args)
    let mut tool_map: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    for msg in &messages {
        if msg.role == "assistant" {
            for b in &msg.content {
                if let deepx_types::ContentBlock::ToolUse { id, name, input } = b {
                    tool_map.insert(id.clone(), (name.clone(), input.to_string()));
                }
            }
        }
    }

    let mut activities = Vec::new();
    for msg in &messages {
        if msg.role != "tool" {
            continue;
        }
        for b in &msg.content {
            if let deepx_types::ContentBlock::ToolResult {
                tool_use_id,
                content,
                success,
            } = b
            {
                let (tool_name, args) = tool_map
                    .get(tool_use_id)
                    .map(|(n, a)| (n.clone(), a.clone()))
                    .unwrap_or_default();
                let summary = content
                    .lines()
                    .skip_while(|l| l.starts_with("[timeis:"))
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect::<String>();
                activities.push(serde_json::json!({
                    "tool_name": tool_name,
                    "summary": summary,
                    "success": success,
                    "time": msg.msg_id.map(|id| id.to_string()).unwrap_or_default(),
                    "args": args,
                }));
            }
        }
    }
    activities.reverse(); // newest first
    serde_json::to_string(&activities).map_err(|e| format!("serialize: {e}"))
}

/// Delete a session by seed. Also kills the agent if running for that seed.

#[tauri::command]
pub fn cmd_undo_turn(seed: String, turn_id: String) -> Result<(), String> {
    send_to_agent(&seed, Ui2Agent::UndoTurn { turn_id })
}

/// Compact conversation history (summarize old turns).

#[tauri::command]
pub fn cmd_compact(seed: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_compact seed={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    send_to_agent(&seed, Ui2Agent::Compact)
}

/// Load older turns from session history (paginated, 20 at a time before the given turn).

#[tauri::command]
pub fn cmd_load_more_turns(seed: String, before_turn_id: String) -> Result<(), String> {
    send_to_agent(
        &seed,
        Ui2Agent::LoadMoreTurns {
            before_turn_id,
            count: 20,
        },
    )
}

/// Get the current session's workspace root path.

#[tauri::command]
pub fn cmd_close_session(seed: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_close_session seed={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    // Extract instance under lock, then wait outside lock.
    let instance = {
        if let Ok(mut registry) = AgentRegistry::get().lock() {
            registry.kill_agent(&seed)
        } else {
            None
        }
    };
    if let Some(inst) = instance {
        inst.shutdown_and_wait();
    }
    Ok(())
}

/// Get git status for the current workspace: lists modified/new/deleted files with diff stats.
/// Runs independently of the agent process — reads git repo directly.

#[tauri::command]
pub fn cmd_get_dashboard_data(seed: String) -> Result<String, String> {
    use std::io::BufRead;

    // Tasks are session-scoped, matching deepx-tools/task.rs.
    let tasks: Vec<serde_json::Value> = {
        let path = deepx_types::platform::sessions_dir()
            .join(&seed)
            .join("tasks.md");
        if let Ok(file) = std::fs::File::open(&path) {
            std::io::BufReader::new(file)
                .lines()
                .filter_map(|l| l.ok())
                .filter(|l| l.starts_with("- ["))
                .filter_map(|line| {
                    let trimmed = line.trim_start();
                    let status = &trimmed[3..trimmed.find(']')?];
                    let after = trimmed.split_once("] ")?.1;
                    let (id_part, rest) = after.split_once(": ")?;
                    let (subject, description) = rest.split_once(" — ").unwrap_or((rest, ""));
                    Some(serde_json::json!({
                        "id": id_part.trim(),
                        "subject": subject.trim(),
                        "description": description.trim(),
                        "status": status,
                    }))
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    // Recent edits from code_stats.jsonl (last 10 unique files)
    let recent_edits: Vec<String> = {
        let path = deepx_types::platform::sessions_dir()
            .join(&seed)
            .join("code_stats.jsonl");
        if let Ok(file) = std::fs::File::open(&path) {
            let mut files: Vec<String> = std::io::BufReader::new(file)
                .lines()
                .filter_map(|l| l.ok())
                .filter_map(|line| {
                    serde_json::from_str::<serde_json::Value>(&line)
                        .ok()
                        .and_then(|v| v.get("file").and_then(|f| f.as_str()).map(String::from))
                })
                .collect();
            files.reverse();
            files.dedup();
            files.truncate(10);
            files
        } else {
            Vec::new()
        }
    };

    serde_json::to_string(&serde_json::json!({
        "tasks": tasks,
        "recent_edits": recent_edits,
    }))
    .map_err(|e| format!("serialize: {e}"))
}
