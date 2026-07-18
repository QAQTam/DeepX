//! Agent subprocess lifecycle management.
//!
//! AgentRegistry manages multiple agent child processes, one per session seed.
//! Each agent communicates via stdin/stdout JSON-framed protocol (Ui2Agent/Agent2Ui).

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use deepx_proto::{Agent2Ui, Ui2Agent};

use super::activity::SessionActivityTracker;
use super::platform::SYSTEM_PATH;
use super::util::agent2ui_event_name_for_ui;

const REPLAY_EVENT_LIMIT: usize = 128;
type ReplayCache = Arc<Mutex<VecDeque<serde_json::Value>>>;

fn new_replay_cache() -> ReplayCache {
    Arc::new(Mutex::new(VecDeque::new()))
}

fn is_replayable_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "turn_start"
            | "round_complete"
            | "tool_results"
            | "turn_end"
            | "done"
            | "cancelled"
            | "error"
            | "permission_request"
            | "ask_user"
            | "ask_resolved"
            | "ask_rejected"
            | "plan_submitted"
            | "plan_resolved"
    )
}

fn record_replay_event(cache: &ReplayCache, payload: &serde_json::Value) {
    let Some(event_type) = payload.get("type").and_then(|value| value.as_str()) else {
        return;
    };
    if !is_replayable_event(event_type) {
        return;
    }
    let Ok(mut events) = cache.lock() else {
        return;
    };
    if event_type == "turn_start" {
        events.clear();
    }
    while events.len() >= REPLAY_EVENT_LIMIT {
        events.pop_front();
    }
    events.push_back(payload.clone());
}

// ── Agent instance ──

pub struct AgentInstance {
    #[allow(dead_code)]
    seed: String,
    stdin: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    shutdown_flag: Arc<AtomicBool>,
    replay_events: ReplayCache,
}

/// Global registry of all running agent subprocesses, keyed by session seed.
static REGISTRY: OnceLock<Mutex<AgentRegistry>> = OnceLock::new();

// ═══════════════════════════════════════════════════════════════
// AgentRegistry
// ═══════════════════════════════════════════════════════════════

pub struct AgentRegistry {
    pub(crate) instances: HashMap<String, AgentInstance>,
    app_handle: AppHandle,
    activity_tracker: SessionActivityTracker,
}

impl std::fmt::Debug for AgentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRegistry")
            .field("instances", &self.instances.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_all_agents_without_init_does_not_panic() {
        // REGISTRY is not initialized in test — should be a clean no-op
        shutdown_all_agents();
    }

    #[test]
    fn test_agent_instance_fields_accessible() {
        // Verify the struct layout is intact after module split
        let stdin: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(std::io::sink())));
        let child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));

        let inst = AgentInstance {
            seed: "test_seed".into(),
            stdin,
            child,
            shutdown_flag: shutdown,
            replay_events: new_replay_cache(),
        };

        assert_eq!(inst.seed, "test_seed");
    }

    #[test]
    fn replay_cache_keeps_lifecycle_events_and_drops_deltas() {
        let cache = new_replay_cache();
        record_replay_event(
            &cache,
            &serde_json::json!({
                "type": "turn_start", "turn_id": "t1", "user_text": "reload"
            }),
        );
        record_replay_event(
            &cache,
            &serde_json::json!({
                "type": "round_delta", "turn_id": "t1", "delta": "partial"
            }),
        );
        record_replay_event(
            &cache,
            &serde_json::json!({
                "type": "round_complete", "turn_id": "t1", "round_num": 0,
                "thinking": null, "answer": "complete", "tool_calls": []
            }),
        );

        let events = cache.lock().unwrap();
        let types: Vec<_> = events
            .iter()
            .filter_map(|event| event.get("type").and_then(|value| value.as_str()))
            .collect();
        assert_eq!(types, vec!["turn_start", "round_complete"]);
    }

    #[test]
    fn a_new_turn_discards_the_previous_turn_replay() {
        let cache = new_replay_cache();
        record_replay_event(&cache, &serde_json::json!({ "type": "done" }));
        record_replay_event(
            &cache,
            &serde_json::json!({
                "type": "turn_start", "turn_id": "t2", "user_text": "next"
            }),
        );

        let events = cache.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "turn_start");
    }
}

