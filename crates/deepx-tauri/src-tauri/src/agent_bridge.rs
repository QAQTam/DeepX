//! Manages the deepx-agent child process lifecycle and bridges stdin/stdout JSON-LP
//! communication between the Tauri frontend and the agent subprocess.
//!
//! Architecture:
//! - `AgentBridge::init()` spawns the same binary with the `agent` CLI flag as a child process,
//!   piping stdin/stdout.
//! - A background reader thread consumes JSON-LP lines from the child's stdout and emits them
//!   as Tauri events to the frontend.
//! - Tauri commands serialize `Ui2Agent` frames as JSON-LP lines written to the child's stdin.
//! - `shutdown()` sends a `Shutdown` frame, then kills and waits for the child process.

use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

use deepx_proto::{Agent2Ui, Ui2Agent};

/// Holds the child process handle and its stdin writer for the agent subprocess.
pub struct AgentBridge {
    stdin: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Option<std::process::Child>>,
    shutdown: Mutex<bool>,
}

/// Global singleton holding the real child process stdin handle, so Tauri commands can
/// write JSON-LP frames to the agent subprocess from any thread.
static BRIDGE: OnceLock<AgentBridge> = OnceLock::new();

fn send_command(frame: Ui2Agent) -> Result<(), String> {
    let bridge = BRIDGE.get().ok_or_else(|| String::from("AgentBridge not initialized"))?;
    log::info!("[BRIDGE] send_command dispatching to agent");
    bridge.send(&frame)
}

