pub mod gates;
pub mod tracker;

pub mod learning;


// ── maybe_save_session ──

use crate::session;
use crate::agent::AgentState;

/// Save session to disk if there are messages and a seed,
/// and no pending tool calls (avoid saving broken restore state).
pub fn maybe_save_session(state: &mut AgentState) {
    if state.ctx.has_pending_tools() { return; }
    let msgs = state.ctx.to_vec();
    if msgs.len() > 1 && !state.session_seed.is_empty() {
        session::finalize_session(
            &state.session_seed,
            &msgs,
            &state.config.model,
            state.config.effort.as_deref(),
        );
    }
}
