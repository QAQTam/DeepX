//! Session lifecycle: initialization, health status.

use crate::agent::AgentState;
use crate::dsx_log;
use crate::session;
use crate::tools;

/// Initialize a session (new or restored from disk).
pub fn init_session(agent: &mut AgentState, restore_seed: Option<&str>) {
    if let Some(seed) = restore_seed {
        if let Some(file) = session::load_session(seed) {
            agent.session_seed = file.seed.clone();
            agent.session_start = file.created_at;
            let (ctx, repairs) = crate::assembly::ContextAssembler::from_legacy(file.messages);
            agent.ctx = ctx;
            agent.session_tokens = 0;

            dsx_log::set_session(&agent.session_seed);
            tools::set_current_session(&agent.session_seed);
            tools::load_workspace(&agent.session_seed);
            log::info!(
                "dsx-agent: restored session {} ({} msgs, {} tokens)",
                agent.session_seed,
                agent.ctx.message_count(),
                agent.session_tokens
            );
            if !repairs.is_empty() {
                log::warn!("session restore: {:?} repairs", repairs);
            }
            return;
        }
        log::info!("dsx-agent: session {seed} not found, creating new with same seed");
        agent.session_seed = seed.to_string();
    } else {
        agent.session_seed = session::generate_seed();
    }
    agent.session_start = session::now_epoch();
    dsx_log::set_session(&agent.session_seed);
    tools::set_current_session(&agent.session_seed);
    session::save_live_snapshot(
        &agent.session_seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        agent.config.effort.as_deref(),
    );
    log::info!("dsx-agent: new session {}", agent.session_seed);
}
