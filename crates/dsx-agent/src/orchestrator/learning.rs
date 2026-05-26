//! Post-turn processing: health snapshots, explore tracking.
//! (Memory extraction removed — will be redesigned later)

use crate::agent::AgentState;
use dsx_types::Message;

pub fn auto_extract_memory(state: &mut AgentState, _final_msg: &Message) {
    let mut had_errors = false;
    for (_tool_name, result) in state.tool_results.drain(..) {
        if result.starts_with("[ERROR]") || result.starts_with("[FAIL]") {
            had_errors = true;
        }
    }

    // Update context health stats
    state.health.context_tokens = state.tokens_used();
    state.health.context_limit = state.config.context_limit;

    // Idle detection
    let is_coding = state.current_task_phase == dsx_types::TaskPhase::Coding;
    if state.tool_calls_this_turn == 0 && is_coding {
        state.health.idle_chat_turns = state.health.idle_chat_turns.saturating_add(1);
    } else {
        state.health.idle_chat_turns = 0;
    }

    // Explore-before-read: track turns since last read, warn at 3+
    state.turns_since_last_read = state.turns_since_last_read.saturating_add(1);
    if state.turns_since_last_read == 3 && state.has_explored {
        state.pending_notes.push(format!(
            "{} turns without reading files. Context may be missing. Consider read_file().",
            state.turns_since_last_read));
    }

    state.health.record_turn(had_errors);

    // Health status line for TUI
    let context_pct = if state.health.context_limit > 0 {
        (state.health.context_tokens as f64 / state.health.context_limit as f64 * 100.0) as u32
    } else { 0 };
    state.health_status_line = format!(
        "t{} | tier:{:?} | ctx:{}%",
        state.health.turn, state.health.context_tier, context_pct,
    );
}
