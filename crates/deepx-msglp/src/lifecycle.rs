//! Session lifecycle: initialization, health status.

use crate::agent::AgentState;
use crate::util::chrono_local_date;
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
            log::info!("[LIFECYCLE] init_session: loading seed={s}");
            // Fast check: if the session directory doesn't exist at all, fail early
            // instead of silently creating a new session. This lets the caller
            // send a proper Error event rather than a confusing SessionCreated.
            if !SessionManager::global().exists(s) {
                log::error!(
                    "deepx-agent: session {} not found — directory does not exist",
                    s
                );
                return false;
            }
            if let Some((meta, messages)) = SessionManager::global().load(s) {
                log::info!("[LIFECYCLE] loaded session, {} messages", messages.len());
                agent.session = meta;
                agent.session.from_resume = true;
                agent.session.tokens = 0;
                let (msg, repairs) = deepx_message::MessageStore::from_messages(&agent.session.seed, &messages, agent.session.compact_skip);
                log::info!("[LIFECYCLE] from_messages done, {} turns, {} repairs", msg.turn_count(), repairs.len());
                agent.msg = msg;

                deepx_tools::bridge::set_current_session(&agent.session.seed);
                deepx_tools::bridge::load_workspace(&agent.session.seed);
                // Hot-load latest tool schema (order-stable: new tools appended at end)
                agent.tool_defs = deepx_tools::bridge::all_tools();
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
            // Directory exists but meta or messages are corrupt — generate a
            // fresh seed so we don't overwrite the corrupted files.
            log::error!(
                "deepx-agent: session {} load failed (corrupt?) — creating fresh session",
                s
            );
            log::warn!("[LIFECYCLE] load failed for {s}, generating new seed");
            SessionManager::generate_seed()
        }
        None => return false,
    };

    // Create fresh session (either no restore_seed, or restore failed)
    agent.session.seed = seed.clone();
    agent.session.created_at = SessionManager::now_epoch();
    agent.session.tokens = 0;
    agent.session.from_resume = false;
    agent.msg = if agent.ephemeral {
        deepx_message::MessageStore::new_ephemeral(&seed)
    } else {
        deepx_message::MessageStore::new(&seed)
    };
    deepx_tools::bridge::set_current_session(&agent.session.seed);
    deepx_tools::bridge::load_workspace(&agent.session.seed);
    agent.msg.push_system(deepx_types::Message::system(
        &deepx_config::prompt::full_system_prompt_with_date(
            &chrono_local_date(),
            deepx_config::prompt::OS_INFO.get().map(|s| s.as_str()).unwrap_or(""),
        )
    ));
    agent.msg.flush_meta(&agent.config.model, &agent.config.reasoning_effort);
    log::info!("deepx-agent: new session {}", agent.session.seed);
    true
}

/// Create a brand-new session with a fresh seed, clearing all prior state.
pub fn create_session(agent: &mut AgentState) {
    agent.session.seed = SessionManager::generate_seed();
    agent.session.created_at = SessionManager::now_epoch();
    agent.session.tokens = 0;
    agent.session.from_resume = false;
    agent.msg = if agent.ephemeral {
        deepx_message::MessageStore::new_ephemeral(&agent.session.seed)
    } else {
        deepx_message::MessageStore::new(&agent.session.seed)
    };
    deepx_tools::bridge::set_current_session(&agent.session.seed);
    deepx_tools::bridge::load_workspace(&agent.session.seed);
    agent.msg.push_system(deepx_types::Message::system(
        &deepx_config::prompt::full_system_prompt_with_date(
            &chrono_local_date(),
            deepx_config::prompt::OS_INFO.get().map(|s| s.as_str()).unwrap_or(""),
        )
    ));
    agent.msg.flush_meta(&agent.config.model, &agent.config.reasoning_effort);
    log::info!("deepx-agent: new session {}", agent.session.seed);
}

/// Create a new session with a pre-set seed (from CLI --seed).
/// Unlike create_session, this does NOT generate a new seed.
pub fn create_session_with_seed(agent: &mut AgentState) {
    agent.session.tokens = 0;
    agent.session.from_resume = false;
    agent.msg = if agent.ephemeral {
        deepx_message::MessageStore::new_ephemeral(&agent.session.seed)
    } else {
        deepx_message::MessageStore::new(&agent.session.seed)
    };
    deepx_tools::bridge::set_current_session(&agent.session.seed);
    deepx_tools::bridge::load_workspace(&agent.session.seed);
    agent.msg.push_system(deepx_types::Message::system(
        &deepx_config::prompt::full_system_prompt_with_date(
            &chrono_local_date(),
            deepx_config::prompt::OS_INFO.get().map(|s| s.as_str()).unwrap_or(""),
        )
    ));
    agent.msg.flush_meta(&agent.config.model, &agent.config.reasoning_effort);
    log::info!("deepx-agent: new session with preset seed {}", agent.session.seed);
}
