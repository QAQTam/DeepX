//! Agent subprocess lifecycle management.
//!
//! AgentRegistry manages multiple agent child processes, one per session seed.
//! Each agent communicates via stdin/stdout JSON-framed protocol (Ui2Agent/Agent2Ui).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use deepx_proto::{Agent2Ui, Ui2Agent};

use super::platform::SYSTEM_PATH;
use super::util::agent2ui_event_name_for_ui;

// ── Agent instance ──

pub struct AgentInstance {
    #[allow(dead_code)]
    seed: String,
    stdin: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    shutdown_flag: Arc<AtomicBool>,
}

/// Global registry of all running agent subprocesses, keyed by session seed.
static REGISTRY: OnceLock<Mutex<AgentRegistry>> = OnceLock::new();

// ═══════════════════════════════════════════════════════════════
// AgentRegistry
// ═══════════════════════════════════════════════════════════════

pub struct AgentRegistry {
    pub(crate) instances: HashMap<String, AgentInstance>,
    app_handle: AppHandle,
}

impl std::fmt::Debug for AgentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRegistry")
            .field("instances", &self.instances.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Spawn an agent child process (free function, no lock needed).
/// Returns an AgentInstance ready for insertion into the registry.
fn spawn_agent_process(
    seed: &str,
    new_seed: Option<&str>,
    app_handle: &AppHandle,
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
            if app_handle.emit(&event_label, &payload).is_err() {
                break;
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
        let _ = app_handle.emit(&event_label, payload.clone());
        let _ = app_handle.emit("agent-event", payload);
    });

    let inst = AgentInstance {
        seed: seed.to_string(),
        stdin: Arc::new(Mutex::new(Box::new(stdin))),
        child: child_for_check,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
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
        let instance = spawn_agent_process(seed, new_seed, &self.app_handle)?;
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

/// Shutdown all running agents. Called on window close.
pub fn shutdown_all_agents() {
    if let Some(registry) = REGISTRY.get() {
        if let Ok(mut reg) = registry.lock() {
            reg.shutdown_all();
        }
    }
}
