//! Agent bridge: runs the deepx-agent in a background thread and bridges
//! mpsc channels to Tauri commands (frontend → agent) and events (agent → frontend).
//!
//! Architecture mirrors deepx-tui's spawn_agent_inproc:
//!   1. AgentState::init("tauri") — init tools, config, tool defs
//!   2. Spawn run_agent_loop in a background thread
//!   3. Forward Ui2Agent frames from Tauri commands via mpsc sender
//!   4. Forward Agent2Ui frames to the frontend as Tauri window events

use std::sync::mpsc;
use std::sync::Mutex;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

use deepx_proto::{Agent2Ui, Ui2Agent};

/// Managed Tauri state that owns the sender side of the agent channel.
pub struct AgentBridge {
    sender: mpsc::Sender<Ui2Agent>,
    /// Whether the agent has been shut down.
    shutdown: Mutex<bool>,
}

/// Global BRIDGE â bypasses Tauri state management issues.
static BRIDGE: OnceLock<AgentBridge> = OnceLock::new();

/// Send a frame to the agent using the global BRIDGE.
fn send_command(frame: Ui2Agent) -> Result<(), String> {
    BRIDGE.get()
        .ok_or_else(|| "AgentBridge not initialized".into())
        .and_then(|bridge| bridge.send(frame))
}

impl AgentBridge {
    /// Initialize the agent and start the event-forwarding loop.
    /// Called once during Tauri app setup.
    pub fn init(app: &AppHandle) -> Self {
        deepx_session::SessionManager::init(deepx_types::platform::data_dir());
    let mut agent = deepx_msglp::agent::AgentState::init("tauri");

        // Init session manager + check for resume seed in the session directory
        if let Some(seed) = active_or_latest_seed() {
            agent.session.resume_seed = Some(seed);
        }

        let (tx, rx) = mpsc::channel::<Ui2Agent>();
        let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

        // Spawn the agent event loop in a background thread
        let agent_handle = std::thread::spawn(move || {
            deepx_msglp::Loop::new(agent, rx, agent_tx).run();
            deepx_tools::bridge::shutdown_tools();
        });

        // Forward Agent2Ui events to the Tauri frontend
        let app_handle = app.clone();
        std::thread::spawn(move || {
            while let Ok(event) = agent_rx.recv() {
                let event_type = agent2ui_event_name(&event);
                eprintln!("[BRIDGE] got event: {}", event_type);
                let payload = serde_json::to_value(&event).unwrap_or_default();
                if app_handle.emit("agent-event", payload.clone()).is_err() {
                    break;
                }
                // Also emit a typed event for the frontend to filter on
                let _ = app_handle.emit(&format!("agent-{}", event_type), payload);
            }
            // Agent thread will finish; drop the join handle
            drop(agent_handle);
        });

        let bridge = Self {
            sender: tx,
            shutdown: Mutex::new(false),
        };
        let _ = BRIDGE.set(bridge);
        Self { sender: mpsc::channel::<Ui2Agent>().0, shutdown: Mutex::new(false) }
    }

    /// Send a frame to the agent. Returns Ok(()) or an error string.
    fn send(&self, frame: Ui2Agent) -> Result<(), String> {
        if *self.shutdown.lock().unwrap() {
            return Err("Agent is shut down".into());
        }
        self.sender.send(frame).map_err(|e| format!("send error: {e}"))
    }

    /// Shut down the agent gracefully.
    pub fn shutdown(&self) {
        let mut s = self.shutdown.lock().unwrap();
        if !*s {
            *s = true;
            let _ = self.sender.send(Ui2Agent::Shutdown);
        }
    }
}

// ── Tauri Commands ──

/// Send a user text message to the agent.
#[tauri::command]
pub fn cmd_send_message(
    text: String,
) -> Result<(), String> {
    eprintln!("[BRIDGE] cmd_send_message: {}", &text[..text.floor_char_boundary(50)]);
    send_command(Ui2Agent::UserInput { text })
}

/// Create a new session.
#[tauri::command]
pub fn cmd_create_session(
) -> Result<(), String> {
    eprintln!("[BRIDGE] cmd_create_session");
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
    eprintln!("[BRIDGE] cmd_load_config called");
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
        Agent2Ui::SessionCreated { .. } => "session_created",
        Agent2Ui::Error { .. } => "error",
        Agent2Ui::ToolNotice { .. } => "tool_notice",
        Agent2Ui::Balance { .. } => "balance",
        Agent2Ui::Dashboard { .. } => "dashboard",
        Agent2Ui::Done => "done",
        Agent2Ui::Cancelled => "cancelled",
        Agent2Ui::ShutdownAck => "shutdown_ack",
        Agent2Ui::AuditRecord { .. } => "audit_record",
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
/// Read .active_session file if present, else fall back to latest from index.
fn active_or_latest_seed() -> Option<String> {
    if let Some(seed) = deepx_session::SessionManager::global().active_seed() {
        return Some(seed);
    }
    let dir = deepx_types::platform::sessions_dir();
    let index_path = dir.join("index.toml");
    let data = std::fs::read_to_string(&index_path).ok()?;
    if let Ok(metas) = toml::from_str::<Vec<deepx_types::SessionMeta>>(&data) {
        metas.into_iter()
            .max_by_key(|m| m.updated_at)
            .map(|m| m.seed)
    } else {
        None
    }
}
