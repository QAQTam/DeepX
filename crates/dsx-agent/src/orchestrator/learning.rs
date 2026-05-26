//! Post-turn processing: health snapshots, explore-before-read tracking.

use crate::agent::AgentState;
use dsx_types::Message;

pub fn post_turn_maintenance(state: &mut AgentState, _final_msg: &Message) {
    let mut had_errors = false;
    for (_tool_name, result) in state.tool_results.drain(..) {
        if result.starts_with("[ERROR]") || result.starts_with("[FAIL]") {
            had_errors = true;
        }
    }

    // Update context health stats
    state.health.context_tokens = state.tokens_used();
    state.health.context_limit = state.config.context_limit;

    // Explore-before-read: track turns since last read
    state.turns_since_last_read = state.turns_since_last_read.saturating_add(1);

    state.health.record_turn(had_errors);
}
