use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

struct AgentState {
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
}

fn data_dir() -> std::path::PathBuf {
    dsx_types::platform::data_dir()
}

fn config_path() -> std::path::PathBuf {
    dsx_types::platform::config_path()
}

fn find_dsx() -> Result<String, String> {
    if let Ok(path) = std::env::var("DSX_BIN") {
        if std::path::Path::new(&path).exists() { return Ok(path); }
    }

    let name = if cfg!(target_os = "windows") { "dsx.exe" } else { "dsx" };

    // Try from executable path and from CWD
    for start_dir in [std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())),
                      std::env::current_dir().ok()].iter().flatten()
    {
        let candidate = start_dir.join(name);
        if candidate.exists() { return Ok(candidate.to_string_lossy().to_string()); }

        let resource = start_dir.join("resources").join(name);
        if resource.exists() { return Ok(resource.to_string_lossy().to_string()); }

        for ancestor in start_dir.ancestors().take(8) {
            for sub in &["debug", "release"] {
                let c = ancestor.join("target").join(sub).join(name);
                if c.exists() { return Ok(c.to_string_lossy().to_string()); }
            }
        }
    }

    #[cfg(unix)]
    if let Ok(out) = Command::new("sh").args(["which", "dsx"]).output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() { return Ok(p); }
        }
    }

    Err("dsx binary not found. Try: cargo build --release -p dsx".to_string())
}

fn ensure_hp(dsx_path: &str) -> Result<(), String> {
    let port_path = dsx_types::platform::hp_port_path();

    if let Ok(s) = std::fs::read_to_string(&port_path) {
        if let Ok(port) = s.trim().parse::<u16>() {
            if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                log::info!("hp already running on port {port}");
                return Ok(());
            }
            let _ = std::fs::write(&port_path, "");
        }
    }

    log::info!("starting dsx-hp...");
    let _ = std::fs::write(&port_path, "");
    let mut child = Command::new(dsx_path)
        .arg("hp")
        .stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("spawn hp: {e}"))?;

    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(s) = std::fs::read_to_string(&port_path) {
            if let Ok(port) = s.trim().parse::<u16>() {
                if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                    log::info!("hp started on port {port}");
                    return Ok(());
                }
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("dsx hp exited early with status {status}"));
        }
    }
    Err("HP failed to start. Run 'dsx hp' manually.".to_string())
}

fn spawn_agent(dsx_path: &str, resume_seed: Option<&str>) -> Result<(Box<dyn Write + Send>, BufReader<Box<dyn Read + Send>>, Box<dyn Read + Send>), String> {
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
    Ok((stdin, reader, Box::new(stderr)))
}