/// Spawn an agent child process (free function, no lock needed).
/// Returns an AgentInstance ready for insertion into the registry.
fn spawn_agent_process(
    seed: &str,
    new_seed: Option<&str>,
    app_handle: &AppHandle,
    activity_tracker: SessionActivityTracker,
    activity_generation: u64,
) -> Result<AgentInstance, String> {
    use std::process::{Command, Stdio};

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut cmd = Command::new(&exe);
    cmd.arg("agent");
    if let Some(s) = new_seed {
        cmd.arg("--seed").arg(s);
    } else {
        cmd.arg("--resume-seed").arg(seed);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Inject cached full system PATH so child processes (pwsh via conpty)
    // can find git, cargo, etc. even when Tauri itself has a stripped PATH.
    if let Some(path) = SYSTEM_PATH.get() {
        cmd.env("PATH", path);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn agent for seed {seed}: {e}"))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to get stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to get stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to get stderr".to_string())?;

    // Debug: pipe agent stderr to a per-seed log file
    let debug_seed = seed.to_string();
    std::thread::spawn(move || {
        let log_path = deepx_types::platform::data_dir().join(format!(
            "agent_{}_debug.log",
            &debug_seed[..debug_seed.floor_char_boundary(debug_seed.len().min(8))]
        ));
        let mut writer = std::fs::File::create(&log_path).unwrap_or_else(|_| {
            std::fs::File::create(deepx_types::platform::data_dir().join("agent_debug.log"))
                .unwrap()
        });
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

    let app_handle = app_handle.clone();
    let seed_owned = seed.to_string();
    let child_for_check = Arc::new(Mutex::new(Some(child)));
    let child_for_thread = child_for_check.clone();
    let replay_events = new_replay_cache();
    let replay_for_reader = replay_events.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let event_label = format!("agent-{}-event", seed_owned);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    log::info!("[REGISTRY] agent {seed_owned} stdout read error: {e}");
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let payload: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    log::info!(
                        "[REGISTRY] failed to parse from {}: {} -- error: {e}",
                        &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))],
                        &line[..line.floor_char_boundary(line.len().min(80))]
                    );
                    continue;
                }
            };
            let event_type = payload
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            record_replay_event(&replay_for_reader, &payload);
            if let Some(activity) =
                activity_tracker.observe(&seed_owned, activity_generation, &payload)
            {
                let _ = app_handle.emit("session-activity", &activity);
            }
            if let Some(activity) = activity_tracker.current(&seed_owned, activity_generation) {
                super::companion_host::publish_agent_event(
                    &seed_owned,
                    activity_generation,
                    &payload,
                    &activity,
                );
            }
            if event_type != "round_delta"
                && event_type != "tool_call_preview"
                && event_type != "exec_progress"
            {
                log::info!(
                    "[REGISTRY] agent {} got event: {}",
                    &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))],
                    event_type
                );
            }
            if let Err(error) = app_handle.emit(&event_label, &payload)
                && event_type != "round_delta"
                && event_type != "tool_call_preview"
                && event_type != "exec_progress"
            {
                log::warn!(
                    "[REGISTRY] WebView emit failed for {seed_owned}; retaining agent stdout: {error}"
                );
            }
        }
        log::warn!(
            "[REGISTRY] agent {} stdout reader thread exiting",
            seed_owned
        );
        let exit_status = child_for_thread
            .lock()
            .ok()
            .and_then(|mut c| c.as_mut().and_then(|c| c.try_wait().ok()).flatten());
        log::warn!(
            "[REGISTRY] agent {} child exit status: {:?}",
            &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))],
            exit_status
        );
        let error_event = Agent2Ui::Error {
            message: format!(
                "Agent process for session {} has exited unexpectedly",
                &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))]
            ),
        };
        let payload = serde_json::to_value(&error_event).unwrap_or_default();
        record_replay_event(&replay_for_reader, &payload);
        let _ = app_handle.emit(&event_label, payload.clone());
        let _ = app_handle.emit("agent-event", payload);
        if let Some(activity) = activity_tracker.disconnect(&seed_owned, activity_generation) {
            let _ = app_handle.emit("session-activity", &activity);
            super::companion_host::publish_agent_event(
                &seed_owned,
                activity_generation,
                &serde_json::json!({ "type": "shutdown_ack" }),
                &activity,
            );
        }
    });

    let inst = AgentInstance {
        seed: seed.to_string(),
        stdin: Arc::new(Mutex::new(Box::new(stdin))),
        child: child_for_check,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
        replay_events,
    };
    inst.spawn_heartbeat();
    Ok(inst)
}

