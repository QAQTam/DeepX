//! Session lifecycle: initialization, health status.

use crate::agent::AgentState;
use dsx_tools;
use dsx_session::SessionManager;

pub fn init_session(agent: &mut AgentState, restore_seed: Option<&str>) -> bool {
    let seed = match restore_seed {
        Some(s) => {
            if let Some(file) = SessionManager::global().load(s) {
                agent.session.seed = file.seed.clone();
                agent.session.start = file.created_at;
                let (msg, repairs) = dsx_message::MessageStore::from_session(&file);
                agent.msg = msg;
                agent.session.from_resume = true;
                agent.session.tokens = 0;

                                tools::set_current_session(&agent.session.seed);
                tools::load_workspace(&agent.session.seed);
                log::info!(
                    "dsx-agent: restored session {} ({} msgs, {} tokens)",
                    agent.session.seed,
                    agent.msg.message_count(),
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
    agent.session.start = SessionManager::now_epoch();
        tools::set_current_session(&agent.session.seed);
    SessionManager::global().save(
        &agent.session.seed,
        &agent.msg.to_vec(),
        &agent.config.model,
        Some(&agent.config.reasoning_effort),
    );
    log::info!("dsx-agent: new session {}", agent.session.seed);
    true
}

pub fn create_session(agent: &mut AgentState) {
    agent.msg = dsx_message::MessageStore::new(&agent.session.seed);
    agent.session.seed = SessionManager::generate_seed();
    agent.session.start = SessionManager::now_epoch();
    agent.session.tokens = 0;
    // token_estimate / api_usage removed (tracked via session.tokens only)
    agent.tool_results.clear();
            tools::set_current_session(&agent.session.seed);
    SessionManager::global().save(
        &agent.session.seed,
        &agent.msg.to_vec(),
        &agent.config.model,
        Some(&agent.config.reasoning_effort),
    );
    log::info!("dsx-agent: new session {}", agent.session.seed);
}