impl AgentBridge {
    /// Spawn the agent subprocess (same binary with `agent` flag), store its handle and
    /// stdin in the `BRIDGE` singleton, and start a background reader thread that parses
    /// JSON-LP events from the child's stdout and emits them as Tauri events.
    pub fn init(app: &AppHandle) -> Self {
        use std::process::{Command, Stdio};

        deepx_session::SessionManager::init(deepx_types::platform::data_dir());

        let exe = std::env::current_exe().expect("cannot get current exe path");
        let mut child_cmd = Command::new(&exe);
        child_cmd.arg("agent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()); // TODO: revert after debug
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            child_cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = child_cmd.spawn()
            .expect("Failed to spawn agent subprocess");

        let stdin = child.stdin.take().expect("Failed to get stdin");
        let stdout = child.stdout.take().expect("Failed to get stdout");
        let stderr = child.stderr.take().expect("Failed to get stderr");

        // Debug: pipe agent stderr to a log file
        std::thread::spawn(move || {
            let mut writer = std::fs::File::create(
                deepx_types::platform::data_dir().join("agent_debug.log")
            ).expect("Failed to create agent_debug.log");
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        use std::io::Write;
                        let _ = write!(writer, "{}", line);
                    }
                }
            }
        });

        let app_handle = app.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        log::info!("[BRIDGE] agent stdout read error: {e}");
                        break;
                    }
                };
                if line.trim().is_empty() {
                    continue;
                }
                let event: Agent2Ui = match serde_json::from_str(&line) {
                    Ok(e) => e,
                    Err(e) => {
                        log::info!("[BRIDGE] failed to parse: {} -- error: {e}", &line[..line.len().min(80)]);
                        continue;
                    }
                };
                let event_type = agent2ui_event_name(&event);
                log::info!("[BRIDGE] got event: {}", event_type);
                let payload = serde_json::to_value(&event).unwrap_or_default();
                if app_handle.emit("agent-event", payload.clone()).is_err() {
                    break;
                }
                let _ = app_handle.emit(&format!("agent-{}", event_type), payload);
            }
            log::info!("[BRIDGE] agent stdout reader thread exiting");
        });

        let bridge = Self {
            stdin: Mutex::new(Box::new(stdin)),
            child: Mutex::new(Some(child)),
            shutdown: Mutex::new(false),
        };
        let _ = BRIDGE.set(bridge);

        Self {
            stdin: Mutex::new(Box::new(std::io::sink())),
            child: Mutex::new(None),
            shutdown: Mutex::new(false),
        }
    }

    /// Write a `Ui2Agent` frame as a JSON-LP line to the child process's stdin.
    fn send(&self, frame: &Ui2Agent) -> Result<(), String> {
        let json = serde_json::to_string(frame).map_err(|e| format!("serialize: {e}"))?;
        let mut stdin = self.stdin.lock().unwrap();
        writeln!(*stdin, "{}", json).map_err(|e| format!("write: {e}"))?;
        stdin.flush().map_err(|e| format!("flush: {e}"))
    }

    /// Send a `Shutdown` frame to the agent child process, wait 500ms for graceful
    /// teardown, then force-kill the process if it is still running.
    pub fn shutdown(&self) {
        {
            let mut s = self.shutdown.lock().unwrap();
            if *s {
                return;
            }
            *s = true;
        } // drop shutdown lock before calling send()
        let _ = self.send(&Ui2Agent::Shutdown);
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Public entry point for graceful agent shutdown. Called from the Tauri window-close handler.
pub fn shutdown_agent() {
    if let Some(bridge) = BRIDGE.get() {
        bridge.shutdown();
    }
}

// ── Tauri Commands ──

/// Send a user text message to the agent.
#[tauri::command]
pub fn cmd_send_message(
    text: String,
) -> Result<(), String> {
    log::info!("[BRIDGE] cmd_send_message: {}", &text[..text.floor_char_boundary(50)]);
    send_command(Ui2Agent::UserInput { text })
}

/// Create a new session.
#[tauri::command]
pub fn cmd_create_session(
) -> Result<(), String> {
    log::info!("[BRIDGE] cmd_create_session");
    send_command(Ui2Agent::CreateSession)
}

/// Cancel the current operation.
#[tauri::command]
pub fn cmd_cancel(
) -> Result<(), String> {
    send_command(Ui2Agent::Cancel)
}

/// Request a debug snapshot from the agent.
#[tauri::command]
pub fn cmd_get_debug_snapshot(
) -> Result<(), String> {
    send_command(Ui2Agent::DebugCommand { cmd: "snapshot".into() })
}

/// Save configuration and reload the agent.
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
) -> Result<(), String> {
    let mut cfg = deepx_config::Config::load().unwrap_or_default();
    if !api_key.is_empty() { cfg.api_key = api_key; }
    if !model.is_empty() { cfg.model = model; }
    if !base_url.is_empty() { cfg.base_url = base_url; }
    if !provider_id.is_empty() { cfg.provider_id = provider_id; }
    if !endpoint.is_empty() { cfg.endpoint = endpoint; }
    if max_tokens > 0 { cfg.max_tokens = max_tokens; }
    if context_limit > 0 { cfg.context_limit = context_limit; }
    if !reasoning_effort.is_empty() { cfg.reasoning_effort = reasoning_effort; }
    if !lang.is_empty() { cfg.lang = Some(lang); }
    cfg.save()?;
    send_command(Ui2Agent::ReloadConfig)
}

/// Load the current config and return it as JSON.
#[tauri::command]
pub fn cmd_load_config() -> Result<String, String> {
    let cfg = deepx_config::Config::load()
        .map_err(|e| format!("load config: {e}"))?;
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
        "providers": providers,
    });
    serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"))
}

// ── Helpers ──

/// Map an Agent2Ui variant to a short event name for typed frontend filtering.
fn agent2ui_event_name(event: &Agent2Ui) -> &'static str {
    match event {
        Agent2Ui::TurnStart { .. } => "turn_start",
        Agent2Ui::TurnEnd { .. } => "turn_end",
        Agent2Ui::RoundDelta { .. } => "round_delta",
        Agent2Ui::RoundComplete { .. } => "round_complete",
        Agent2Ui::ToolResults { .. } => "tool_results",
        Agent2Ui::ToolExecDelta { .. } => "tool_exec_delta",
        Agent2Ui::SessionRestored { .. } => "session_restored",
        Agent2Ui::MoreTurns { .. } => "more_turns",
        Agent2Ui::SessionCreated { .. } => "session_created",
        Agent2Ui::Error { .. } => "error",
        Agent2Ui::ToolNotice { .. } => "tool_notice",
        Agent2Ui::Balance { .. } => "balance",
        Agent2Ui::Dashboard { .. } => "dashboard",
        Agent2Ui::Done => "done",
        Agent2Ui::CompactStart { .. } => "compact_start",
        Agent2Ui::CompactEnd { .. } => "compact_end",
        Agent2Ui::Cancelled => "cancelled",
        Agent2Ui::ShutdownAck => "shutdown_ack",
        Agent2Ui::AuditRecord { .. } => "audit_record",
        Agent2Ui::Ready => "ready",
        _ => "unknown",
    }
}