impl AgentRegistry {
    pub(crate) fn get() -> &'static Mutex<AgentRegistry> {
        REGISTRY
            .get()
            .expect("AgentRegistry not initialized — call init() first")
    }

    /// Initialize the global registry and SessionManager. Called once at startup.
    pub fn init(app: &AppHandle) {
        let turso_enabled = deepx_config::Config::load()
            .map(|c| c.turso_enabled())
            .unwrap_or(true);
        deepx_session::SessionManager::init(deepx_types::platform::data_dir(), turso_enabled);
        let registry = AgentRegistry {
            instances: HashMap::new(),
            app_handle: app.clone(),
            activity_tracker: SessionActivityTracker::default(),
        };
        REGISTRY
            .set(Mutex::new(registry))
            .expect("AgentRegistry already initialized");
    }

    pub fn get_or_spawn(&mut self, seed: &str) -> Result<(), String> {
        if self.instances.contains_key(seed) {
            return Ok(());
        }
        self.spawn_agent(seed, None)
    }

    pub fn spawn_new(&mut self, seed: &str) -> Result<(), String> {
        if self.instances.contains_key(seed) {
            return Err(format!("Agent for seed {} already exists", seed));
        }
        self.spawn_agent(seed, Some(seed))
    }

    fn spawn_agent(&mut self, seed: &str, new_seed: Option<&str>) -> Result<(), String> {
        let (generation, starting) = self.activity_tracker.begin(seed);
        let _ = self.app_handle.emit("session-activity", &starting);
        super::companion_host::publish_agent_event(
            seed,
            generation,
            &serde_json::json!({ "type": "starting" }),
            &starting,
        );
        let instance = match spawn_agent_process(
            seed,
            new_seed,
            &self.app_handle,
            self.activity_tracker.clone(),
            generation,
        ) {
            Ok(instance) => instance,
            Err(error) => {
                if let Some(activity) = self.activity_tracker.disconnect(seed, generation) {
                    let _ = self.app_handle.emit("session-activity", &activity);
                }
                return Err(error);
            }
        };
        self.instances.insert(seed.to_string(), instance);
        log::info!(
            "[REGISTRY] spawned agent for seed={}",
            &seed[..seed.floor_char_boundary(seed.len().min(8))]
        );
        Ok(())
    }

    pub fn send_to(&self, seed: &str, frame: &Ui2Agent) -> Result<(), String> {
        let stdin_arc = self
            .instances
            .get(seed)
            .ok_or_else(|| {
                format!(
                    "No agent running for seed: {}",
                    &seed[..seed.floor_char_boundary(seed.len().min(8))]
                )
            })?
            .stdin
            .clone();
        let json = serde_json::to_string(frame).map_err(|e| format!("serialize: {e}"))?;
        let mut stdin = stdin_arc.lock().map_err(|e| format!("lock: {e}"))?;
        writeln!(*stdin, "{}", json).map_err(|e| format!("write: {e}"))?;
        stdin.flush().map_err(|e| format!("flush: {e}"))
    }

    pub fn has_instance(&self, seed: &str) -> bool {
        self.instances.contains_key(seed)
    }

    pub fn session_activity(&self) -> Vec<deepx_proto::SessionActivity> {
        self.activity_tracker.snapshot()
    }

    pub fn replay_events(&self, seed: &str) -> Result<Vec<serde_json::Value>, String> {
        let instance = self
            .instances
            .get(seed)
            .ok_or_else(|| format!("No running agent for seed {seed}"))?;
        let events = instance
            .replay_events
            .lock()
            .map_err(|error| format!("replay cache lock: {error}"))?;
        Ok(events.iter().cloned().collect())
    }

    pub fn kill_agent(&mut self, seed: &str) -> Option<AgentInstance> {
        let instance = self.instances.remove(seed);
        if instance.is_some() {
            log::info!(
                "[REGISTRY] removed agent for seed={}",
                &seed[..seed.floor_char_boundary(seed.len().min(8))]
            );
        }
        instance
    }

    pub fn shutdown_all(&mut self) {
        let drained: Vec<AgentInstance> = self.instances.drain().map(|(_, v)| v).collect();
        for inst in drained {
            inst.shutdown_and_wait();
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// AgentInstance
// ═══════════════════════════════════════════════════════════════

impl AgentInstance {
    fn spawn_heartbeat(&self) {
        let seed = self.seed.clone();
        let child = self.child.clone();
        let shutdown = self.shutdown_flag.clone();
        std::thread::Builder::new()
            .name(format!(
                "hb-{}",
                &seed[..seed.floor_char_boundary(seed.len().min(8))]
            ))
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    let is_dead = child
                        .lock()
                        .ok()
                        .and_then(|mut c| c.as_mut().and_then(|c| c.try_wait().ok()).flatten())
                        .is_some();
                    if is_dead {
                        log::warn!(
                            "[HEARTBEAT] agent {} died, auto-respawning",
                            &seed[..seed.floor_char_boundary(seed.len().min(8))]
                        );
                        let _ = ensure_agent(&seed);
                    }
                }
            })
            .expect("failed to spawn heartbeat");
    }

    fn mark_shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
    }

    fn send_shutdown(&self) -> Result<(), String> {
        let frame = Ui2Agent::Shutdown;
        let json = serde_json::to_string(&frame).map_err(|e| format!("serialize: {e}"))?;
        let mut stdin = self.stdin.lock().map_err(|e| format!("lock: {e}"))?;
        let _ = writeln!(*stdin, "{}", json);
        let _ = stdin.flush();
        Ok(())
    }

    pub fn shutdown_and_wait(self) {
        self.mark_shutdown();
        let seed = &self.seed[..self.seed.floor_char_boundary(self.seed.len().min(8))];
        let _ = self.send_shutdown();
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let start = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            if start.elapsed() > std::time::Duration::from_secs(5) {
                                log::warn!(
                                    "[REGISTRY] agent {seed} did not exit after kill, force-killing again"
                                );
                                let _ = child.kill();
                                let _ = child.wait();
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        Err(e) => {
                            log::warn!("[REGISTRY] error waiting for agent {seed}: {e}");
                            break;
                        }
                    }
                }
            }
        }
        log::info!("[REGISTRY] killed agent for seed={seed}");
    }
}

