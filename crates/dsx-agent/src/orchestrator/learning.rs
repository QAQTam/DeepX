//! Post-turn processing: health snapshots, document tracking, explore-before-read tracking.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

use crate::agent::AgentState;
use dsx_types::Message;

pub fn post_turn_maintenance(state: &mut AgentState, _final_msg: &Message) {
    const MAX_TOOL_HISTORY: usize = 80;
    let excess = state.tool_results.len().saturating_sub(MAX_TOOL_HISTORY);
    if excess > 0 {
        state.tool_results.drain(0..excess);
    }

    // Only advance staleness clock when files were written this turn.
    // Chat-only turns don't make files go stale.
    if !state.files.files_written_this_turn.is_empty() {
        state.files.staleness_epoch += 1;
    }

    // Inject stale-document warnings into turn annotations
    document_annotations(state);

    state.turn_count += 1;
}

/// Generate a short tag for a file path.
pub fn doc_tag(path: &str) -> String {
    let mut h = DefaultHasher::new();
    h.write(path.as_bytes());
    format!("{:04x}", h.finish() as u16)
}

/// Scan tracked files and inject turnover annotations for stale or modified docs.
/// Only flags files that are actually stale per is_file_stale() — i.e. files
/// that were written to after they were last read, or read long ago while
/// other writes happened. Truncates to top 5 to avoid context pollution.
fn document_annotations(state: &mut AgentState) {
    let mut stale: Vec<(String, u32)> = Vec::new();

    for (path, &read_at) in &state.files.file_read_at {
        if state.is_file_stale(path) {
            stale.push((path.clone(), read_at));
        }
    }

    if stale.is_empty() {
        return;
    }

    // Sort by risk: older reads first (more likely to be stale)
    stale.sort_by_key(|(_, read_at)| *read_at);
    stale.truncate(5);

    let mut msg = String::from("[system] Document status (top 5 stale):\n");
    for (path, read_at) in &stale {
        let tag = doc_tag(path);
        msg.push_str(&format!(
            "  tag:{} {} — last read turn {}. Stale — re-read before editing.\n",
            tag, path, read_at
        ));
    }
    state.turn.annotations.push(msg);
}
