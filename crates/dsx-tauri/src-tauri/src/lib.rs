use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use dsx_proto::Ui2Agent;

struct ProcessHandle {
    child: Child,
    reader_handle: JoinHandle<()>,
    stderr_handle: JoinHandle<()>,
}

struct AgentState {
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
    process: Mutex<Option<ProcessHandle>>,
    hp_child: Mutex<Option<Child>>,
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

fn find_dsx(app: &AppHandle) -> Result<String, String> {
    if let Ok(path) = std::env::var("DSX_BIN") {
        if std::path::Path::new(&path).exists() { return Ok(path); }
    }

    let name = if cfg!(target_os = "windows") { "dsx.exe" } else { "dsx" };

    let check = |p: std::path::PathBuf| -> Option<String> {
        if p.exists() { Some(p.to_string_lossy().to_string()) } else { None }
    };

    // ── debug/dev: prefer target/ over stale resources/ copy ──
    #[cfg(debug_assertions)]
    {
        if let Ok(cwd) = std::env::current_dir() {
            for ancestor in cwd.ancestors().take(8) {
                for sub in &["debug", "release"] {
                    if let Some(p) = check(ancestor.join("target").join(sub).join(name)) { return Ok(p); }
                }
            }
        }
    }

    // 1) Tauri resource dir (serves release .deb packaging)
    if let Ok(d) = app.path().resource_dir() {
        if let Some(p) = check(d.join(name)) { return Ok(p); }
    }

    let current_exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));

    // 2) next to dsx-tauri binary (e.g. /usr/bin/)
    if let Some(ref dir) = current_exe_dir {
        if let Some(p) = check(dir.join(name)) { return Ok(p); }
        // 2b) lib/<productName>/resources/ (deb structure: binary in /usr/bin, resources in /usr/lib/DSX/)
        for ancestor in dir.ancestors().take(3) {
            let lib = ancestor.join("lib").join("DSX").join(name);
            if let Some(p) = check(lib) { return Ok(p); }
            let lib_res = ancestor.join("lib").join("DeepX").join("resources").join(name);
            if let Some(p) = check(lib_res) { return Ok(p); }
        }
    }

    // 3) resources/ subdir next to executable
    if let Some(ref dir) = current_exe_dir {
        if let Some(p) = check(dir.join("resources").join(name)) { return Ok(p); }
    }

    // 4) cwd + ancestors (non-debug fallback)
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(p) = check(cwd.join(name)) { return Ok(p); }
        if let Some(p) = check(cwd.join("resources").join(name)) { return Ok(p); }
        #[cfg(not(debug_assertions))]
        for ancestor in cwd.ancestors().take(8) {
            for sub in &["debug", "release"] {
                if let Some(p) = check(ancestor.join("target").join(sub).join(name)) { return Ok(p); }
            }
        }
    }

    #[cfg(unix)]
    if let Ok(out) = Command::new("sh").args(["which", "dsx"]).output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                if let Some(p) = check(std::path::PathBuf::from(p)) { return Ok(p); }
            }
        }
    }

    Err("dsx binary not found. Try: cargo build --release -p dsx".to_string())
}

fn ensure_hp(dsx_path: &str, state: &AgentState) -> Result<(), String> {
    let mut guard = state.hp_child.lock().map_err(|e| format!("lock: {e}"))?;
    let port_path = dsx_types::platform::hp_port_path();

    if let Ok(s) = std::fs::read_to_string(&port_path) {
        if let Ok(port) = s.trim().parse::<u16>() {
            if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                log::info!("hp already running on port {port}");
                return Ok(());
            }
        }
    }

    log::info!("starting dsx-gate...");
    let _ = std::fs::write(&port_path, "");
    let mut child = Command::new(dsx_path)
        .arg("gate")
        .stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("spawn hp: {e}"))?;

    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(s) = std::fs::read_to_string(&port_path) {
            if let Ok(port) = s.trim().parse::<u16>() {
                if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                    log::info!("hp started on port {port}");
                    let old = guard.replace(child);
                    if let Some(mut old_child) = old {
                        let _ = old_child.kill();
                        let _ = old_child.wait();
                    }
                    return Ok(());
                }
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("dsx gate exited early with status {status}"));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("gate failed to start. Run 'dsx gate' manually.".to_string())
}

