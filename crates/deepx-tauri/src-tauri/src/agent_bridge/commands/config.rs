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
    // Broadcast reload to all running agents
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
