//! Configuration, tool listing, skill management, workspace, session list commands.

use super::super::registry::{AgentRegistry, send_to_agent};
use deepx_proto::Ui2Agent;

#[tauri::command]
pub fn cmd_unload_skill(seed: String, name: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_unload_skill seed={} name={name}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    send_to_agent(&seed, Ui2Agent::UnloadSkill { name })
}

/// Explicitly activate a skill by name (equivalent to $skill-name mention).

#[tauri::command]
pub fn cmd_activate_skill(seed: String, name: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_activate_skill seed={} name={name}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    send_to_agent(&seed, Ui2Agent::ActivateSkill { name })
}

#[tauri::command]
pub fn cmd_skill_operation(
    seed: String,
    operation_id: String,
    action: String,
    name: String,
    expected_revision: u64,
) -> Result<(), String> {
    send_to_agent(
        &seed,
        Ui2Agent::SkillOperation {
            operation_id,
            action,
            name,
            expected_revision,
        },
    )
}

/// Reload the skill catalog from disk and refresh the catalog system message.

#[tauri::command]
pub fn cmd_reload_skills(seed: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_reload_skills seed={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    send_to_agent(&seed, Ui2Agent::ReloadSkills)
}

/// Return the app version from Cargo.toml.

#[tauri::command]
pub fn cmd_get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Return all registered tool names. Used by Settings for default tools.

#[tauri::command]
pub fn cmd_list_available_tools() -> Result<String, String> {
    let tools = deepx_tools::runtime::all_tool_names();
    serde_json::to_string(&tools).map_err(|e| format!("{e}"))
}

/// Resume an existing session — spawns agent with --resume-seed if not already running.
/// The agent auto-loads the session on startup and emits SessionRestored.

#[tauri::command]
pub fn cmd_save_config(
    api_key: String,
    model: String,
    base_url: String,
    provider_id: String,
    endpoint: String,
    max_tokens: u32,
    context_limit: u32,
    reasoning_effort: String,
    lang: String,
    subagent_model: String,
    subagent_base_url: String,
    subagent_api_key: String,
    subagent_max_tokens: u32,
    subagent_timeout_secs: u64,
    subagent_default_tools: Vec<String>,
    database_enabled: bool,
    tokenizer_path: String,
) -> Result<(), String> {
    let mut cfg = deepx_config::Config::load().unwrap_or_default();
    let set_str = |dest: &mut String, src: &str| {
        if !src.is_empty() {
            *dest = src.to_string();
        }
    };
    let set_u32 = |dest: &mut u32, src: u32| {
        if src > 0 {
            *dest = src;
        }
    };
    let set_u64 = |dest: &mut u64, src: u64| {
        if src > 0 {
            *dest = src;
        }
    };

    set_str(&mut cfg.api_key, &api_key);
    set_str(&mut cfg.model, &model);
    set_str(&mut cfg.base_url, &base_url);
    set_str(&mut cfg.provider_id, &provider_id);
    set_str(&mut cfg.endpoint, &endpoint);
    set_u32(&mut cfg.max_tokens, max_tokens);
    set_u32(&mut cfg.context_limit, context_limit);
    set_str(&mut cfg.reasoning_effort, &reasoning_effort);
    if !lang.is_empty() {
        cfg.lang = Some(lang);
    }

    set_str(&mut cfg.subagent.model, &subagent_model);
    set_str(&mut cfg.subagent.base_url, &subagent_base_url);
    set_str(&mut cfg.subagent.api_key, &subagent_api_key);
    set_u32(&mut cfg.subagent.max_tokens, subagent_max_tokens);
    set_u64(&mut cfg.subagent.timeout_secs, subagent_timeout_secs);
    if !subagent_default_tools.is_empty() {
        cfg.subagent.default_tools = subagent_default_tools;
    }
    cfg.database.enabled = database_enabled;
    if !tokenizer_path.is_empty() {
        cfg.tokenizer_path = Some(tokenizer_path);
    } else {
        cfg.tokenizer_path = None;
    }
    cfg.save()?;
    deepx_session::SessionManager::global().set_turso_enabled(cfg.database.enabled);
    // Broadcast reload to all running agents
    let registry = AgentRegistry::get()
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    for seed in registry.instances.keys() {
        let _ = registry.send_to(seed, &Ui2Agent::ReloadConfig);
    }
    Ok(())
}

/// Persist the database mirror toggle and apply it to the running session manager.
///
/// This is intentionally separate from the general settings form so a toggle takes
/// effect even when no agent process is currently running.
#[tauri::command]
pub fn cmd_set_database_enabled(enabled: bool) -> Result<(), String> {
    let mut cfg = deepx_config::Config::load().unwrap_or_default();
    cfg.database.enabled = enabled;
    cfg.save()?;
    deepx_session::SessionManager::global().set_turso_enabled(enabled);

    let registry = AgentRegistry::get()
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    for seed in registry.instances.keys() {
        let _ = registry.send_to(seed, &Ui2Agent::ReloadConfig);
    }
    Ok(())
}

fn validate_permission_level(level: u8) -> Result<u8, String> {
    if (1..=4).contains(&level) {
        Ok(level)
    } else {
        Err(format!(
            "permission level must be between 1 and 4, got {level}"
        ))
    }
}

#[tauri::command]
pub fn cmd_set_permission_level(level: u8) -> Result<(), String> {
    let level = validate_permission_level(level)?;
    let mut cfg = deepx_config::Config::load().unwrap_or_default();
    cfg.permission_level = level;
    cfg.save()?;

    let registry = AgentRegistry::get()
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    for seed in registry.instances.keys() {
        let _ = registry.send_to(seed, &Ui2Agent::ReloadConfig);
    }
    Ok(())
}

/// Resolve the `.deepx/` directory path.
/// Priority: {workspace}/.deepx/ → {data_dir}/workspace/ (fallback)
pub(crate) fn resolve_deepx_dir(seed: &str) -> std::path::PathBuf {
    let ws = cmd_get_workspace(seed.to_string()).unwrap_or_default();
    if !ws.is_empty() && ws != "." {
        std::path::Path::new(&ws).join(".deepx")
    } else {
        deepx_types::platform::data_dir().join("workspace")
    }
}

/// Load the current config and return it as JSON.

#[tauri::command]
pub fn cmd_load_config() -> Result<String, String> {
    let cfg = deepx_config::Config::load().map_err(|e| format!("load config: {e}"))?;
    let providers: Vec<serde_json::Value> = deepx_config::registry::all_providers()
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "display": p.display,
                "endpoints": p.endpoints.into_iter().map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "display": e.display,
                        "base_url": e.base_url,
                        "default_model": e.default_model,
                        "models": e.models,
                        "stateful": e.stateful,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    let result = serde_json::json!({
        "api_key": if cfg.api_key.is_empty() { "" } else { "****" },
        "model": cfg.model,
        "base_url": cfg.base_url,
        "provider_id": cfg.provider_id,
        "endpoint": cfg.endpoint,
        "max_tokens": cfg.max_tokens,
        "context_limit": cfg.context_limit,
        "reasoning_effort": cfg.reasoning_effort,
        "permission_level": cfg.permission_level,
        "lang": cfg.lang,
        "active_profile": cfg.active_profile,
        "providers": providers,
        "subagent": {
            "model": cfg.subagent.model,
            "base_url": cfg.subagent.base_url,
            "api_key": if cfg.subagent.api_key.is_empty() { "" } else { "****" },
            "max_tokens": cfg.subagent.max_tokens,
            "timeout_secs": cfg.subagent.timeout_secs,
            "default_tools": cfg.subagent.default_tools,
        },
        "database": {
            "enabled": cfg.database.enabled,
        },
    });
    serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"))
}

#[cfg(test)]
mod permission_level_tests {
    use super::validate_permission_level;

    #[test]
    fn accepts_only_levels_one_through_four() {
        assert_eq!(validate_permission_level(1), Ok(1));
        assert_eq!(validate_permission_level(4), Ok(4));
        assert!(validate_permission_level(0).is_err());
        assert!(validate_permission_level(5).is_err());
    }
}

/// List all sessions with metadata.

#[tauri::command]
pub fn cmd_list_sessions() -> Result<String, String> {
    let metas = deepx_session::SessionManager::global().list();
    // Inject turso-backed flag
    let mgr = deepx_session::SessionManager::global();
    let result: Vec<serde_json::Value> = metas
        .into_iter()
        .map(|m| {
            let mut v = serde_json::to_value(&m).unwrap_or_default();
            let backed = mgr.is_turso_backed(&m.seed);
            v["turso_backed"] = serde_json::Value::Bool(backed);
            // Check if an agent process is running for this session
            let running = if let Ok(reg) = AgentRegistry::get().lock() {
                reg.has_instance(&m.seed)
            } else {
                false
            };
            v["running"] = serde_json::Value::Bool(running);
            v
        })
        .collect();
    serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"))
}

/// Return the authoritative activity snapshot for every spawned session.
#[tauri::command]
pub fn cmd_list_session_activity() -> Result<Vec<deepx_proto::SessionActivity>, String> {
    let registry = AgentRegistry::get()
        .lock()
        .map_err(|error| format!("lock: {error}"))?;
    Ok(registry.session_activity())
}

/// Compare every JSONL session with its Turso mirror without changing data.
#[tauri::command]
pub fn cmd_audit_turso_mirrors() -> Result<String, String> {
    serde_json::to_string(&deepx_session::SessionManager::global().audit_all_mirrors())
        .map_err(|e| format!("serialize mirror audit: {e}"))
}

/// Replay durable file outboxes and bring JSONL-authoritative sessions back
/// into parity with their Turso mirrors.
#[tauri::command]
pub fn cmd_reconcile_turso_mirrors() -> Result<String, String> {
    serde_json::to_string(&deepx_session::SessionManager::global().reconcile_all_mirrors())
        .map_err(|e| format!("serialize mirror reconciliation: {e}"))
}

/// Non-mutating release gate for enabling DB-primary reads in a later version.
#[tauri::command]
pub fn cmd_check_db_primary_readiness() -> Result<String, String> {
    serde_json::to_string(&deepx_session::SessionManager::global().db_primary_readiness())
        .map_err(|e| format!("serialize DB-primary readiness: {e}"))
}

/// Count sessions pending JSONL → Turso migration.

#[tauri::command]
pub fn cmd_delete_session(seed: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_delete_session seed={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))]
    );
    // Kill the agent first so it doesn't resurrect the session on flush.
    let instance = {
        if let Ok(mut registry) = AgentRegistry::get().lock() {
            registry.kill_agent(&seed)
        } else {
            None
        }
    };
    if let Some(inst) = instance {
        inst.shutdown_and_wait();
    }
    deepx_session::SessionManager::global().delete(&seed)
}

/// Undo a turn and all subsequent content.

#[tauri::command]
pub fn cmd_get_workspace(seed: String) -> Result<String, String> {
    if seed.is_empty() {
        return Ok(String::new());
    }
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    Ok(std::fs::read_to_string(dir.join("workspace.txt"))
        .unwrap_or_default()
        .trim()
        .to_string())
}

/// Set the current session's workspace root path and notify the agent.

#[tauri::command]
pub fn cmd_set_workspace(seed: String, path: String) -> Result<(), String> {
    if seed.is_empty() {
        return Err("No active session".into());
    }
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(dir.join("workspace.txt"), path.trim()).map_err(|e| format!("write: {e}"))?;
    // Tell agent to reload config (which includes workspace).
    // If agent isn't running yet, the change will apply on next spawn.
    if let Err(e) = send_to_agent(&seed, Ui2Agent::ReloadConfig) {
        log::warn!("cmd_set_workspace: ReloadConfig not sent (agent not running): {e}");
    }
    Ok(())
}
