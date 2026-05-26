pub mod gates;
pub mod tracker;

pub mod turn_scorer;
pub mod learning;
pub mod phase_detector;

// ── maybe_save_session ──

use crate::session;
use crate::agent::AgentState;

/// Save session to disk if there are messages and a seed.
pub fn maybe_save_session(state: &mut AgentState) {
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
