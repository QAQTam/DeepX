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

    state.health.context_tokens = state.tokens_used();
    state.health.context_limit = state.config.context_limit;

    // Age all per-file counters
    state.age_files();

    // Inject stale-document warnings into turn annotations
    document_annotations(state);

    state.health.record_turn();
}

/// Generate a short tag for a file path.
pub fn doc_tag(path: &str) -> String {
    let mut h = DefaultHasher::new();
    h.write(path.as_bytes());
    format!("{:04x}", h.finish() as u16)
}

/// Scan tracked files and inject turnover annotations for stale or modified docs.
fn document_annotations(state: &mut AgentState) {
    let mut stale: Vec<String> = Vec::new();

    for (path, turns) in &state.file_last_read {
        if *turns >= 7 {
            let tag = doc_tag(path);
            stale.push(format!("  tag:{} {} — {} turns since last read. Stale — re-read before editing.",
                tag, path, turns));
        } else if *turns >= 3 {
            let tag = doc_tag(path);
            stale.push(format!("  tag:{} {} — {} turns since last read. Nearing stale.",
                tag, path, turns));
        }
    }

    if !stale.is_empty() {
        stale.sort();
        let mut msg = String::from("[system] Document status:\n");
        for s in &stale {
            msg.push_str(s);
            msg.push('\n');
        }
        state.turn_annotations.push(msg);
    }
}