// ── Public API ──

pub(crate) fn ensure_agent(seed: &str) -> Result<(), String> {
    let mut registry = AgentRegistry::get()
        .lock()
        .map_err(|e| format!("lock: {e}"))?;
    registry.get_or_spawn(seed)
}

pub(crate) fn send_to_agent(seed: &str, frame: Ui2Agent) -> Result<(), String> {
    log::info!(
        "[REGISTRY] send_to_agent seed={} type={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        agent2ui_event_name_for_ui(&frame)
    );

    let json = serde_json::to_string(&frame).map_err(|e| format!("serialize: {e}"))?;
    let stdin_arc = {
        let registry = AgentRegistry::get()
            .lock()
            .map_err(|e| format!("lock: {e}"))?;
        registry
            .instances
            .get(seed)
            .ok_or_else(|| {
                format!(
                    "No agent running for seed: {}",
                    &seed[..seed.floor_char_boundary(seed.len().min(8))]
                )
            })?
            .stdin
            .clone()
    };
    let mut stdin = stdin_arc.lock().map_err(|e| format!("lock: {e}"))?;
    if writeln!(*stdin, "{}", json).is_err() || stdin.flush().is_err() {
        // Agent process is dead — remove it, respawn, and retry once.
        let mut registry = AgentRegistry::get()
            .lock()
            .map_err(|e| format!("lock: {e}"))?;
        registry.instances.remove(seed);
        drop(registry);
        log::warn!(
            "[REGISTRY] agent {} pipe broken, respawning...",
            &seed[..seed.floor_char_boundary(seed.len().min(8))]
        );
        let mut registry = AgentRegistry::get()
            .lock()
            .map_err(|e| format!("lock: {e}"))?;
        registry.get_or_spawn(seed)?;
        let stdin_arc2 = registry
            .instances
            .get(seed)
            .ok_or_else(|| format!("respawn failed for {seed}"))?
            .stdin
            .clone();
        drop(registry);
        let mut stdin2 = stdin_arc2.lock().map_err(|e| format!("lock: {e}"))?;
        writeln!(*stdin2, "{}", json).map_err(|e| format!("write retry: {e}"))?;
        stdin2.flush().map_err(|e| format!("flush retry: {e}"))?;
    }
    Ok(())
}

/// Send an interaction response only to the exact agent process generation
/// that requested it. Unlike ordinary user input, this never respawns or
/// retries: carrying an approval across a process boundary is unsafe.
pub(crate) fn send_to_agent_generation(
    seed: &str,
    generation: u64,
    frame: Ui2Agent,
) -> Result<(), String> {
    let registry = AgentRegistry::get()
        .lock()
        .map_err(|error| format!("lock: {error}"))?;
    if registry
        .activity_tracker
        .current(seed, generation)
        .is_none()
    {
        return Err(format!(
            "Agent generation {generation} is no longer current for session {seed}"
        ));
    }
    let stdin = registry
        .instances
        .get(seed)
        .ok_or_else(|| format!("No agent running for seed: {seed}"))?
        .stdin
        .clone();
    let json = serde_json::to_string(&frame).map_err(|error| format!("serialize: {error}"))?;
    let mut stdin = stdin.lock().map_err(|error| format!("lock: {error}"))?;
    writeln!(*stdin, "{json}").map_err(|error| format!("write: {error}"))?;
    stdin.flush().map_err(|error| format!("flush: {error}"))
}

/// Shutdown all running agents. Called on window close.
pub fn shutdown_all_agents() {
    if let Some(registry) = REGISTRY.get() {
        if let Ok(mut reg) = registry.lock() {
            reg.shutdown_all();
        }
    }
}
