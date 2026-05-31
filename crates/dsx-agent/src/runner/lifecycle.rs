//! Session lifecycle: initialization, health status, phase configuration.

use crate::agent::AgentState;
use crate::dsx_log;
use crate::router;
use crate::session;
use crate::tools;
use dsx_types::DebugLevel;

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
        log::info!("dsx-agent: session {seed} not found, creating new");
    }
    agent.session_seed = session::generate_seed();
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

/// Apply phase-specific model config (auto or user-specified).
pub fn apply_phase_config(
    agent: &mut AgentState,
    phase: dsx_types::TaskPhase,
    level: DebugLevel,
) {
    let phase_name = format!("{:?}", phase).to_lowercase();
    if let Some(user_pc) = agent.config.phase_configs.get(&phase_name) {
        agent.config.model = user_pc.model.clone();
        agent.config.effort = user_pc.effort.clone().filter(|e| !e.is_empty());
        agent.config.max_tokens = user_pc.max_tokens;
        agent.config.context_limit = user_pc.context_limit;
    } else {
        let pc = router::phase_config(phase, level);
        agent.config.model = pc.model.to_string();
        agent.config.effort = pc.effort.map(|s| s.to_string());
        agent.config.max_tokens = pc.max_tokens;
    }
}