fn spawn_agent(dsx_path: &str, resume_seed: Option<&str>) -> Result<(Box<dyn Write + Send>, BufReader<Box<dyn Read + Send>>, Box<dyn Read + Send>, Child), String> {
    let mut cmd = Command::new(dsx_path);
    cmd.arg("agent");
    if let Some(seed) = resume_seed {
        cmd.arg("--session").arg(seed);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("spawn agent: {e}"))?;

    let stdin = Box::new(child.stdin.take().ok_or("agent: no stdin")?) as Box<dyn Write + Send>;
    let stdout = child.stdout.take().ok_or("agent: no stdout")?;
    let stderr = child.stderr.take().ok_or("agent: no stderr")?;
    let reader = BufReader::new(Box::new(stdout) as Box<dyn Read + Send>);
    Ok((stdin, reader, Box::new(stderr), child))
}

fn start_reader(reader: BufReader<Box<dyn Read + Send>>, app: AppHandle) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    log::info!("agent stdout closed");
                    let _ = app.emit("agent-closed", serde_json::json!({}));
                    break;
                }
                Ok(_) => {
                    let t = line.trim();
                    if t.is_empty() { continue; }
                    match serde_json::from_str::<serde_json::Value>(t) {
                        Ok(v) => {
                            let kind = v["type"].as_str().unwrap_or("");
                            match kind {
                                "stream_start" | "stream_delta" | "stream_end" |
                                "assistant_msg" | "user_msg" |
                                "tool_call" | "tool_result" |
                                "turn_end" | "done" | "error" | "cancelled" |
                                "ask_user" | "balance" | "session_restored" |
                                "debug_snapshot" | "shutdown_ack" |
                                "audit_record" => {
                                    let _ = app.emit("agent-event", v);
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            log::warn!("agent: non-JSON stdout line ({} chars): {}", t.len(), e);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn start_stderr_reader(stderr: Box<dyn Read + Send>, app: AppHandle) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let t = line.trim();
                    if !t.is_empty() {
                        log::warn!("agent stderr: {t}");
                        let lower = t.to_lowercase();
                        if lower.contains("refused") || lower.contains("connection reset")
                           || lower.contains("broken pipe") || lower.contains("timeout")
                           || lower.contains("panic") || lower.contains("fatal") {
                            let _ = app.emit("agent-error", serde_json::json!({"message": t}));
                        }
                    }
                }
                Err(_) => break,
            }
        }
    })
}

