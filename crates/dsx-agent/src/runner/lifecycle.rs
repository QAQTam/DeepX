//! Session lifecycle: initialization, health status.

use crate::agent::AgentState;
use crate::dsx_log;
use crate::session;
use crate::tools;

pub fn init_session(agent: &mut AgentState, restore_seed: Option<&str>) -> bool {
    let seed = match restore_seed {
        Some(s) => {
            if let Some(file) = session::load_session(s) {
                agent.session.seed = file.seed.clone();
                agent.session.start = file.created_at;
                let (ctx, repairs) = crate::assembly::ContextAssembler::from_legacy(file.messages);
                agent.ctx = ctx;
                agent.session.from_resume = true;
                agent.session.tokens = 0;

                dsx_log::set_session(&agent.session.seed);
                tools::set_current_session(&agent.session.seed);
                tools::load_workspace(&agent.session.seed);
                log::info!(
                    "dsx-agent: restored session {} ({} msgs, {} tokens)",
                    agent.session.seed,
                    agent.ctx.message_count(),
                    agent.session.tokens
                );
                if !repairs.is_empty() {
                    log::warn!("session restore: {:?} repairs", repairs);
                }
                return true;
            }
            log::info!("dsx-agent: session {s} not found, creating new with same seed");
            s.to_string()
        }
        None => return false,
    };

    agent.session.seed = seed;
    agent.session.start = session::now_epoch();
    dsx_log::set_session(&agent.session.seed);
    tools::set_current_session(&agent.session.seed);
    session::save_live_snapshot(
        &agent.session.seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        Some(&agent.config.reasoning_effort),
    );
    log::info!("dsx-agent: new session {}", agent.session.seed);
    true
}

pub fn create_session(agent: &mut AgentState) {
    agent.ctx = crate::assembly::ContextAssembler::new();
    agent.session.seed = session::generate_seed();
    agent.session.start = session::now_epoch();
    agent.session.tokens = 0;
    agent.token_estimate = 0;
    agent.api_usage = None;
    agent.tool_results.clear();
    agent.turn.reset();
    dsx_log::set_session(&agent.session.seed);
    tools::set_current_session(&agent.session.seed);
    session::save_live_snapshot(
        &agent.session.seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        Some(&agent.config.reasoning_effort),
    );
    log::info!("dsx-agent: new session {}", agent.session.seed);
}
