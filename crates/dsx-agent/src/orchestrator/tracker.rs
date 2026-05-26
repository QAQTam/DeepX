//! Tracking: file writes, assistant content analysis.
use crate::agent::AgentState;

/// Track a file written/edited this turn for sandbox enforcement.
pub fn track_file_written(state: &mut AgentState, args: &str) {
    if let Some(p) = dsx_types::arg::parse_file_arg(args) {
        if !state.files_written_this_turn.contains(&p) {
            state.files_written_this_turn.push(p.clone());
        }
        if let Ok(abs) = std::path::absolute(&p) {
            let abs_str = abs.to_string_lossy().to_string();
            if abs_str != p && !state.files_written_this_turn.contains(&abs_str) {
                state.files_written_this_turn.push(abs_str);
            }
        }
    }
}

/// Get the last assistant message text content (for intent gate).
pub fn last_assistant_content(state: &AgentState) -> String {
    state.ctx.to_vec().iter().rev()
        .find(|m| m.role == "assistant" && !m.content.is_empty())
        .and_then(|m| {
            m.content.iter().find_map(|b| {
                if let dsx_types::ContentBlock::Text { text } = b {
                    Some(text.clone())
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