#[tauri::command]
fn send_message(state: tauri::State<AgentState>, text: String) -> Result<(), String> {
    let mut guard = state.stdin.lock().map_err(|e| format!("lock: {e}"))?;
    let writer = guard.as_mut().ok_or("Agent not started")?;
    let frame = Ui2Agent::UserInput { text };
    dsx_proto::write_frame(writer, &frame).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

#[tauri::command]
fn reload_agent(state: tauri::State<AgentState>) -> Result<(), String> {
    let mut guard = state.stdin.lock().map_err(|e| format!("lock: {e}"))?;
    let writer = guard.as_mut().ok_or("Agent not started")?;
    let frame = Ui2Agent::ReloadConfig;
    dsx_proto::write_frame(writer, &frame).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

fn scan_sessions() -> Vec<serde_json::Value> {
    let dir = data_dir().join("sessions");
    let mut sessions = Vec::new();
    if !dir.is_dir() { return sessions; }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();
            // New format: sessions/{seed}-{date}/session.json
            if path.is_dir() {
                let inner = path.join("session.json");
                if inner.is_file() {
                    if let Ok(data) = std::fs::read_to_string(&inner) {
                        if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&data) {
                            if meta.get("message_count").is_none() {
                                let count = meta["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                                meta["message_count"] = serde_json::json!(count);
                            }
                            sessions.push(meta);
                        }
                    }
                }
                continue;
            }
            // Old format: only {seed}.json or {seed}.live.json (skip index.json, pitfalls.json, etc.)
            if path.is_file() && path.extension().map(|e| e == "json").unwrap_or(false) {
                if fname == "index.json" || fname == "pitfalls.json" { continue; }
                let is_session = fname.ends_with(".live.json") || fname.ends_with(".json");
                if is_session {
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&data) {
                            if meta.get("message_count").is_none() {
                                let count = meta["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                                meta["message_count"] = serde_json::json!(count);
                            }
                            sessions.push(meta);
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
            // New format: sessions/{seed}-{date}/session.json
            if path.is_dir() && fname.starts_with(&format!("{}-", seed)) {
                let inner = path.join("session.json");
                if let Ok(data) = std::fs::read_to_string(&inner) {
                    file_data = Some(data);
                    break;
                }
            }
            // Old format: sessions/{seed}.json or sessions/{seed}.live.json
            if path.is_file() && (fname == format!("{}.json", seed) || fname == format!("{}.live.json", seed)) {
                file_data = std::fs::read_to_string(&path).ok();
                break;
            }
        }
    }

    let data = file_data.ok_or_else(|| format!("Session not found: {seed}"))?;
    let file: dsx_types::SessionFile = serde_json::from_str(&data)
        .map_err(|e| format!("Parse session: {e}"))?;
    Ok(session_to_frontend(&file))
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
    if state.stdin.lock().map_err(|e| format!("lock: {e}"))?.is_some() {
        return Err("Agent already started".to_string());
    }
    let dsx_path = find_dsx(&app)?;
    log::info!("dsx binary: {dsx_path}");
    ensure_hp(&dsx_path, &state)?;
    let (stdin, reader, stderr, child) = spawn_agent(&dsx_path, None)?;
    let reader_handle = start_reader(reader, app.clone());
    let stderr_handle = start_stderr_reader(stderr, app.clone());
    *state.stdin.lock().map_err(|e| format!("lock: {e}"))? = Some(stdin);
    *state.process.lock().map_err(|e| format!("lock: {e}"))? = Some(ProcessHandle {
        child,
        reader_handle,
        stderr_handle,
    });
    *state.session_seed.lock().map_err(|e| format!("lock: {e}"))? = None;

    let sessions = scan_sessions();
    log::info!("agent connected");
    Ok(serde_json::json!({"ok": true, "sessions": sessions}))
}

#[tauri::command]
fn check_agent_status(state: tauri::State<AgentState>) -> Result<serde_json::Value, String> {
    let running = state.stdin.lock().map_err(|e| format!("lock: {e}"))?.is_some();
    let seed = state.session_seed.lock().map_err(|e| format!("lock: {e}"))?.clone();
    Ok(serde_json::json!({"running": running, "seed": seed}))
}

#[tauri::command]
fn check_config() -> Result<bool, String> {
    Ok(config_path().exists())
}

#[tauri::command]
fn save_config(state: tauri::State<AgentState>, api_key: String, base_url: String, model: String, context_limit: u32, max_tokens: u32, effort: String, lang: String) -> Result<(), String> {
    let _lock = state.config_lock.lock().map_err(|e| format!("lock: {e}"))?;
    let p = config_path();
    if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))?; }
    let mut old_cfg = serde_json::json!({});
    if let Ok(data) = std::fs::read_to_string(&p) {
        if let Ok(old) = serde_json::from_str::<serde_json::Value>(&data) { old_cfg = old; }
    }
    let final_model = if model.is_empty() {
        old_cfg.get("model").and_then(|m| m.as_str()).unwrap_or("deepseek-v4-flash").to_string()
    } else { model };
    let mut c = old_cfg;
    c["api_key"] = serde_json::json!(api_key);
    c["base_url"] = serde_json::json!(base_url);
    c["model"] = serde_json::json!(final_model);
    c["context_limit"] = serde_json::json!(context_limit);
    c["max_tokens"] = serde_json::json!(max_tokens);
    c["effort"] = serde_json::json!(effort);
    c["lang"] = serde_json::json!(lang);
    let data = serde_json::to_string_pretty(&c).map_err(|e| format!("json: {e}"))?;
    std::fs::write(&p, data).map_err(|e| format!("write: {e}"))
}

