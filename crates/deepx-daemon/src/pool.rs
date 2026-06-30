//! Agent process pool: spawn, monitor, and restart per-session agent processes.
//!
//! Each agent communicates with the daemon via stdin/stdout pipes.
//! The daemon routes FrontendToDaemon frames to the correct agent's stdin,
//! and broadcasts the agent's Agent2Ui events to all subscribed frontends.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use deepx_proto::{Agent2Ui, Ui2Agent};

/// Maximum consecutive restart attempts before marking unhealthy.
const MAX_RESTART_COUNT: u32 = 3;
/// Idle timeout before killing an unused agent (seconds).
const IDLE_TIMEOUT_SECS: u64 = 1800;

type Seed = String;
type FrontendId = usize;

/// An event from an agent process, tagged with its seed.
#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub seed: Seed,
    pub event: Agent2Ui,
}

/// Handle to a running agent process.
struct AgentHandle {
    process: Child,
    /// Stdin writer (daemon writes Ui2Agent frames here).
    stdin: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Channel receiver for Agent2Ui events from the reader thread.
    event_rx: mpsc::Receiver<Agent2Ui>,
    last_activity: Instant,
    restart_count: u32,
}

/// Manages a pool of agent processes, one per session seed.
pub struct AgentPool {
    agents: HashMap<Seed, AgentHandle>,
    /// Sender to broadcast AgentEvents. Cloned to each reader thread.
    event_tx: mpsc::Sender<AgentEvent>,
    /// Receiver for the daemon's main loop.
    pub event_rx: mpsc::Receiver<AgentEvent>,
    /// Path to the agent binary (same as current exe).
    agent_exe: std::path::PathBuf,
    max_idle: std::time::Duration,
}

impl AgentPool {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let agent_exe = std::env::current_exe().unwrap_or_else(|_| "deepx".into());
        Self {
            agents: HashMap::new(),
            event_tx,
            event_rx,
            agent_exe,
            max_idle: std::time::Duration::from_secs(IDLE_TIMEOUT_SECS),
        }
    }

    /// Get or spawn an agent for the given seed. Returns true if newly spawned.
    pub fn ensure_agent(&mut self, seed: &str) -> Result<bool, String> {
        if let Some(handle) = self.agents.get_mut(seed) {
            // Check if the process is still alive
            match handle.process.try_wait() {
                Ok(Some(_)) => {
                    // Process died — remove and re-spawn
                    log::warn!("[POOL] agent {} died unexpectedly, restarting", &seed[..seed.len().min(8)]);
                    handle.restart_count += 1;
                    if handle.restart_count > MAX_RESTART_COUNT {
                        return Err(format!("Agent {} exceeded max restart count", seed));
                    }
                    drop(handle); // release borrow
                    self.agents.remove(seed);
                    // Fall through to spawn
                }
                Ok(None) => {
                    // Still alive
                    handle.last_activity = Instant::now();
                    return Ok(false);
                }
                Err(_) => {
                    self.agents.remove(seed);
                    // Fall through to spawn
                }
            }
        }
        self.spawn_agent(seed)?;
        Ok(true)
    }

    /// Spawn a new agent child process for `seed`.
    fn spawn_agent(&mut self, seed: &str) -> Result<(), String> {
        let mut cmd = Command::new(&self.agent_exe);
        cmd.arg("agent")
            .arg("--resume-seed").arg(seed)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn()
            .map_err(|e| format!("spawn agent {}: {e}", &seed[..seed.len().min(8)]))?;

        let stdin = child.stdin.take()
            .ok_or("no stdin")?;
        let stdout = child.stdout.take()
            .ok_or("no stdout")?;

        let seed_owned = seed.to_string();
        let event_tx = self.event_tx.clone();

        // Reader thread: agent stdout → AgentEvent channel
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                if line.trim().is_empty() { continue; }
                if let Ok(event) = serde_json::from_str::<Agent2Ui>(&line) {
                    let _ = event_tx.send(AgentEvent {
                        seed: seed_owned.clone(),
                        event,
                    });
                }
            }
            log::info!("[POOL] agent {} stdout reader exiting", &seed_owned[..seed_owned.len().min(8)]);
        });

        let handle = AgentHandle {
            process: child,
            stdin: Arc::new(Mutex::new(Box::new(stdin))),
            event_rx: mpsc::channel().1, // dummy, unused
            last_activity: Instant::now(),
            restart_count: 0,
        };

        self.agents.insert(seed.to_string(), handle);
        log::info!("[POOL] spawned agent for seed={}", &seed[..seed.len().min(8)]);
        Ok(())
    }

    /// Send a Ui2Agent frame to a specific agent.
    pub fn send_to_agent(&self, seed: &str, frame: &Ui2Agent) -> Result<(), String> {
        let handle = self.agents.get(seed)
            .ok_or_else(|| format!("no agent for seed {}", &seed[..seed.len().min(8)]))?;
        let json = serde_json::to_string(frame)
            .map_err(|e| format!("serialize: {e}"))?;
        let mut stdin = handle.stdin.lock()
            .map_err(|e| format!("lock: {e}"))?;
        writeln!(*stdin, "{}", json)
            .map_err(|e| format!("write: {e}"))?;
        stdin.flush().map_err(|e| format!("flush: {e}"))
    }

    /// Kill and remove an agent.
    pub fn kill_agent(&mut self, seed: &str) {
        if let Some(mut handle) = self.agents.remove(seed) {
            let _ = handle.process.kill();
            let _ = handle.process.wait();
        }
    }

    /// Reap idle agents that exceed max_idle.
    pub fn reap_idle(&mut self) {
        let now = Instant::now();
        self.agents.retain(|seed, handle| {
            let idle = now.duration_since(handle.last_activity);
            if idle > self.max_idle {
                log::info!("[POOL] reaping idle agent {}", &seed[..seed.len().min(8)]);
                let _ = handle.process.kill();
                let _ = handle.process.wait();
                false
            } else {
                true
            }
        });
    }

    /// Shutdown all agents gracefully.
    pub fn shutdown_all(&mut self) {
        for (seed, mut handle) in self.agents.drain() {
            // Send shutdown frame
            let frame = Ui2Agent::Shutdown;
            if let Ok(json) = serde_json::to_string(&frame) {
                if let Ok(mut stdin) = handle.stdin.lock() {
                    let _ = writeln!(*stdin, "{}", json);
                    let _ = stdin.flush();
                }
            }
            let _ = handle.process.wait();
            log::info!("[POOL] shut down agent {}", &seed[..seed.len().min(8)]);
        }
    }
}
