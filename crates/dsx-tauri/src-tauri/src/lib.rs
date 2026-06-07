use std::sync::{mpsc, Mutex};
use std::thread::{self, JoinHandle};
use tauri::{AppHandle, Emitter, Manager};
use dsx_proto::{Agent2Ui, Ui2Agent};

struct AgentState {
    tui_tx: Mutex<Option<mpsc::Sender<Ui2Agent>>>,
    agent_handle: Mutex<Option<JoinHandle<()>>>,
    reader_handle: Mutex<Option<JoinHandle<()>>>,
    op_lock: Mutex<()>,
    config_lock: Mutex<()>,
    session_seed: Mutex<Option<String>>,
}

fn data_dir() -> std::path::PathBuf {
    dsx_types::platform::data_dir()
}

fn config_path() -> std::path::PathBuf {
    dsx_types::platform::config_path()
}


#[tauri::command]
fn send_message(state: tauri::State<AgentState>, text: String) -> Result<(), String> {
    let guard = state.tui_tx.lock().map_err(|e| format!("lock: {e}"))?;
    let tx = guard.as_ref().ok_or("Agent not started")?;
    tx.send(Ui2Agent::UserInput { text }).map_err(|e| format!("send: {e}"))?;
    Ok(())
}

#[tauri::command]
fn reload_agent(state: tauri::State<AgentState>) -> Result<(), String> {
    let guard = state.tui_tx.lock().map_err(|e| format!("lock: {e}"))?;
    let tx = guard.as_ref().ok_or("Agent not started")?;
    tx.send(Ui2Agent::ReloadConfig).map_err(|e| format!("send: {e}"))?;
    Ok(())
}