fn start_reader(reader: BufReader<Box<dyn Read + Send>>, app: AppHandle) {
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
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
                        let kind = v["type"].as_str().unwrap_or("").to_string();
                        match kind.as_str() {
                            "content_delta" => { let _ = app.emit("content-delta", v); }
                            "tool_progress" => { let _ = app.emit("tool-progress", v); }
                            "api_response" => { let _ = app.emit("api-response", v); }
                            "done" => { let _ = app.emit("agent-done", v); }
                            "error" => { let _ = app.emit("agent-error", v); }
                            "ask_user" => { let _ = app.emit("ask-user", v); }
                            "tool_state" => { let _ = app.emit("tool-state", v); }
                            "tool_result" => { let _ = app.emit("tool-result", v); }
                            "session_restored" => { let _ = app.emit("session-restored", v); }
                            "cache_prediction" => { let _ = app.emit("cache-prediction", v); }
                            "balance" => { let _ = app.emit("balance", v); }
                            _ => {}
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn start_stderr_reader(stderr: Box<dyn Read + Send>, app: AppHandle) {
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
                        if t.contains("refused") || t.contains("error") || t.contains("Error") {
                            let _ = app.emit("agent-error", serde_json::json!({"message": t}));
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

#[tauri::command]
fn send_message(state: tauri::State<AgentState>, text: String) -> Result<(), String> {
    let mut guard = state.stdin.lock().map_err(|e| format!("lock: {e}"))?;
    let writer = guard.as_mut().ok_or("Agent not started")?;
    let frame = serde_json::json!({"type": "user_input", "text": text});
    writeln!(writer, "{}", serde_json::to_string(&frame).unwrap())
        .map_err(|e| format!("write: {e}"))?;
    writer.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

#[tauri::command]
fn reload_agent(state: tauri::State<AgentState>) -> Result<(), String> {
    let mut guard = state.stdin.lock().map_err(|e| format!("lock: {e}"))?;
    let writer = guard.as_mut().ok_or("Agent not started")?;
    let frame = serde_json::json!({"type": "reload_config"});
    writeln!(writer, "{}", serde_json::to_string(&frame).unwrap())
        .map_err(|e| format!("write: {e}"))?;
    writer.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

/// Scan sessions/ dir for both flat *.json and subdirectory session.json files.
fn scan_sessions() -> Vec<serde_json::Value> {
    let dir = data_dir().join("sessions");
    let mut sessions = Vec::new();
    if !dir.is_dir() { return sessions; }
    // Non-session files to skip
    let skip = |name: &str| name == "index.json" || name == "pitfalls.json";
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();
            if skip(&fname) { continue; }
            // Old format: sessions/{seed}.json
            if path.is_file() && path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&data) {
                        // Compute message_count from messages array
                        if meta.get("message_count").is_none() {
                            let count = meta["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                            meta["message_count"] = serde_json::json!(count);
                        }
                        sessions.push(meta);
                        continue;
                    }
                }
            }
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
            if fname.starts_with(&seed) && path.is_dir() {
                let inner = path.join("session.json");
                if let Ok(data) = std::fs::read_to_string(&inner) {
                    file_data = Some(data);
                    break;
                }
            }
            // Old format: sessions/{seed}.json
            if fname == format!("{}.json", seed) && path.is_file() {
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
                                "args": serde_json::to_string(input).unwrap_or_default(),
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
                    "tool_calls": if tool_calls.is_empty() { serde_json::Value::Null } else { serde_json::json!(tool_calls) },
                }));
            }
            "tool" => {
                for block in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content } = block {
                        if let Some(last) = ui.last_mut() {
                            if last["role"] == "assistant" {
                                if let Some(tcs) = last["tool_calls"].as_array_mut() {
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
    let dsx_path = find_dsx()?;
    log::info!("dsx binary: {dsx_path}");
    ensure_hp(&dsx_path)?;
    let (stdin, reader, stderr) = spawn_agent(&dsx_path, None)?;
    *state.stdin.lock().map_err(|e| format!("lock: {e}"))? = Some(stdin);
    start_reader(reader, app.clone());
    start_stderr_reader(stderr, app);

    let sessions = scan_sessions();
    log::info!("agent connected");
    Ok(serde_json::json!({"ok": true, "sessions": sessions}))
}

#[tauri::command]
fn check_config() -> Result<bool, String> {
    Ok(config_path().exists())
}

#[tauri::command]
fn save_config(api_key: String, base_url: String, model: String, context_limit: u32, max_tokens: u32, effort: String, lang: String) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))?; }
    let mut old_cfg = serde_json::json!({});
    if let Ok(data) = std::fs::read_to_string(&p) {
        if let Ok(old) = serde_json::from_str::<serde_json::Value>(&data) { old_cfg = old; }
    }
    let final_model = if model.is_empty() {
        old_cfg.get("model").and_then(|m| m.as_str()).unwrap_or("deepseek-v4-flash").to_string()
    } else { model };
    let mut c = serde_json::json!({
        "api_key": api_key, "base_url": base_url,
        "model": final_model, "context_limit": context_limit, "max_tokens": max_tokens,
        "effort": effort, "lang": lang,
    });
    for key in &["profiles", "active_profile", "max_tool_rounds", "context7_api_key"] {
        if let Some(v) = old_cfg.get(*key) { c[*key] = v.clone(); }
    }
    std::fs::write(&p, serde_json::to_string_pretty(&c).unwrap()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn fetch_models(api_key: String, base_url: String) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build().map_err(|e| format!("http client: {e}"))?;

    // Strip /chat/completions or trailing /v1, then append /models
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
        return Err("API Key 无效".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build().map_err(|e| format!("http client: {e}"))?;
    let resp = client.get("https://api.deepseek.com/user/balance")
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
    let data = std::fs::read_to_string(&config_path()).map_err(|e| format!("read: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse: {e}"))
}

#[tauri::command]
fn update_config(field: String, value: String) -> Result<(), String> {
    let p = config_path();
    let mut cfg: serde_json::Value = std::fs::read_to_string(&p).ok()
        .and_then(|d| serde_json::from_str(&d).ok()).unwrap_or(serde_json::json!({}));
    if let Some(obj) = cfg.as_object_mut() {
        // Try JSON parse first (handles arrays, objects from frontend)
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
            obj.insert(field, parsed);
        } else if let Ok(n) = value.parse::<u32>() { obj.insert(field, serde_json::json!(n)); }
        else if value == "true" { obj.insert(field, serde_json::json!(true)); }
        else if value == "false" { obj.insert(field, serde_json::json!(false)); }
        else { obj.insert(field, serde_json::json!(value)); }
    }
    std::fs::write(&p, serde_json::to_string_pretty(&cfg).unwrap()).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

#[tauri::command]
// Session listing is included in start_agent return value
fn cmd_sessions() -> Result<Vec<serde_json::Value>, String> {
    Ok(scan_sessions())
}

fn restart_agent(app: &AppHandle, state: &AgentState, seed: Option<&str>) -> Result<(), String> {
    // Politely ask the old agent to exit before dropping stdin
    if let Ok(mut guard) = state.stdin.lock() {
        if let Some(writer) = guard.as_mut() {
            let frame = serde_json::json!({"type": "shutdown"});
            let _ = writeln!(writer, "{}", serde_json::to_string(&frame).unwrap());
            let _ = writer.flush();
        }
        *guard = None;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    let dsx_path = find_dsx()?;
    ensure_hp(&dsx_path)?;
    let (stdin, reader, stderr) = spawn_agent(&dsx_path, seed)?;
    *state.stdin.lock().map_err(|e| format!("lock: {e}"))? = Some(stdin);
    start_reader(reader, app.clone());
    start_stderr_reader(stderr, app.clone());
    Ok(())
}

#[tauri::command]
fn stop_agent(app: AppHandle, state: tauri::State<AgentState>) -> Result<(), String> {
    restart_agent(&app, &state, None)
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
    std::fs::read_to_string(&f).map_err(|_| "No workspace set".to_string())
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
    if let Some(writer) = guard.as_mut() {
        let frame = serde_json::json!({"type": "cancel"});
        writeln!(writer, "{}", serde_json::to_string(&frame).unwrap())
            .map_err(|e| format!("write: {e}"))?;
        writer.flush().map_err(|e| format!("flush: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
fn delete_session(seed: String) -> Result<(), String> {
    let dir = data_dir().join("sessions");
    if !dir.is_dir() { return Ok(()); }
    // Sessions are stored as {seed}-{date}/ directories or {seed}.live.json files
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("read_dir: {e}"))?.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.starts_with(&seed) {
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
    let dir = data_dir().join("sessions");
    if !dir.is_dir() { return Ok(()); }
    // Stop agent so it doesn't re-create sessions while we delete
    stop_agent_inner(&app, &state);
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("read_dir: {e}"))?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| format!("rm_dir {}: {e}", path.display()))?;
        } else {
            std::fs::remove_file(&path).map_err(|e| format!("rm_file {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn stop_agent_inner(_app: &AppHandle, state: &AgentState) {
    if let Ok(mut guard) = state.stdin.lock() {
        if let Some(writer) = guard.as_mut() {
            let frame = serde_json::json!({"type": "shutdown"});
            let _ = writeln!(writer, "{}", serde_json::to_string(&frame).unwrap());
            let _ = writer.flush();
        }
        *guard = None;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[tauri::command]
fn list_plans() -> Result<Vec<serde_json::Value>, String> {
    let plans_dir = dsx_types::platform::plans_dir();
    if !plans_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut plans = Vec::new();
    for entry in std::fs::read_dir(&plans_dir).map_err(|e| format!("read_dir: {e}"))?.flatten() {
        if entry.path().extension().map(|e| e == "md").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let name = entry.path().file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let status = if content.contains("status: done") { "done" }
                    else if content.contains("status: active") { "active" }
                    else if content.contains("status: cancelled") { "cancelled" }
                    else { "draft" };
                plans.push(serde_json::json!({
                    "name": name,
                    "status": status,
                    "summary": content.lines().find(|l| l.starts_with("## ")).unwrap_or("").to_string(),
                }));
            }
        }
    }
    plans.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(plans)
}

#[tauri::command]
fn read_plan(name: String) -> Result<String, String> {
    let path = dsx_types::platform::plans_dir()
        .join(format!("{}.md", name));
    std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AgentState { stdin: Mutex::new(None) })
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
            start_agent, send_message, reload_agent, stop_agent, resume_agent,
            load_session_messages,
            set_workspace, get_workspace, scan_directory,
            cancel_agent,
            cmd_sessions, delete_session, delete_all_sessions,
            list_plans, read_plan, get_balance,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
