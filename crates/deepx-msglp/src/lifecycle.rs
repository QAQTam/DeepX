//! Session lifecycle: initialization, health status.

use crate::agent::AgentState;
use deepx_tools;
use deepx_session::SessionManager;

pub fn init_session(agent: &mut AgentState, restore_seed: Option<&str>) -> bool {
    let seed = match restore_seed {
        Some(s) => {
            if let Some(file) = SessionManager::global().load(s) {
                agent.session.seed = file.seed.clone();
                agent.session.start = file.created_at;
                let (msg, repairs) = deepx_message::MessageStore::from_session(&file);
                agent.msg = msg;
                agent.session.from_resume = true;
                agent.session.tokens = 0;

                deepx_tools::bridge::set_current_session(&agent.session.seed);
                deepx_tools::bridge::load_workspace(&agent.session.seed);
                    log::info!(
                        "deepx-agent: restored session {} ({} msgs, {} tokens)",
                    agent.session.seed,
                    agent.msg.message_count(),
                    agent.session.tokens
                );
                if !repairs.is_empty() {
                    log::warn!("session restore: {:?} repairs", repairs);
                }
                return true;
            }
            log::info!("deepx-agent: session {s} not found, creating new with same seed");
            s.to_string()
        }
        None => return false,
    };

    agent.session.seed = seed;
    agent.session.start = SessionManager::now_epoch();
            deepx_tools::bridge::set_current_session(&agent.session.seed);
    SessionManager::global().save(
        &agent.session.seed,
        &agent.msg.to_vec(),
        &agent.config.model,
        Some(&agent.config.reasoning_effort),
    );
    log::info!("deepx-agent: new session {}", agent.session.seed);
    true
}

pub fn create_session(agent: &mut AgentState) {
    agent.msg = deepx_message::MessageStore::new(&agent.session.seed);
    agent.session.seed = SessionManager::generate_seed();
    agent.session.start = SessionManager::now_epoch();
    agent.session.tokens = 0;
    agent.session.from_resume = false;
    agent.tool_results.clear();
        deepx_tools::bridge::set_current_session(&agent.session.seed);
    SessionManager::global().save(
        &agent.session.seed,
        &agent.msg.to_vec(),
        &agent.config.model,
        Some(&agent.config.reasoning_effort),
    );
    log::info!("deepx-agent: new session {}", agent.session.seed);
}