fn scan_sessions() -> Vec<serde_json::Value> {
    let dir = data_dir().join("sessions");
    let mut sessions = Vec::new();
    if !dir.is_dir() { return sessions; }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut seen = std::collections::HashSet::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();
            // New format: sessions/{seed}-{date}/session.toml
            if path.is_dir() {
                let inner = path.join("session.toml");
                if !inner.is_file() { continue; }
                if let Ok(data) = std::fs::read_to_string(&inner) {
                    let parsed: Option<serde_json::Value> = toml::from_str::<toml::Value>(&data)
                        .ok()
                        .and_then(|tv| serde_json::to_value(tv).ok())
                        .or_else(|| serde_json::from_str(&data).ok());
                    if let Some(mut meta) = parsed {
                        if meta.get("message_count").is_none() {
                            let count = meta["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                            meta["message_count"] = serde_json::json!(count);
                        }
                        if let Some(seed) = meta.get("seed").and_then(|s| s.as_str()) {
                            if seen.insert(seed.to_string()) {
                                sessions.push(meta);
                            }
                        }
                    }
                }
                continue;
            }
            // Old format: flat .toml or .json files (skip index, pitfalls)
            if path.is_file() && path.extension().map(|e| e == "toml" || e == "json").unwrap_or(false) {
                if fname == "index.toml" || fname == "index.json" || fname == "pitfalls.json" { continue; }
                let is_session = fname.ends_with(".live.json") || fname.ends_with(".json")
                    || fname.ends_with(".live.toml") || fname.ends_with(".toml");
                if is_session {
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&data) {
                            if meta.get("message_count").is_none() {
                                let count = meta["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                                meta["message_count"] = serde_json::json!(count);
                            }
                            if let Some(seed) = meta.get("seed").and_then(|s| s.as_str()) {
                                if seen.insert(seed.to_string()) {
                                    sessions.push(meta);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    sessions.sort_by(|a, b| {
        let au = a["updated_at"].as_u64().unwrap_or(0);
        let bu = b["updated_at"].as_u64().unwrap_or(0);
        bu.cmp(&au)
    });
    sessions
}

#[tauri::command]
fn load_session_messages(seed: String) -> Result<serde_json::Value, String> {
    let dir = data_dir().join("sessions");
    if !dir.is_dir() { return Err("No sessions directory".into()); }

    let mut file_data: Option<String> = None;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            // New format: sessions/{seed}-{date}/session.toml
            if path.is_dir() && fname.starts_with(&format!("{}-", seed)) {
                for inner_name in &["session.toml", "session.json"] {
                    let inner = path.join(inner_name);
                    if let Ok(data) = std::fs::read_to_string(&inner) {
                        file_data = Some(data);
                        break;
                    }
                }
                if file_data.is_some() { break; }
            }
            // Old format: sessions/{seed}.{toml|json} or {seed}.live.{toml|json}
            let is_old = |ext: &str| {
                path.is_file() && (
                    fname == format!("{}.{}", seed, ext) || fname == format!("{}.live.{}", seed, ext)
                )
            };
            if is_old("toml") || is_old("json") {
                file_data = std::fs::read_to_string(&path).ok();
                break;
            }
        }
    }

    let data = file_data.ok_or_else(|| format!("Session not found: {seed}"))?;
    let file: dsx_types::SessionFile = toml::from_str(&data)
        .or_else(|_| serde_json::from_str(&data))
        .map_err(|e| format!("Parse session: {e}"))?;
    Ok(serde_json::json!({ "messages": session_to_frontend(&file) }))
}

fn session_to_frontend(file: &dsx_types::SessionFile) -> serde_json::Value {
    use dsx_types::ContentBlock;
    let mut ui: Vec<serde_json::Value> = Vec::new();

    for msg in &file.messages {
        match msg.role.as_str() {
            "user" => {
                let content: String = msg.content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                }).collect();
                ui.push(serde_json::json!({"role": "user", "content": content}));
            }
            "assistant" => {
                let mut content = String::new();
                let mut reasoning = String::new();
                let mut reasoning_segs: Vec<String> = Vec::new();
                let mut tool_calls: Vec<serde_json::Value> = Vec::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => content.push_str(text),
                        ContentBlock::Reasoning { reasoning: r } => {
                            reasoning.push_str(r);
                            reasoning_segs.push(r.clone());
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_calls.push(serde_json::json!({
                                "id": id, "name": name,
                                "args": input.clone(),
                                "output": "",
                            }));
                        }
                        _ => {}
                    }
                }
                ui.push(serde_json::json!({
                    "role": "assistant", "content": content,
                    "reasoning": if reasoning.is_empty() { serde_json::Value::Null } else { serde_json::json!(reasoning) },
                    "reasoningSegments": reasoning_segs,
                        "tool_cards": if tool_calls.is_empty() { serde_json::Value::Null } else { serde_json::json!(tool_calls) },
                }));
            }
            "tool" => {
                for block in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content } = block {
                        if let Some(last) = ui.last_mut() {
                            if last["role"] == "assistant" {
                                if let Some(tcs) = last["tool_cards"].as_array_mut() {
                                    for tc in tcs.iter_mut() {
                                        if tc["id"].as_str() == Some(tool_use_id.as_str()) {
                                            tc["output"] = serde_json::json!(content);
                                            let s = content.as_str();
                                            let success = !s.starts_with("[ERROR]") && !s.starts_with("[FAIL]");
                                            tc["success"] = serde_json::json!(success);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    serde_json::json!(ui)
}

#[tauri::command]
fn start_agent(app: AppHandle, state: tauri::State<AgentState>) -> Result<serde_json::Value, String> {
    let _op = state.op_lock.lock().map_err(|e| format!("lock: {e}"))?;
    start_agent_inner(&app, &state)
}

fn start_agent_inner(app: &AppHandle, state: &AgentState) -> Result<serde_json::Value, String> {
    if state.tui_tx.lock().map_err(|e| format!("lock: {e}"))?.is_some() {
        restart_agent_inner(app, state, None)?;
        let sessions = scan_sessions();
        return Ok(serde_json::json!({"ok": true, "sessions": sessions}));
    }

    let agent = dsx_agent::agent::AgentState::init("tauri");

    let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    // Reader thread: Agent2Ui → Tauri events
    let app_handle = app.clone();
    let rdr_handle = thread::spawn(move || {
        while let Ok(frame) = agent_rx.recv() {
            match &frame {
                Agent2Ui::SessionRestored { ref seed, .. } | Agent2Ui::SessionCreated { ref seed } => {
                    if let Ok(mut guard) = app_handle.state::<AgentState>().session_seed.lock() {
                        *guard = Some(seed.clone());
                    }
                }
                _ => {}
            }
            if let Ok(v) = serde_json::to_value(&frame) {
                let _ = app_handle.emit("agent-event", v);
            }
        }
        log::info!("agent: reader thread exiting");
    });

    // Agent thread
    let hdl = thread::spawn(move || {
        dsx_agent::runner::run_agent_loop(agent, tui_rx, agent_tx);
        dsx_agent::tools::shutdown_tools();
    });

    *state.tui_tx.lock().map_err(|e| format!("lock: {e}"))? = Some(tui_tx);
    *state.agent_handle.lock().map_err(|e| format!("lock: {e}"))? = Some(hdl);
    *state.reader_handle.lock().map_err(|e| format!("lock: {e}"))? = Some(rdr_handle);
    *state.session_seed.lock().map_err(|e| format!("lock: {e}"))? = None;

    let sessions = scan_sessions();
    log::info!("agent connected (in-process)");
    Ok(serde_json::json!({"ok": true, "sessions": sessions}))
}

#[tauri::command]
fn check_agent_status(state: tauri::State<AgentState>) -> Result<serde_json::Value, String> {
    let running = state.tui_tx.lock().map_err(|e| format!("lock: {e}"))?.is_some();
    let seed = state.session_seed.lock().map_err(|e| format!("lock: {e}"))?.clone();
    Ok(serde_json::json!({"running": running, "seed": seed}))
}

#[tauri::command]
fn check_config() -> Result<bool, String> {
    Ok(config_path().exists())
}

#[tauri::command]
fn save_config(state: tauri::State<AgentState>, api_key: String, base_url: String, model: String, context_limit: u32, max_tokens: u32, provider_id: String, endpoint: String, reasoning_effort: String, lang: String, context7_api_key: String) -> Result<(), String> {
    let _lock = state.config_lock.lock().map_err(|e| format!("lock: {e}"))?;
    let store = dsx_types::ConfigStore::default_location();
    let mut cfg = store.load().unwrap_or_default();
    if !api_key.is_empty() { cfg.api_key = Some(api_key); }
    if !base_url.is_empty() { cfg.base_url = Some(base_url); }
    if !model.is_empty() { cfg.model = Some(model); }
    cfg.context_limit = Some(context_limit);
    cfg.max_tokens = Some(max_tokens);
    if !provider_id.is_empty() { cfg.provider_id = Some(provider_id); }
    if !endpoint.is_empty() { cfg.endpoint = Some(endpoint); }
    if !reasoning_effort.is_empty() { cfg.reasoning_effort = Some(reasoning_effort); }
    if !lang.is_empty() { cfg.lang = Some(lang); }
    if !context7_api_key.is_empty() { cfg.context7_api_key = Some(context7_api_key); }
    if !store.save(&cfg) { return Err("Failed to save config".into()); }
    Ok(())
}


#[tauri::command]
async fn get_balance(api_key: String) -> Result<serde_json::Value, String> {
    let key = api_key.trim().trim_matches('"');
    if key.is_empty() || key == "null" {
        return Err("API key is invalid".to_string());
    }
    let store = dsx_types::ConfigStore::default_location();
    let (pid, ep) = dsx_agent::gate::registry::first_provider_endpoint();
    let base_url = store.load()
        .and_then(|c| c.base_url)
        .unwrap_or_else(|| dsx_agent::gate::registry::base_url_for(&pid, &ep));
    let stripped = base_url.trim_end_matches('/').trim_end_matches("/chat/completions").trim_end_matches("/v1");
    let url = format!("{}/user/balance", stripped);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build().map_err(|e| format!("http client: {e}"))?;
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", key))
        .send().await.map_err(|e| format!("request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error: {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;
    Ok(body)
}

#[tauri::command]
fn load_config() -> Result<serde_json::Value, String> {
    let store = dsx_types::ConfigStore::default_location();
    let cfg = store.load().unwrap_or_default();
    let pid = cfg.provider_id.clone().unwrap_or_else(|| "deepseek".into());
    let ep = cfg.endpoint.clone().unwrap_or_else(|| "openai".into());
    let protocol = dsx_agent::gate::registry::protocol_for(&pid, &ep);
    Ok(serde_json::json!({
        "api_key": cfg.api_key,
        "base_url": cfg.base_url,
        "model": cfg.model,
        "context_limit": cfg.context_limit,
        "max_tokens": cfg.max_tokens,
        "provider_id": cfg.provider_id,
        "protocol": protocol,
        "endpoint": cfg.endpoint,
        "reasoning_effort": cfg.reasoning_effort,
        "lang": cfg.lang,
        "context7_api_key": cfg.context7_api_key,
    }))
}

#[tauri::command]
fn update_config(state: tauri::State<AgentState>, field: String, value: String) -> Result<(), String> {
    let _lock = state.config_lock.lock().map_err(|e| format!("lock: {e}"))?;
    let store = dsx_types::ConfigStore::default_location();
    let mut cfg = store.load().unwrap_or_default();
    match field.as_str() {
        "api_key" => cfg.api_key = Some(value),
        "base_url" => cfg.base_url = Some(value),
        "model" => cfg.model = Some(value),
        "reasoning_effort" => cfg.reasoning_effort = Some(value),
        "lang" => cfg.lang = Some(value),
        "context7_api_key" => cfg.context7_api_key = Some(value),
        "provider_id" => cfg.provider_id = Some(value),
        "endpoint" => cfg.endpoint = Some(value),
        _ => {
            if let Ok(n) = value.parse::<u32>() {
                match field.as_str() {
                    "context_limit" => cfg.context_limit = Some(n),
                    "max_tokens" => cfg.max_tokens = Some(n),
                    _ => return Err(format!("Unknown config field: {field}")),
                }
            } else if value == "true" || value == "false" {
                return Err(format!("Field '{field}' is not a boolean"));
            } else {
                return Err(format!("Unknown config field: {field}"));
            }
        }
    }
    if !store.save(&cfg) { return Err("Failed to save config".into()); }
    Ok(())
}

#[tauri::command]
fn cmd_sessions() -> Result<Vec<serde_json::Value>, String> {
    Ok(scan_sessions())
}

fn restart_agent(app: &AppHandle, state: &AgentState, seed: Option<&str>) -> Result<(), String> {
    let _op = state.op_lock.lock().map_err(|e| format!("lock: {e}"))?;
    restart_agent_inner(app, state, seed)
}

fn restart_agent_inner(app: &AppHandle, state: &AgentState, seed: Option<&str>) -> Result<(), String> {
    stop_agent_inner(app, state);

    let mut agent = dsx_agent::agent::AgentState::init("tauri");
    agent.session.resume_seed = seed.map(String::from);

    let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    let app_handle = app.clone();
    let rdr_handle = thread::spawn(move || {
        while let Ok(frame) = agent_rx.recv() {
            match &frame {
                Agent2Ui::SessionRestored { ref seed, .. } | Agent2Ui::SessionCreated { ref seed } => {
                    if let Ok(mut guard) = app_handle.state::<AgentState>().session_seed.lock() {
                        *guard = Some(seed.clone());
                    }
                }
                _ => {}
            }
            if let Ok(v) = serde_json::to_value(&frame) {
                let _ = app_handle.emit("agent-event", v);
            }
        }
    });

    let hdl = thread::spawn(move || {
        dsx_agent::runner::run_agent_loop(agent, tui_rx, agent_tx);
        dsx_agent::tools::shutdown_tools();
    });

    *state.tui_tx.lock().map_err(|e| format!("lock: {e}"))? = Some(tui_tx);
    *state.agent_handle.lock().map_err(|e| format!("lock: {e}"))? = Some(hdl);
    *state.reader_handle.lock().map_err(|e| format!("lock: {e}"))? = Some(rdr_handle);
    *state.session_seed.lock().map_err(|e| format!("lock: {e}"))? = seed.map(|s| s.to_string());
    Ok(())
}

#[tauri::command]
fn stop_agent(app: AppHandle, state: tauri::State<AgentState>) -> Result<(), String> {
    stop_agent_inner(&app, &state);
    Ok(())
}

#[tauri::command]
fn resume_agent(app: AppHandle, state: tauri::State<AgentState>, seed: String) -> Result<(), String> {
    restart_agent(&app, &state, Some(&seed))
}

#[tauri::command]
fn create_session(state: tauri::State<AgentState>) -> Result<(), String> {
    let guard = state.tui_tx.lock().map_err(|e| format!("lock: {e}"))?;
    let tx = guard.as_ref().ok_or("Agent not started")?;
    tx.send(Ui2Agent::CreateSession).map_err(|e| format!("send: {e}"))?;
    Ok(())
}

#[tauri::command]
fn list_providers() -> Result<serde_json::Value, String> {
    let providers = dsx_agent::gate::registry::all_providers();
    let json: Vec<serde_json::Value> = providers.iter().map(|p| {
        serde_json::json!({
            "id": p.id,
            "display": p.display,
            "endpoints": p.endpoints.iter().map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "display": e.display,
                    "protocol": e.protocol,
                    "base_url": e.base_url,
                    "default_model": e.default_model,
                    "models": e.models,
                })
            }).collect::<Vec<_>>(),
        })
    }).collect();
    Ok(serde_json::json!(json))
}

#[tauri::command]
fn set_workspace(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("Path does not exist".to_string());
    }
    let f = dsx_types::platform::workspace_path();
    std::fs::write(&f, &path).map_err(|e| format!("write: {e}"))
}

#[tauri::command]
fn get_workspace() -> Result<String, String> {
    let f = dsx_types::platform::workspace_path();
    let path = std::fs::read_to_string(&f).map_err(|_| "No workspace set".to_string())?;
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Workspace no longer exists: {path}"));
    }
    Ok(path)
}

#[tauri::command]
fn scan_directory(path: String) -> Result<serde_json::Value, String> {
    let dir = std::path::Path::new(&path);
    if !dir.is_dir() {
        return Err("Not a directory".to_string());
    }
    let mut entries = Vec::new();
    let read = std::fs::read_dir(dir).map_err(|e| format!("read_dir: {e}"))?;
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        entries.push(serde_json::json!({"name": name, "is_dir": is_dir, "size": size}));
    }
    entries.sort_by(|a, b| {
        let a_dir = a["is_dir"].as_bool().unwrap_or(false);
        let b_dir = b["is_dir"].as_bool().unwrap_or(false);
        b_dir.cmp(&a_dir).then(a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
    });
    Ok(serde_json::json!({"path": path, "entries": entries}))
}

#[tauri::command]
fn cancel_agent(state: tauri::State<AgentState>) -> Result<(), String> {
    dsx_tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    dsx_agent::tools::cancel_current_tool();
    let guard = state.tui_tx.lock().map_err(|e| format!("lock: {e}"))?;
    let tx = guard.as_ref().ok_or("Agent not started")?;
    tx.send(Ui2Agent::Cancel).map_err(|e| format!("send: {e}"))?;
    Ok(())
}

#[tauri::command]
fn delete_session(seed: String) -> Result<(), String> {
    let dir = data_dir().join("sessions");
    if !dir.is_dir() { return Ok(()); }
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("read_dir: {e}"))?.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        let is_match = if entry.path().is_dir() {
            // New format: sessions/{seed}-{date}/
            fname.starts_with(&format!("{}-", seed))
        } else {
            // Old format: sessions/{seed}.{toml|json} or {seed}.live.{toml|json}
            fname == format!("{seed}.toml") || fname == format!("{seed}.json")
                || fname == format!("{seed}.live.toml") || fname == format!("{seed}.live.json")
        };
        if is_match {
            if entry.path().is_dir() {
                std::fs::remove_dir_all(&entry.path()).map_err(|e| format!("delete_dir: {e}"))?;
            } else {
                std::fs::remove_file(&entry.path()).map_err(|e| format!("delete_file: {e}"))?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn delete_all_sessions(app: AppHandle, state: tauri::State<AgentState>) -> Result<(), String> {
    let _op = state.op_lock.lock().map_err(|e| format!("lock: {e}"))?;
    let dir = data_dir().join("sessions");
    if !dir.is_dir() { return Ok(()); }
    stop_agent_inner(&app, &state);
    let mut errors = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("read_dir: {e}"))?.flatten() {
        let path = entry.path();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result {
            errors.push(format!("{}: {e}", path.display()));
        }
    }
    restart_agent_inner(&app, &state, None)?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("Partial errors: {}", errors.join("; ")))
    }
}

fn stop_agent_inner(_app: &AppHandle, state: &AgentState) {
    // Send Shutdown to agent thread
    if let Ok(guard) = state.tui_tx.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(Ui2Agent::Shutdown);
        }
    }
    // Drop sender so agent loop exits
    if let Ok(mut guard) = state.tui_tx.lock() {
        *guard = None;
    }
    // Spawn background cleanup to avoid blocking the caller
    let agent_hdl = state.agent_handle.lock().ok().and_then(|mut g| g.take());
    let reader_hdl = state.reader_handle.lock().ok().and_then(|mut g| g.take());
    std::thread::spawn(move || {
        if let Some(hdl) = agent_hdl {
            let _ = hdl.join();
        }
        if let Some(hdl) = reader_hdl {
            let _ = hdl.join();
        }
    });
    if let Ok(mut seed) = state.session_seed.lock() {
        *seed = None;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AgentState { tui_tx: Mutex::new(None), agent_handle: Mutex::new(None), reader_handle: Mutex::new(None), op_lock: Mutex::new(()), config_lock: Mutex::new(()), session_seed: Mutex::new(None) })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info).build())?;
            }
            app.handle().plugin(tauri_plugin_dialog::init())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_config, save_config, load_config, update_config, list_providers,
            start_agent, check_agent_status, send_message, reload_agent, stop_agent, resume_agent, create_session,
            load_session_messages,
            set_workspace, get_workspace, scan_directory,
            cancel_agent,
            cmd_sessions, delete_session, delete_all_sessions,
            get_balance,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