/// List all sessions with metadata.
#[tauri::command]
pub fn cmd_list_sessions() -> Result<String, String> {
    let metas = deepx_session::SessionManager::global().list();
    serde_json::to_string(&metas).map_err(|e| format!("serialize: {e}"))
}

/// Load full session data (messages) by seed.
#[tauri::command]
pub fn cmd_load_session(seed: String) -> Result<String, String> {
    let session = deepx_session::SessionManager::global().load(&seed)
        .ok_or_else(|| format!("Session not found: {seed}"))?;
    serde_json::to_string(&session).map_err(|e| format!("serialize: {e}"))
}

/// Set the active session seed for next app restart.
#[tauri::command]
pub fn cmd_set_active_session(seed: String) -> Result<(), String> {
    if seed.is_empty() {
        deepx_session::SessionManager::global().clear_active();
    } else {
        deepx_session::SessionManager::global().set_active_seed(&seed);
    }
    Ok(())
}

/// Delete a session by seed.
#[tauri::command]
pub fn cmd_delete_session(seed: String) -> Result<(), String> {
    deepx_session::SessionManager::global().delete(&seed)
}

/// Undo a turn and all subsequent content.
#[tauri::command]
pub fn cmd_undo_turn(turn_id: String) -> Result<(), String> {
    send_command(Ui2Agent::UndoTurn { turn_id })
}

/// Compact conversation history (summarize old turns).
#[tauri::command]
pub fn cmd_compact() -> Result<(), String> {
    log::info!("[BRIDGE] cmd_compact called");
    send_command(Ui2Agent::Compact)
}

/// Resume a specific session by seed.
#[tauri::command]
pub fn cmd_resume_session(seed: String) -> Result<(), String> {
    log::info!("[BRIDGE] cmd_resume_session called, seed={seed}");
    deepx_session::SessionManager::global().set_active_seed(&seed);
    send_command(Ui2Agent::ResumeSession { seed })
}

/// Create a new session (clears active marker).
#[tauri::command]
pub fn cmd_new_session() -> Result<(), String> {
    deepx_session::SessionManager::global().clear_active();
    send_command(Ui2Agent::NewSession)
}

/// Load older turns from session history (paginated, 20 at a time before the given turn).
#[tauri::command]
pub fn cmd_load_more_turns(before_turn_id: String) -> Result<(), String> {
    send_command(Ui2Agent::LoadMoreTurns { before_turn_id, count: 20 })
}

/// Get the current session's workspace root path.
#[tauri::command]
pub fn cmd_get_workspace() -> Result<String, String> {
    let mgr = deepx_session::SessionManager::global();
    let seed = mgr.active_seed().unwrap_or_default();
    if seed.is_empty() { return Ok(String::new()); }
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    Ok(std::fs::read_to_string(dir.join("workspace.txt")).unwrap_or_default().trim().to_string())
}

/// Set the current session's workspace root path and notify the agent.
#[tauri::command]
pub fn cmd_set_workspace(path: String) -> Result<(), String> {
    let mgr = deepx_session::SessionManager::global();
    let seed = mgr.active_seed().unwrap_or_default();
    if seed.is_empty() { return Err("No active session".into()); }
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(dir.join("workspace.txt"), path.trim()).map_err(|e| format!("write: {e}"))?;
    // Tell agent to reload config (which includes workspace)
    send_command(Ui2Agent::ReloadConfig)
}
