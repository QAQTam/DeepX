//! Session file persistence: save and load conversation sessions.

use dsx_types::{Message, SessionFile, SessionMeta};

// ── Session loading ──

pub fn load_session(seed: &str) -> Option<SessionFile> {
    // Try live first, then completed
    if let Some((file, _)) = load_session_or_live(seed) {
        return Some(file);
    }
    None
}

pub fn load_session_or_live(seed: &str) -> Option<(SessionFile, bool)> {
    let path = super::find_existing_session_path(seed)?;
    let data = std::fs::read_to_string(&path).ok()?;
    if let Ok(file) = toml::from_str::<SessionFile>(&data) {
        return Some((file, false));
    }
    // Fallback: try legacy JSON
    if let Ok(file) = serde_json::from_str::<SessionFile>(&data) {
        return Some((file, false));
    }
    migrate_legacy_session(&data, seed).map(|f| (f, false))
}

/// Migrate a legacy-format session file (old Message struct) to the new
/// ContentBlock format. Writes the migrated file back to disk.
fn migrate_legacy_session(data: &str, seed: &str) -> Option<SessionFile> {
    // Old format: content: String, tool_calls, tool_call_id, reasoning_content, thinking_signature, name
    // Try to parse as dynamic JSON and manually convert
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let old_msgs = v.get("messages")?.as_array()?;

    let new_msgs: Vec<dsx_types::Message> = old_msgs.iter().map(|m| {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();
        let mut blocks = Vec::new();

        match role.as_str() {
            "system" => {
                let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                blocks.push(dsx_types::ContentBlock::text(text));
            }
            "user" => {
                let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                if !text.is_empty() {
                    blocks.push(dsx_types::ContentBlock::text(text));
                }
            }
            "assistant" => {
                // reasoning_content → Reasoning block
                if let Some(rc) = m.get("reasoning_content").and_then(|v| v.as_str()) {
                    blocks.push(dsx_types::ContentBlock::Reasoning {
                        reasoning: rc.to_string(),
                    });
                }
                // content → Text block
                if let Some(text) = m.get("content").and_then(|c| c.as_str()) {
                    if !text.is_empty() {
                        blocks.push(dsx_types::ContentBlock::text(text));
                    }
                }
                // tool_calls → ToolUse blocks
                if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name = tc.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .or_else(|| tc.get("name").and_then(|n| n.as_str()))
                            .unwrap_or("")
                            .to_string();
                        let args = tc.get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .map(|s| serde_json::from_str(s).unwrap_or(serde_json::Value::Null))
                            .or_else(|| tc.get("arguments").cloned())
                            .unwrap_or(serde_json::Value::Null);
                        blocks.push(dsx_types::ContentBlock::ToolUse { id, name, input: args });
                    }
                }
            }
            "tool" => {
                let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                let tool_use_id = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                blocks.push(dsx_types::ContentBlock::ToolResult {
                    tool_use_id,
                    content: text.to_string(),
                });
            }
            _ => {}
        }

        dsx_types::Message { role, name: None, content: blocks }
    }).collect();

    Some(SessionFile {
        seed: seed.to_string(),
        created_at: v.get("created_at").and_then(|c| c.as_u64()).unwrap_or(0),
        updated_at: v.get("updated_at").and_then(|c| c.as_u64()).unwrap_or(0),
        model: v.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string(),
        effort: v.get("effort").and_then(|e| e.as_str()).map(String::from),
        messages: new_msgs,
        last_summary: v.get("last_summary").and_then(|l| l.as_str()).unwrap_or("").to_string(),
    })
}

// ── Session saving ──

pub fn save_session(
    seed: &str,
    messages: &[Message],
    model: &str,
    effort: Option<&str>,
) {
    let Some(sfile_path) = super::session_path(seed) else { return };
    let Some(parent) = sfile_path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);

    let now = super::now_epoch();
    // Preserve created_at from existing meta if available
    let created_at = load_index().iter()
        .find(|m| m.seed == seed)
        .map(|m| m.created_at)
        .unwrap_or(now);

    let last_summary = super::extract_last_summary(messages);

    let file = SessionFile {
        seed: seed.to_string(),
        created_at,
        updated_at: now,
        model: model.to_string(),
        effort: effort.map(|s| s.to_string()),
        messages: messages.to_vec(),
        last_summary,
    };

    let serialized = toml::to_string_pretty(&file).unwrap_or_default();
    let tmp_path = sfile_path.with_extension("toml.tmp");
    let _ = std::fs::write(&tmp_path, &serialized);
    let _ = std::fs::rename(&tmp_path, &sfile_path);
    update_index_entry(&file);
}

pub fn finalize_session(
    seed: &str,
    messages: &[Message],
    model: &str,
    effort: Option<&str>,
) {
    save_session(seed, messages, model, effort);
}

// ── Session index ──

fn save_index(metas: &[SessionMeta]) {
    let Some(path) = super::index_path() else { return };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    let _ = std::fs::write(&path, toml::to_string_pretty(metas).unwrap_or_default());
}

pub fn load_index() -> Vec<SessionMeta> {
    let Some(path) = super::index_path() else { return vec![] };
    if !path.exists() { return vec![]; }
    let Ok(data) = std::fs::read_to_string(&path) else { return vec![] };
    toml::from_str::<Vec<SessionMeta>>(&data).unwrap_or_else(|_| {
        // Fallback: try legacy JSON index
        serde_json::from_str::<Vec<SessionMeta>>(&data).unwrap_or_default()
    })
}

pub(super) fn update_index_entry(file: &SessionFile) {
    let mut metas = load_index();
    let meta = SessionMeta {
        seed: file.seed.clone(),
        created_at: file.created_at,
        updated_at: file.updated_at,
        model: file.model.clone(),
        effort: file.effort.clone(),
        message_count: file.messages.len(),
        last_summary: file.last_summary.clone(),
    };
    if let Some(existing) = metas.iter_mut().find(|m| m.seed == meta.seed) {
        *existing = meta;
    } else {
        metas.push(meta);
    }
    save_index(&metas);
}

// ── Live snapshot ──

pub fn save_live_snapshot(
    seed: &str,
    messages: &[Message],
    model: &str,
    effort: Option<&str>,
) {
    save_session(seed, messages, model, effort);
}