#[tauri::command]
async fn fetch_models(api_key: String, base_url: String) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build().map_err(|e| format!("http client: {e}"))?;

    let stripped = base_url
        .trim_end_matches('/')
        .trim_end_matches("/chat/completions")
        .trim_end_matches("/v1");
    let url = format!("{}/models", stripped);

    let mut req = client.get(&url);
    let key = api_key.trim().trim_matches('"').to_string();
    if !key.is_empty() && key != "null" {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("{} returned {}: {}", url, status, body.chars().take(120).collect::<String>()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;
    let models: Vec<String> = body["data"].as_array()
        .map(|arr| arr.iter()
            .filter_map(|m| m["id"].as_str().map(String::from))
            .filter(|id| !id.starts_with("deepseek-re"))
            .collect())
        .unwrap_or_default();

    if models.is_empty() {
        log::warn!("No models returned from API, using hardcoded defaults");
        Ok(vec![
            "deepseek-v4-flash".into(),
            "deepseek-reasoner-v4".into(),
            "deepseek-chat".into(),
        ])
    } else {
        Ok(models)
    }
}

#[tauri::command]
async fn get_balance(api_key: String) -> Result<serde_json::Value, String> {
    let key = api_key.trim().trim_matches('"');
    if key.is_empty() || key == "null" {
        return Err("API key is invalid".to_string());
    }
    let base_url = std::fs::read_to_string(&config_path()).ok()
        .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
        .and_then(|c| c["base_url"].as_str().map(String::from))
        .unwrap_or_else(|| "https://api.deepseek.com".to_string());
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
    let path = config_path();
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let data = std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse: {e}"))
}

#[tauri::command]
fn update_config(state: tauri::State<AgentState>, field: String, value: String) -> Result<(), String> {
    let _lock = state.config_lock.lock().map_err(|e| format!("lock: {e}"))?;
    let p = config_path();
    let mut cfg: serde_json::Value = std::fs::read_to_string(&p).ok()
        .and_then(|d| serde_json::from_str(&d).ok()).unwrap_or(serde_json::json!({}));
    if let Some(obj) = cfg.as_object_mut() {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
            obj.insert(field, parsed);
        } else if let Ok(n) = value.parse::<u32>() { obj.insert(field, serde_json::json!(n)); }
        else if value == "true" { obj.insert(field, serde_json::json!(true)); }
        else if value == "false" { obj.insert(field, serde_json::json!(false)); }
        else { obj.insert(field, serde_json::json!(value)); }
    }
    let data = serde_json::to_string_pretty(&cfg).map_err(|e| format!("json: {e}"))?;
    std::fs::write(&p, data).map_err(|e| format!("write: {e}"))
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
    if let Ok(mut guard) = state.stdin.lock() {
        if let Some(writer) = guard.as_mut() {
            let frame = Ui2Agent::Shutdown;
            let _ = dsx_proto::write_frame(writer, &frame);
            let _ = writer.flush();
        }
        *guard = None;
    }
    if let Ok(mut proc) = state.process.lock() {
        if let Some(mut handle) = proc.take() {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
            let _ = handle.reader_handle.join();
            let _ = handle.stderr_handle.join();
        }
    }
    let dsx_path = find_dsx(app)?;
    ensure_hp(&dsx_path, state)?;
    let (stdin, reader, stderr, child) = spawn_agent(&dsx_path, seed)?;
    let reader_handle = start_reader(reader, app.clone());
    let stderr_handle = start_stderr_reader(stderr, app.clone());
    *state.stdin.lock().map_err(|e| format!("lock: {e}"))? = Some(stdin);
    *state.process.lock().map_err(|e| format!("lock: {e}"))? = Some(ProcessHandle {
        child,
        reader_handle,
        stderr_handle,
    });
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
    let mut guard = state.stdin.lock().map_err(|e| format!("lock: {e}"))?;
    let writer = guard.as_mut().ok_or("Agent not started")?;
    let frame = Ui2Agent::Cancel;
    dsx_proto::write_frame(writer, &frame).map_err(|e| format!("write: {e}"))?;
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
            // Old format: sessions/{seed}.json or sessions/{seed}.live.json
            fname == format!("{seed}.json") || fname == format!("{seed}.live.json")
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
    if let Ok(mut guard) = state.stdin.lock() {
        if let Some(writer) = guard.as_mut() {
            let frame = Ui2Agent::Shutdown;
            let _ = dsx_proto::write_frame(writer, &frame);
            let _ = writer.flush();
        }
        *guard = None;
    }
    if let Ok(mut proc) = state.process.lock() {
        if let Some(mut handle) = proc.take() {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
            let _ = handle.reader_handle.join();
            let _ = handle.stderr_handle.join();
        }
    }
    if let Ok(mut seed) = state.session_seed.lock() {
        *seed = None;
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AgentState { stdin: Mutex::new(None), process: Mutex::new(None), hp_child: Mutex::new(None), op_lock: Mutex::new(()), config_lock: Mutex::new(()), session_seed: Mutex::new(None) })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info).build())?;
            }
            app.handle().plugin(tauri_plugin_dialog::init())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_config, save_config, load_config, update_config, fetch_models,
            start_agent, check_agent_status, send_message, reload_agent, stop_agent, resume_agent,
            load_session_messages,
            set_workspace, get_workspace, scan_directory,
            cancel_agent,
            cmd_sessions, delete_session, delete_all_sessions,
            get_balance,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
