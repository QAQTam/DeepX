//! Session lifecycle: initialization, health status.

use crate::agent::AgentState;
use deepx_tools;
use deepx_session::SessionManager;

/// Load session from disk via [`SessionManager`].
///
/// On success, restores the message store and rebinds the workspace.
/// On failure (file missing or corrupt), generates a fresh seed and
/// creates a new session as fallback. Returns `false` only when
/// `restore_seed` is `None`.
pub fn init_session(agent: &mut AgentState, restore_seed: Option<&str>) -> bool {
    let seed = match restore_seed {
        Some(s) => {
            eprintln!("[LIFECYCLE] init_session: loading seed={s}");
            if let Some(file) = SessionManager::global().load(s) {
                eprintln!("[LIFECYCLE] loaded session file, {} messages", file.messages.len());
                agent.session.seed = file.seed.clone();
                agent.session.start = file.created_at;
                let (msg, repairs) = deepx_message::MessageStore::from_session(&file);
                eprintln!("[LIFECYCLE] from_session done, {} turns, {} repairs", msg.turn_count(), repairs.len());
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
            // Session file not found or checksum mismatch — don't reuse broken seed.
            // Generate a fresh seed so we don't overwrite the corrupted file.
            log::error!(
                "deepx-agent: session {} load failed — creating fresh session",
                s
            );
            eprintln!("[LIFECYCLE] load failed for {s}, generating new seed");
            SessionManager::generate_seed()
        }
        None => return false,
    };

    // Create fresh session (either no restore_seed, or restore failed)
    agent.session.seed = seed.clone();
    agent.session.start = SessionManager::now_epoch();
    agent.session.tokens = 0;
    agent.session.from_resume = false;
    agent.msg = deepx_message::MessageStore::new(&seed);
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

/// Create a brand-new session with a fresh seed, clearing all prior state.
pub fn create_session(agent: &mut AgentState) {
    agent.session.seed = SessionManager::generate_seed();
    agent.session.start = SessionManager::now_epoch();
    agent.session.tokens = 0;
    agent.session.from_resume = false;
    agent.msg = deepx_message::MessageStore::new(&agent.session.seed);
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
