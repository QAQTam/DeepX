//! AgentRegistry — manages multiple agent child processes, one per session.
//!
//! Architecture (v9 — direct child process spawn):
//! - Each session gets its own agent subprocess, spawned directly via stdin/stdout pipes.
//! - A per-agent reader thread dispatches Agent2Ui events from stdout to Tauri events.
//! - Tauri commands write Ui2Agent frames directly to the agent's stdin pipe.
//! - `shutdown_all()` kills all child processes directly.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

use deepx_proto::{Agent2Ui, Ui2Agent};

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

/// Cached full system PATH captured at startup (Windows GUI apps get stripped PATH).
static SYSTEM_PATH: OnceLock<String> = OnceLock::new();

/// Capture the full system PATH at process startup, before any Windows GUI stripping.
/// Must be called from main() early, before Tauri initialization.
pub fn cache_system_path() {
    let mut path = std::env::var("PATH").unwrap_or_default();
    
    // On Windows GUI apps, the process PATH may be stripped. Read the full
    // system+user PATH from the registry as a reliable fallback.
    #[cfg(target_os = "windows")]
    {
        let reg_path = windows_reg_path();
        if !reg_path.is_empty() {
            // Merge with current PATH, deduplicating
            let mut seen: std::collections::HashSet<String> = path.split(';').map(|s| s.to_string()).collect();
            for segment in reg_path.split(';') {
                if !segment.is_empty() && seen.insert(segment.to_string()) {
                    if !path.is_empty() { path.push(';'); }
                    path.push_str(segment);
                }
            }
        }
    }
    
    let _ = SYSTEM_PATH.set(path.clone());
    // Apply the full PATH to the current process so all child processes
    // (agent subprocess, pwsh via conpty, daemon, etc.) inherit it automatically.
    unsafe { std::env::set_var("PATH", &path); }
}

/// Detect OS version and store it for injection into the system prompt [SESSION] block.
/// Must be called from main() early, before any session is created.
pub fn detect_os_info() {
    #[cfg(target_os = "windows")]
    {
        let info = windows_os_info();
        if !info.is_empty() {
            let _ = deepx_config::prompt::OS_INFO.set(info);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let info = unix_os_info();
        let _ = deepx_config::prompt::OS_INFO.set(info);
    }
    // Detect shell + toolchain versions
    let tools = detect_tools();
    let _ = deepx_config::prompt::TOOLS_INFO.set(tools);
}

#[cfg(target_os = "windows")]
fn windows_reg_path() -> String {
    
    unsafe {
        // Win32 FFI declarations
        unsafe extern "system" {
            fn RegOpenKeyExW(
                hkey: isize, subkey: *const u16, _uloptions: u32,
                _samdesired: u32, phkresult: *mut isize,
            ) -> i32;
            fn RegQueryValueExW(
                hkey: isize, value: *const u16, _reserved: *const u8,
                pdwtype: *mut u32, pbdata: *mut u8, pcbdata: *mut u32,
            ) -> i32;
            fn RegCloseKey(hkey: isize) -> i32;
        }
        
        const HKEY_LOCAL_MACHINE: isize = -2147483646i64 as isize; // 0x80000002
        const HKEY_CURRENT_USER: isize = -2147483647i64 as isize;   // 0x80000001
        const KEY_READ: u32 = 0x20019;
        
        let mut result = String::new();
        
        for (hkey, subkey_str) in [
            (HKEY_LOCAL_MACHINE, "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment\0"),
            (HKEY_CURRENT_USER, "Environment\0"),
        ] {
            let subkey_wide: Vec<u16> = subkey_str.encode_utf16().collect();
            let value_name: Vec<u16> = "PATH\0".encode_utf16().collect();
            let mut key_handle: isize = 0;
            
            if RegOpenKeyExW(hkey, subkey_wide.as_ptr(), 0, KEY_READ, &mut key_handle) != 0 {
                continue;
            }
            
            let mut data_type: u32 = 0;
            let mut data_size: u32 = 0;
            
            if RegQueryValueExW(key_handle, value_name.as_ptr(), std::ptr::null(),
                &mut data_type, std::ptr::null_mut(), &mut data_size) != 0 || data_size == 0 {
                RegCloseKey(key_handle);
                continue;
            }
            
            let mut buf: Vec<u16> = vec![0u16; (data_size / 2) as usize + 1];
            if RegQueryValueExW(key_handle, value_name.as_ptr(), std::ptr::null(),
                &mut data_type, buf.as_mut_ptr() as *mut u8, &mut data_size) != 0 {
                RegCloseKey(key_handle);
                continue;
            }
            RegCloseKey(key_handle);
            
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let path = String::from_utf16_lossy(&buf[..len]);
            
            if !result.is_empty() { result.push(';'); }
            result.push_str(&path);
        }
        
        result
    }
}

/// Read a string value from a Windows registry key (returns empty if not found).
#[cfg(target_os = "windows")]
fn reg_read_string(hkey: isize, subkey_str: &str, value_name_str: &str) -> String {
    unsafe {
        unsafe extern "system" {
            fn RegOpenKeyExW(
                hkey: isize, subkey: *const u16, _uloptions: u32,
                _samdesired: u32, phkresult: *mut isize,
            ) -> i32;
            fn RegQueryValueExW(
                hkey: isize, value: *const u16, _reserved: *const u8,
                pdwtype: *mut u32, pbdata: *mut u8, pcbdata: *mut u32,
            ) -> i32;
            fn RegCloseKey(hkey: isize) -> i32;
        }
        const KEY_READ: u32 = 0x20019;
        let subkey_wide: Vec<u16> = subkey_str.encode_utf16().collect();
        let value_wide: Vec<u16> = value_name_str.encode_utf16().collect();
        let mut key_handle: isize = 0;
        if RegOpenKeyExW(hkey, subkey_wide.as_ptr(), 0, KEY_READ, &mut key_handle) != 0 {
            return String::new();
        }
        let mut data_type: u32 = 0;
        let mut data_size: u32 = 0;
        if RegQueryValueExW(key_handle, value_wide.as_ptr(), std::ptr::null(),
            &mut data_type, std::ptr::null_mut(), &mut data_size) != 0 || data_size == 0 {
            RegCloseKey(key_handle);
            return String::new();
        }
        let mut buf: Vec<u16> = vec![0u16; (data_size / 2) as usize + 1];
        if RegQueryValueExW(key_handle, value_wide.as_ptr(), std::ptr::null(),
            &mut data_type, buf.as_mut_ptr() as *mut u8, &mut data_size) != 0 {
            RegCloseKey(key_handle);
            return String::new();
        }
        RegCloseKey(key_handle);
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }
}

/// Build an OS info string like "Windows 11 Pro 24H2 26200.5518".
#[cfg(target_os = "windows")]
fn windows_os_info() -> String {
    const HKLM: isize = -2147483646i64 as isize; // 0x80000002
    let nt = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0";
    let name = reg_read_string(HKLM, nt, "ProductName\0");
    if name.is_empty() { return String::new(); }
    let display = reg_read_string(HKLM, nt, "DisplayVersion\0");
    let build = reg_read_string(HKLM, nt, "CurrentBuild\0");
    let ubr = reg_read_string(HKLM, nt, "UBR\0");
    if build.is_empty() { return name; }
    let mut s = name;
    if !display.is_empty() {
        s.push_str(&format!(" {} ({}.{})", display, build, ubr));
    } else {
        s.push_str(&format!(" ({}.{})", build, ubr));
    }
    s
}

/// Detect OS info on Unix via uname.
#[cfg(not(target_os = "windows"))]
fn unix_os_info() -> String {
    use std::process::Command;
    let sysname = Command::new("uname").arg("-s").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let release = Command::new("uname").arg("-r").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if sysname.is_empty() { return String::new(); }
    if release.is_empty() { sysname } else { format!("{} {}", sysname, release) }
}

/// Quick scan of shell version and common toolchains on PATH.
fn detect_tools() -> String {
    use std::process::Command;
    /// Run a command, return first line of output or empty.
    fn try_version(cmd: &str, args: &[&str]) -> Option<String> {
        let child = Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        let output = child.wait_with_output().ok()?;
        // Some tools (python, java) output version to stderr
        let raw = if output.stdout.is_empty() { &output.stderr } else { &output.stdout };
        let s = String::from_utf8_lossy(raw);
        let first_line = s.lines().next().unwrap_or("").trim().to_string();
        if first_line.is_empty() { None } else { Some(first_line) }
    }
    // Ordered: shell first, then important toolchains
    let probes: &[(&str, &[&str])] = &[
        #[cfg(target_os = "windows")]
        ("pwsh", &["--version"]),
        #[cfg(not(target_os = "windows"))]
        ("bash", &["--version"]),
        ("rustc", &["--version"]),
        ("cargo", &["--version"]),
        ("python", &["--version"]),
        ("python3", &["--version"]),
        ("node", &["--version"]),
        ("git", &["--version"]),
        ("java", &["--version"]),
    ];
    let mut parts: Vec<String> = Vec::new();
    for (cmd, args) in probes {
        if let Some(v) = try_version(cmd, args) {
            // Compact: "rustc 1.92.0" or "pwsh 7.4.6"
            // Keep first 60 chars to avoid junk
            let short = if v.len() > 80 {
                let boundary = v.floor_char_boundary(77);
                format!("{}...", &v[..boundary])
            } else { v };
            parts.push(short);
        }
    }
    parts.join(" | ")
}

pub struct AgentRegistry {
    instances: HashMap<String, AgentInstance>,
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
fn spawn_agent_process(seed: &str, new_seed: Option<&str>, app_handle: &AppHandle) -> Result<AgentInstance, String> {
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

    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn agent for seed {seed}: {e}"))?;

    let stdin = child.stdin.take()
        .ok_or_else(|| "Failed to get stdin".to_string())?;
    let stdout = child.stdout.take()
        .ok_or_else(|| "Failed to get stdout".to_string())?;
    let stderr = child.stderr.take()
        .ok_or_else(|| "Failed to get stderr".to_string())?;

    // Debug: pipe agent stderr to a per-seed log file
    let debug_seed = seed.to_string();
    std::thread::spawn(move || {
        let log_path = deepx_types::platform::data_dir()
            .join(format!("agent_{}_debug.log", &debug_seed[..debug_seed.floor_char_boundary(debug_seed.len().min(8))]));
        let mut writer = std::fs::File::create(&log_path)
            .unwrap_or_else(|_| std::fs::File::create(deepx_types::platform::data_dir().join("agent_debug.log")).unwrap());
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
    // Arc for try_wait check on reader exit (diagnose kill-vs-crash)
    let child_for_check = Arc::new(Mutex::new(Some(child)));
    let child_for_thread = child_for_check.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    log::info!("[REGISTRY] agent {seed_owned} stdout read error: {e}");
                    break;
                }
            };
            if line.trim().is_empty() { continue; }
            let event: Agent2Ui = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(e) => {
                    log::info!("[REGISTRY] failed to parse from {}: {} -- error: {e}",
                        &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))],
                        &line[..line.floor_char_boundary(line.len().min(80))]);
                    continue;
                }
            };
            let event_type = agent2ui_event_name(&event);
            log::info!("[REGISTRY] agent {} got event: {}", &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))], event_type);
            let payload = serde_json::to_value(&event).unwrap_or_default();
            if app_handle.emit(&format!("agent-{}-event", seed_owned), payload.clone()).is_err() {
                break;
            }
            let _ = app_handle.emit("agent-event", payload);
        }
        log::warn!("[REGISTRY] agent {} stdout reader thread exiting", seed_owned);
        // Check if the child process actually exited (vs just pipe closed)
        let exit_status = child_for_thread.lock().ok()
            .and_then(|mut c| c.as_mut().and_then(|c| c.try_wait().ok()).flatten());
        log::warn!("[REGISTRY] agent {} child exit status: {:?}", &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))], exit_status);
        // Notify frontend that the agent died so it can trigger reconnection
        let error_event = Agent2Ui::Error {
            message: format!("Agent process for session {} has exited unexpectedly", &seed_owned[..seed_owned.floor_char_boundary(seed_owned.len().min(8))]),
        };
        let payload = serde_json::to_value(&error_event).unwrap_or_default();
        let _ = app_handle.emit(&format!("agent-{}-event", seed_owned), payload.clone());
        let _ = app_handle.emit("agent-event", payload);
    });

    let inst = AgentInstance {
        seed: seed.to_string(),
        stdin: Arc::new(Mutex::new(Box::new(stdin))),
        child: child_for_check,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
    };
    // Start heartbeat to auto-recover if agent dies
    inst.spawn_heartbeat();
    Ok(inst)
}

impl AgentRegistry {
    fn get() -> &'static Mutex<AgentRegistry> {
        REGISTRY.get().expect("AgentRegistry not initialized — call init() first")
    }

    /// Initialize the global registry and SessionManager. Called once at startup.
    pub fn init(app: &AppHandle) {
        deepx_session::SessionManager::init(deepx_types::platform::data_dir(), None);
        let registry = AgentRegistry {
            instances: HashMap::new(),
            app_handle: app.clone(),
        };
        REGISTRY.set(Mutex::new(registry))
            .expect("AgentRegistry already initialized");
    }

    /// Get or spawn an agent for the given seed. If an agent is already running for this
    /// seed, returns immediately. Otherwise spawns a new subprocess that auto-resumes
    /// the session. The heavy spawn work is done outside the registry lock.
    pub fn get_or_spawn(&mut self, seed: &str) -> Result<(), String> {
        if self.instances.contains_key(seed) {
            return Ok(());
        }
        // Spawn outside lock context — caller already holds lock, so we must
        // do the spawn inline. The lock is held briefly for HashMap ops only;
        // the actual process spawn happens here but it's acceptably fast (~ms).
        self.spawn_agent(seed, None)
    }

    /// Spawn a new agent for a brand-new session. The seed is pre-generated by the caller.
    pub fn spawn_new(&mut self, seed: &str) -> Result<(), String> {
        if self.instances.contains_key(seed) {
            return Err(format!("Agent for seed {} already exists", seed));
        }
        self.spawn_agent(seed, Some(seed))
    }

    /// Internal: spawn a child process running `deepx agent`.
    /// If `new_seed` is Some, passes `--seed {seed}` (create new session).
    /// Otherwise passes `--resume-seed {seed}` (resume existing).
    /// The instance is inserted into self.instances.
    fn spawn_agent(&mut self, seed: &str, new_seed: Option<&str>) -> Result<(), String> {
        let instance = spawn_agent_process(seed, new_seed, &self.app_handle)?;
        self.instances.insert(seed.to_string(), instance);
        log::info!("[REGISTRY] spawned agent for seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
        Ok(())
    }

    /// Send a Ui2Agent frame to a specific agent instance.
    /// Only holds the registry lock during lookup, not during the write.
    pub fn send_to(&self, seed: &str, frame: &Ui2Agent) -> Result<(), String> {
        let stdin_arc = self.instances.get(seed)
            .ok_or_else(|| format!("No agent running for seed: {}", &seed[..seed.floor_char_boundary(seed.len().min(8))]))?
            .stdin.clone();
        // Registry lock released here via the caller dropping the MutexGuard
        let json = serde_json::to_string(frame).map_err(|e| format!("serialize: {e}"))?;
        let mut stdin = stdin_arc.lock().map_err(|e| format!("lock: {e}"))?;
        writeln!(*stdin, "{}", json).map_err(|e| format!("write: {e}"))?;
        stdin.flush().map_err(|e| format!("flush: {e}"))
    }

    /// Kill and remove a specific agent instance from the registry.
    /// Returns the removed instance so the caller can wait for process exit
    /// **outside** the registry lock.
    pub fn kill_agent(&mut self, seed: &str) -> Option<AgentInstance> {
        let instance = self.instances.remove(seed);
        if instance.is_some() {
            log::info!("[REGISTRY] removed agent for seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
        }
        instance
    }

    /// Shutdown all agents gracefully. Waits for each process **outside** the lock.
    pub fn shutdown_all(&mut self) {
        let drained: Vec<AgentInstance> = self.instances.drain().map(|(_, v)| v).collect();
        for inst in drained {
            inst.shutdown_and_wait();
        }
    }
}

impl AgentInstance {
    /// Spawn a background heartbeat thread that checks agent liveness every 10s.
    /// If the agent process dies unexpectedly, triggers auto-respawn.
    fn spawn_heartbeat(&self) {
        let seed = self.seed.clone();
        let child = self.child.clone();
        let shutdown = self.shutdown_flag.clone();
        std::thread::Builder::new()
            .name(format!("hb-{}", &seed[..seed.floor_char_boundary(seed.len().min(8))]))
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(10));
                if shutdown.load(Ordering::Relaxed) { break; }
                let is_dead = child.lock().ok()
                    .and_then(|mut c| c.as_mut().and_then(|c| c.try_wait().ok()).flatten())
                    .is_some();
                if is_dead {
                    log::warn!("[HEARTBEAT] agent {} died, auto-respawning",
                        &seed[..seed.floor_char_boundary(seed.len().min(8))]);
                    let _ = ensure_agent(&seed);
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

    /// Send shutdown, kill the child process, and wait for it to exit.
    /// Designed to be called **outside** the registry lock.
    pub fn shutdown_and_wait(self) {
        self.mark_shutdown();
        let seed = &self.seed[..self.seed.floor_char_boundary(self.seed.len().min(8))];
        let _ = self.send_shutdown();
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                // Wait up to 5s, then force-kill again if still running
                let start = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            if start.elapsed() > std::time::Duration::from_secs(5) {
                                log::warn!("[REGISTRY] agent {seed} did not exit after kill, force-killing again");
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

/// Ensure an agent is running for the given seed (resume existing session).
fn ensure_agent(seed: &str) -> Result<(), String> {
    let mut registry = AgentRegistry::get().lock().map_err(|e| format!("lock: {e}"))?;
    registry.get_or_spawn(seed)
}

/// Send a Ui2Agent frame to the agent for the given seed.
fn send_to_agent(seed: &str, frame: Ui2Agent) -> Result<(), String> {
    log::info!("[REGISTRY] send_to_agent seed={} type={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))], agent2ui_event_name_for_ui(&frame));
    
    let json = serde_json::to_string(&frame).map_err(|e| format!("serialize: {e}"))?;
    let stdin_arc = {
        let registry = AgentRegistry::get().lock().map_err(|e| format!("lock: {e}"))?;
        registry.instances.get(seed)
            .ok_or_else(|| format!("No agent running for seed: {}", &seed[..seed.floor_char_boundary(seed.len().min(8))]))?
            .stdin.clone()
    };
    let mut stdin = stdin_arc.lock().map_err(|e| format!("lock: {e}"))?;
    if writeln!(*stdin, "{}", json).is_err() || stdin.flush().is_err() {
        // Agent process is dead — remove it, respawn, and retry once.
        let mut registry = AgentRegistry::get().lock().map_err(|e| format!("lock: {e}"))?;
        registry.instances.remove(seed);
        drop(registry);
        log::warn!("[REGISTRY] agent {} pipe broken, respawning...", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
        let mut registry = AgentRegistry::get().lock().map_err(|e| format!("lock: {e}"))?;
        registry.get_or_spawn(seed)?;
        let stdin_arc2 = registry.instances.get(seed)
            .ok_or_else(|| format!("respawn failed for {seed}"))?
            .stdin.clone();
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

// ═══════════════════════════════════════════════════════════════════════════
// Tauri Commands (v6 — all commands now carry `seed`)
// ═══════════════════════════════════════════════════════════════════════════

/// Read a file preview: first `max_lines` lines, capped at `max_chars` chars.
/// Truncation is CJK-safe (uses char boundary).
fn read_file_preview(path: &str, max_lines: usize, max_chars: usize) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let reader = BufReader::new(file);
    let mut result = String::new();
    let mut line_count = 0;
    for line in reader.lines() {
        if line_count >= max_lines { break; }
        let line = line.map_err(|e| format!("read: {e}"))?;
        if !result.is_empty() { result.push('\n'); }
        result.push_str(&line);
        line_count += 1;
        if result.chars().count() >= max_chars {
            // Truncate at char boundary
            let end = result.floor_char_boundary(max_chars);
            result.truncate(end);
            result.push_str("\n… (truncated)");
            break;
        }
    }
    Ok(result)
}

/// Send a user text message to the agent for the given session.
/// If `files` is non-empty, reads and truncates each file,
/// prepending their content to the user text.
#[tauri::command]
pub fn cmd_send_message(
    seed: String,
    text: String,
    files: Option<Vec<String>>,
) -> Result<(), String> {
    let files = files.unwrap_or_default();
    log::info!("[REGISTRY] cmd_send_message seed={}: {}", &seed[..seed.floor_char_boundary(seed.len().min(8))], &text[..text.floor_char_boundary(50)]);
    ensure_agent(&seed)?;

    let full_text = if files.is_empty() {
        text
    } else {
        let mut parts = Vec::new();
        parts.push("[Files]".to_string());
        for path in &files {
            match read_file_preview(path, 10, 1000) {
                Ok(preview) => {
                    parts.push(format!("\n{path}:\n{preview}"));
                }
                Err(e) => {
                    parts.push(format!("\n{path}: [ERROR: {e}]"));
                }
            }
        }
        parts.push(format!("\n\n[Message]\n{text}"));
        parts.join("")
    };

    send_to_agent(&seed, Ui2Agent::UserInput { text: full_text })
}

/// Set the agent's operating mode (Normal, Plan, Code).
#[tauri::command]
pub fn cmd_set_mode(seed: String, mode: String) -> Result<(), String> {
    log::info!("[REGISTRY] cmd_set_mode seed={} mode={mode}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
    send_to_agent(&seed, Ui2Agent::SetMode { mode })
}

/// Return the app version from Cargo.toml.
#[tauri::command]
pub fn cmd_get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Return all registered tool names. Used by Settings for default tools.
#[tauri::command]
pub fn cmd_list_available_tools() -> Result<String, String> {
    let tools = deepx_tools::bridge::all_tool_names();
    serde_json::to_string(&tools).map_err(|e| format!("{e}"))
}

/// Resume an existing session — spawns agent with --resume-seed if not already running.
/// The agent auto-loads the session on startup and emits SessionRestored.
#[tauri::command]
pub fn cmd_resume_session(seed: String) -> Result<(), String> {
    log::info!("[REGISTRY] cmd_resume_session seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
    deepx_session::SessionManager::global().set_active_seed(&seed);
    ensure_agent(&seed)?;
    Ok(())
}

/// Create a new session with a pre-generated seed.
#[tauri::command]
pub fn cmd_new_session() -> Result<String, String> {
    let seed = deepx_session::SessionManager::generate_seed();
    log::info!("[REGISTRY] cmd_new_session seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
    deepx_session::SessionManager::global().clear_active();
    {
        let mut registry = AgentRegistry::get().lock().map_err(|e| format!("lock: {e}"))?;
        registry.spawn_new(&seed)?;
    }
    Ok(seed)
}

/// Cancel the current operation for the given session.
#[tauri::command]
pub fn cmd_cancel(seed: String) -> Result<(), String> {
    send_to_agent(&seed, Ui2Agent::Cancel)
}

/// Save configuration and reload all agents.
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
    context7_api_key: String,
    subagent_model: String,
    subagent_base_url: String,
    subagent_api_key: String,
    subagent_max_tokens: u32,
    subagent_timeout_secs: u64,
    subagent_default_tools: Vec<String>,
) -> Result<(), String> {
    let mut cfg = deepx_config::Config::load().unwrap_or_default();
    let set_str = |dest: &mut String, src: &str| { if !src.is_empty() { *dest = src.to_string(); } };
    let set_u32 = |dest: &mut u32, src: u32| { if src > 0 { *dest = src; } };
    let set_u64 = |dest: &mut u64, src: u64| { if src > 0 { *dest = src; } };

    set_str(&mut cfg.api_key, &api_key);
    set_str(&mut cfg.model, &model);
    set_str(&mut cfg.base_url, &base_url);
    set_str(&mut cfg.provider_id, &provider_id);
    set_str(&mut cfg.endpoint, &endpoint);
    set_u32(&mut cfg.max_tokens, max_tokens);
    set_u32(&mut cfg.context_limit, context_limit);
    set_str(&mut cfg.reasoning_effort, &reasoning_effort);
    if !lang.is_empty() { cfg.lang = Some(lang); }
    if !context7_api_key.is_empty() { cfg.context7_api_key = Some(context7_api_key); }

    set_str(&mut cfg.subagent.model, &subagent_model);
    set_str(&mut cfg.subagent.base_url, &subagent_base_url);
    set_str(&mut cfg.subagent.api_key, &subagent_api_key);
    set_u32(&mut cfg.subagent.max_tokens, subagent_max_tokens);
    set_u64(&mut cfg.subagent.timeout_secs, subagent_timeout_secs);
    if !subagent_default_tools.is_empty() { cfg.subagent.default_tools = subagent_default_tools; }
    cfg.save()?;
    // Broadcast reload to all running agents
    let registry = AgentRegistry::get().lock().map_err(|e| format!("lock: {e}"))?;
    for seed in registry.instances.keys() {
        let _ = registry.send_to(seed, &Ui2Agent::ReloadConfig);
    }
    Ok(())
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
        "active_profile": cfg.active_profile,
        "context7_api_key": if cfg.context7_api_key.as_deref().unwrap_or("").is_empty() { "" } else { "****" },
        "providers": providers,
        "subagent": {
            "model": cfg.subagent.model,
            "base_url": cfg.subagent.base_url,
            "api_key": if cfg.subagent.api_key.is_empty() { "" } else { "****" },
            "max_tokens": cfg.subagent.max_tokens,
            "timeout_secs": cfg.subagent.timeout_secs,
            "default_tools": cfg.subagent.default_tools,
        },
    });
    serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"))
}

/// List all sessions with metadata.
#[tauri::command]
pub fn cmd_list_sessions() -> Result<String, String> {
    let metas = deepx_session::SessionManager::global().list();
    serde_json::to_string(&metas).map_err(|e| format!("serialize: {e}"))
}

/// Delete a session by seed. Also kills the agent if running for that seed.
#[tauri::command]
pub fn cmd_delete_session(seed: String) -> Result<(), String> {
    log::info!("[REGISTRY] cmd_delete_session seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
    // Kill the agent first so it doesn't resurrect the session on flush.
    let instance = {
        if let Ok(mut registry) = AgentRegistry::get().lock() {
            registry.kill_agent(&seed)
        } else { None }
    };
    if let Some(inst) = instance {
        inst.shutdown_and_wait();
    }
    deepx_session::SessionManager::global().delete(&seed)
}

/// Undo a turn and all subsequent content.
#[tauri::command]
pub fn cmd_undo_turn(seed: String, turn_id: String) -> Result<(), String> {
    send_to_agent(&seed, Ui2Agent::UndoTurn { turn_id })
}

/// Compact conversation history (summarize old turns).
#[tauri::command]
pub fn cmd_compact(seed: String) -> Result<(), String> {
    log::info!("[REGISTRY] cmd_compact seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
    send_to_agent(&seed, Ui2Agent::Compact)
}

/// Load older turns from session history (paginated, 20 at a time before the given turn).
#[tauri::command]
pub fn cmd_load_more_turns(seed: String, before_turn_id: String) -> Result<(), String> {
    send_to_agent(&seed, Ui2Agent::LoadMoreTurns { before_turn_id, count: 20 })
}

/// Get the current session's workspace root path.
#[tauri::command]
pub fn cmd_get_workspace(seed: String) -> Result<String, String> {
    if seed.is_empty() { return Ok(String::new()); }
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    Ok(std::fs::read_to_string(dir.join("workspace.txt")).unwrap_or_default().trim().to_string())
}

/// Set the current session's workspace root path and notify the agent.
#[tauri::command]
pub fn cmd_set_workspace(seed: String, path: String) -> Result<(), String> {
    if seed.is_empty() { return Err("No active session".into()); }
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(dir.join("workspace.txt"), path.trim()).map_err(|e| format!("write: {e}"))?;
    // Tell agent to reload config (which includes workspace)
    send_to_agent(&seed, Ui2Agent::ReloadConfig)
}

/// Get aggregated code stats for the last N days (removed in v0.7.0).
// #[tauri::command]
// pub fn cmd_get_code_stats(seed: String, days: u32) -> Result<String, String> { ... }

/// Convert epoch seconds to "YYYY-MM-DD" UTC.
#[allow(dead_code)]
fn chrono_local_date_from_epoch(epoch_secs: u64) -> String {
    let total_days = (epoch_secs / 86400) as i64;
    let (y, m, d) = deepx_types::platform::civil_from_days(total_days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Kill the agent for a session (when tab is closed but session not deleted).
#[tauri::command]
pub fn cmd_close_session(seed: String) -> Result<(), String> {
    log::info!("[REGISTRY] cmd_close_session seed={}", &seed[..seed.floor_char_boundary(seed.len().min(8))]);
    // Extract instance under lock, then wait outside lock.
    let instance = {
        if let Ok(mut registry) = AgentRegistry::get().lock() {
            registry.kill_agent(&seed)
        } else { None }
    };
    if let Some(inst) = instance {
        inst.shutdown_and_wait();
    }
    Ok(())
}

/// Get git status for the current workspace: lists modified/new/deleted files with diff stats.
/// Runs independently of the agent process — reads git repo directly.
#[tauri::command]
pub fn cmd_get_git_diff(seed: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path).unwrap_or_default().trim().to_string()
    };
    if workspace.is_empty() { return Ok("[]".into()); }

    let repo = match git2::Repository::open(&workspace) {
        Ok(r) => r,
        Err(_) => return Ok("[]".into()),
    };

    let mut files: Vec<serde_json::Value> = Vec::new();

    if let Ok(statuses) = repo.statuses(None) {
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let status = entry.status();
            let change = if status.is_index_new() || status.is_wt_new() {
                "added"
            } else if status.is_index_deleted() || status.is_wt_deleted() {
                "deleted"
            } else if status.is_index_modified() || status.is_wt_modified() {
                "modified"
            } else if status.is_index_renamed() || status.is_wt_renamed() {
                "renamed"
            } else {
                continue;
            };

            // Per-file line stats: diff just this file against HEAD.
            let (lines_added, lines_removed) = if matches!(change, "modified" | "added") {
                let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
                let mut opts = git2::DiffOptions::new();
                opts.pathspec(&path);
                head_tree
                    .and_then(|tree| repo.diff_tree_to_workdir(Some(&tree), Some(&mut opts)).ok())
                    .and_then(|d| d.stats().ok())
                    .map(|s| (s.insertions() as u32, s.deletions() as u32))
                    .unwrap_or((0, 0))
            } else {
                (0, 0)
            };

            files.push(serde_json::json!({
                "path": path,
                "change": change,
                "lines_added": lines_added,
                "lines_removed": lines_removed,
            }));
        }
    }
    serde_json::to_string(&files).map_err(|e| format!("serialize: {e}"))
}

/// Get the diff for a single file in the workspace git repo.
#[tauri::command]
pub fn cmd_get_git_file_diff(seed: String, file_path: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path).unwrap_or_default().trim().to_string()
    };
    if workspace.is_empty() { return Ok("".into()); }

    let repo = git2::Repository::open(&workspace).map_err(|e| format!("open repo: {e}"))?;
    let head = repo.head().map_err(|e| format!("head: {e}"))?;
    let head_tree = head.peel_to_tree().map_err(|e| format!("tree: {e}"))?;

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(&file_path);

    let diff = repo.diff_tree_to_workdir(Some(&head_tree), Some(&mut diff_opts))
        .map_err(|e| format!("diff: {e}"))?;

    let mut patch_text = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        let content = std::str::from_utf8(line.content()).unwrap_or("");
        patch_text.push(origin);
        patch_text.push_str(content);
        true
    }).map_err(|e| format!("print diff: {e}"))?;

    Ok(patch_text)
}

/// Get dashboard data (tasks, recent edits) directly from session files.
/// Does NOT go through the agent process — reads disk directly.
#[tauri::command]
pub fn cmd_get_dashboard_data(seed: String) -> Result<String, String> {
    use std::io::BufRead;

    // Tasks from sessions/{seed}/tasks-mem.md
    let tasks: Vec<serde_json::Value> = {
        let path = deepx_types::platform::sessions_dir().join(&seed).join("tasks-mem.md");
        if let Ok(file) = std::fs::File::open(&path) {
            std::io::BufReader::new(file).lines()
                .filter_map(|l| l.ok())
                .filter(|l| l.starts_with("- ["))
                .filter_map(|line| {
                    let trimmed = line.trim_start();
                    let status = &trimmed[3..trimmed.find(']')?];
                    let after = trimmed.split_once("] ")?.1;
                    let (id_part, rest) = after.split_once(": ")?;
                    let (subject, description) = rest.split_once(" — ").unwrap_or((rest, ""));
                    Some(serde_json::json!({
                        "id": id_part.trim(),
                        "subject": subject.trim(),
                        "description": description.trim(),
                        "status": status,
                    }))
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    // Recent edits from code_stats.jsonl (last 10 unique files)
    let recent_edits: Vec<String> = {
        let path = deepx_types::platform::sessions_dir().join(&seed).join("code_stats.jsonl");
        if let Ok(file) = std::fs::File::open(&path) {
            let mut files: Vec<String> = std::io::BufReader::new(file).lines()
                .filter_map(|l| l.ok())
                .filter_map(|line| {
                    serde_json::from_str::<serde_json::Value>(&line).ok()
                        .and_then(|v| v.get("file").and_then(|f| f.as_str()).map(String::from))
                })
                .collect();
            files.reverse();
            files.dedup();
            files.truncate(10);
            files
        } else {
            Vec::new()
        }
    };

    serde_json::to_string(&serde_json::json!({
        "tasks": tasks,
        "recent_edits": recent_edits,
    })).map_err(|e| format!("serialize: {e}"))
}

/// Modify or delete a task directly from the frontend.
/// Writes to tasks-mem.md on disk, then sends a ToolCall frame to the agent
/// so its in-memory state stays in sync.
#[tauri::command]
pub fn cmd_task_action(seed: String, action: String, task_id: u32) -> Result<(), String> {
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    let path = dir.join("tasks-mem.md");
    let _guard = std::sync::Mutex::new(()); // serialize access

    let mut lines: Vec<String> = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
            .lines().map(String::from).collect()
    } else {
        Vec::new()
    };

    let prefix = format!("T{}:", task_id);
    let idx = lines.iter().position(|l| l.contains(&prefix));

    match action.as_str() {
        "cancel" => {
            let idx = idx.ok_or_else(|| format!("Task T{} not found", task_id))?;
            for marker in &["[pending]", "[in_progress]", "[completed]", "[cancelled]"] {
                if lines[idx].contains(marker) {
                    lines[idx] = lines[idx].replace(marker, "[cancelled]");
                    break;
                }
            }
        }
        "delete" => {
            if let Some(idx) = idx {
                lines.remove(idx);
            }
        }
        _ => return Err(format!("Unknown action: {action}")),
    }

    std::fs::write(&path, lines.join("\n")).map_err(|e| format!("write tasks: {e}"))?;

    // Notify agent if running
    let args = serde_json::json!({"id": task_id, "status": if action == "cancel" { "cancelled" } else { "deleted" }});
    let frame = if action == "cancel" {
        Ui2Agent::ToolCall { id: format!("frontend_tc_{}", task_id), name: "task".into(), action: "update".into(), args }
    } else {
        Ui2Agent::ToolCall { id: format!("frontend_tc_{}", task_id), name: "task".into(), action: "delete".into(), args }
    };
    let _ = send_to_agent(&seed, frame);
    Ok(())
}

/// Get context composition stats from the agent's compact stats file.
/// Returns JSON breakdown from context_stats.json (written by the agent after compaction).
#[tauri::command]
pub fn cmd_get_context_stats(seed: String) -> Result<String, String> {
    let stats_path = deepx_types::platform::sessions_dir().join(&seed).join("context_stats.json");
    if stats_path.exists() {
        return Ok(std::fs::read_to_string(&stats_path).unwrap_or_default());
    }
    // No stats yet — return zeroed template
    Ok(serde_json::json!({
        "messages":0,"chat_text":0,"thinking":0,"tool_calls":0,"tool_results":0,
        "tools_schema":0,"system_prompt":0,"thinking_blocks":0,"tool_call_blocks":0
    }).to_string())
}

/// Get aggregated token usage stats for the last N days.
/// Returns JSON: { daily: [{date, prompt_tokens, completion_tokens, cache_hit, cache_miss, calls}], totals: {...} }
#[tauri::command]
pub fn cmd_get_token_stats(days: u32) -> Result<String, String> {
    use std::collections::BTreeMap;
    use std::io::BufRead;

    let path = deepx_types::platform::data_dir().join("token_stats.jsonl");
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => {
            // No data yet — return empty result
            let result = serde_json::json!({
                "daily": generate_date_range(days),
                "totals": { "prompt_tokens": 0, "completion_tokens": 0, "calls": 0, "cache_hit_pct": 0.0 },
            });
            return serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"));
        }
    };
    let reader = std::io::BufReader::new(file);

    // Compute cutoff date string "YYYY-MM-DD"
    let cutoff = days_before_today(days);

    // Aggregate: date -> { prompt_tokens, completion_tokens, cache_hit, cache_miss, calls }
    let mut daily: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("read: {e}"))?;
        if line.trim().is_empty() { continue; }
        let entry: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| format!("parse: {e}"))?;
        let date = entry["date"].as_str().unwrap_or("").to_string();
        if date < cutoff { continue; } // before range, skip

        let prompt = entry["prompt_tokens"].as_u64().unwrap_or(0);
        let completion = entry["completion_tokens"].as_u64().unwrap_or(0);
        let hit = entry["cache_hit"].as_u64().unwrap_or(0);
        let miss = entry["cache_miss"].as_u64().unwrap_or(0);

        let day = daily.entry(date).or_insert_with(|| serde_json::json!({
            "prompt_tokens": 0u64,
            "completion_tokens": 0u64,
            "cache_hit": 0u64,
            "cache_miss": 0u64,
            "calls": 0u64,
        }));
        day["prompt_tokens"] = serde_json::json!(day["prompt_tokens"].as_u64().unwrap_or(0) + prompt);
        day["completion_tokens"] = serde_json::json!(day["completion_tokens"].as_u64().unwrap_or(0) + completion);
        day["cache_hit"] = serde_json::json!(day["cache_hit"].as_u64().unwrap_or(0) + hit);
        day["cache_miss"] = serde_json::json!(day["cache_miss"].as_u64().unwrap_or(0) + miss);
        day["calls"] = serde_json::json!(day["calls"].as_u64().unwrap_or(0) + 1);
    }

    // Build daily array sorted by date, filling gaps with zeros
    let mut daily_arr = Vec::new();
    let mut totals = serde_json::json!({
        "prompt_tokens": 0u64,
        "completion_tokens": 0u64,
        "cache_hit": 0u64,
        "cache_miss": 0u64,
        "calls": 0u64,
    });

    // Generate all dates in range
    for d in 0..days {
        let date = days_before_today(days - 1 - d); // start from cutoff, go forward
        let entry = daily.get(&date).cloned().unwrap_or_else(|| serde_json::json!({
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "cache_hit": 0,
            "cache_miss": 0,
            "calls": 0,
        }));
        for key in &["prompt_tokens", "completion_tokens", "cache_hit", "cache_miss", "calls"] {
            let v = entry[key].as_u64().unwrap_or(0);
            totals[key] = serde_json::json!(totals[key].as_u64().unwrap_or(0) + v);
        }
        daily_arr.push(serde_json::json!({
            "date": date,
            "prompt_tokens": entry["prompt_tokens"],
            "completion_tokens": entry["completion_tokens"],
            "cache_hit": entry["cache_hit"],
            "cache_miss": entry["cache_miss"],
            "calls": entry["calls"],
        }));
    }

    // Compute cache hit percentage
    let total_hit = totals["cache_hit"].as_u64().unwrap_or(0);
    let total_miss = totals["cache_miss"].as_u64().unwrap_or(0);
    let total_cache = total_hit + total_miss;
    let hit_pct = if total_cache > 0 {
        (total_hit as f64 / total_cache as f64 * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };
    totals["cache_hit_pct"] = serde_json::json!(hit_pct);
    // Remove raw hit/miss from totals (keep only percentage)
    totals.as_object_mut().map(|o| { o.remove("cache_hit"); o.remove("cache_miss"); });

    let result = serde_json::json!({
        "daily": daily_arr,
        "totals": totals,
    });
    serde_json::to_string(&result).map_err(|e| format!("serialize: {e}"))
}

/// Generate the daily array for the given range, all zeroed.
fn generate_date_range(days: u32) -> Vec<serde_json::Value> {
    let mut arr = Vec::new();
    for d in 0..days {
        let date = days_before_today(days - 1 - d);
        arr.push(serde_json::json!({
            "date": date,
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "cache_hit": 0,
            "cache_miss": 0,
            "calls": 0,
        }));
    }
    arr
}

/// Compute the date string `days` days before today (UTC-based).
fn days_before_today(days: u32) -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs().saturating_sub((days as u64) * 86400);
    let epoch_days = total_secs / 86400;
    let (y, m, d) = deepx_types::platform::civil_from_days(epoch_days as i64);
    format!("{y:04}-{m:02}-{d:02}")
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
        Agent2Ui::ExecProgress { .. } => "exec_progress",
        Agent2Ui::ToolCallPreview { .. } => "tool_call_preview",
        Agent2Ui::CodeDelta { .. } => "code_delta",
        _ => "unknown",
    }
}

// ── PLAN Review commands ────────────────────────────────────────────

/// Read PLAN.md from .deepx/ and return parsed plan items as JSON.
/// Returns empty array if PLAN.md doesn't exist or workspace is not set.
#[tauri::command]
pub fn cmd_read_plan(seed: String) -> Result<String, String> {
    if seed.is_empty() {
        return Ok("[]".into());
    }
    let ws = match crate::agent_bridge::cmd_get_workspace(seed) {
        Ok(w) if !w.is_empty() => w,
        _ => return Ok("[]".into()),
    };
    let plan_path = std::path::Path::new(&ws).join(".deepx").join("PLAN.md");
    let content = match std::fs::read_to_string(&plan_path) {
        Ok(c) => c,
        Err(_) => return Ok("[]".into()),
    };
    let items = parse_plan_items(&content);
    // Manual JSON serialization (avoid serde derive dependency)
    let json_items: Vec<serde_json::Value> = items.into_iter().map(|item| {
        serde_json::json!({
            "id": item.id,
            "title": item.title,
            "status": item.status,
            "comment": item.comment,
            "actions": item.actions,
        })
    }).collect();
    serde_json::to_string(&json_items).map_err(|e| format!("serialize: {e}"))
}

/// Write a plan action (approve/reject/ask) back to PLAN.md by updating
/// the checklist status marker. Format: `- [✓] P1: ...` or `- [-] P1: ... | reason`
#[tauri::command]
pub fn cmd_plan_action(app: AppHandle, seed: String, item_id: String, action: String, user_comment: String) -> Result<(), String> {
    if seed.is_empty() {
        return Err("No active session".into());
    }
    let ws = crate::agent_bridge::cmd_get_workspace(seed.clone())?;
    if ws.is_empty() {
        return Err("No workspace set".into());
    }
    let plan_path = std::path::Path::new(&ws).join(".deepx").join("PLAN.md");
    let content = std::fs::read_to_string(&plan_path)
        .map_err(|e| format!("read PLAN.md: {e}"))?;

    // Find checklist line matching "- [ ] P1:" or "- [✓] P1:" etc.
    let mut found = false;
    let new_content: String = content.lines().map(|line| {
        let trimmed = line.trim();
        if !found && trimmed.starts_with("- [") && trimmed.contains(&format!(" {}: ", item_id)) {
            found = true;
            match action.as_str() {
                "approve" => line.replacen("- [", "- [✓", 1),
                "reject" => {
                    let base = line.replacen("- [", "- [-", 1);
                    if user_comment.is_empty() { base } else { format!("{base} | {user_comment}") }
                },
                "ask" => line.replacen("- [", "- [?", 1),
                _ => format!("{line} | {user_comment}"),
            }
        } else {
            line.to_string()
        }
    }).collect::<Vec<_>>().join("\n");

    if !found {
        return Err(format!("Plan item '{}' not found in PLAN.md", item_id));
    }

    std::fs::write(&plan_path, new_content).map_err(|e| format!("write PLAN.md: {e}"))?;

    // Notify frontend that PLAN.md changed
    let _ = app.emit("plan-changed", serde_json::json!({"seed": seed}));

    Ok(())
}

/// Parse PLAN.md checklist format into structured items.
/// Format: `- [ ] P1: Title — Description。Deps: none。Effort: 2h | comment`
struct PlanItem {
    id: String,       // e.g. "P1"
    title: String,    // e.g. "审计持久化"
    status: String,   // "pending", "approved", "rejected", or "ask"
    comment: String,  // text after `|`
    actions: Vec<String>, // kept for frontend compatibility
}

fn parse_plan_items(content: &str) -> Vec<PlanItem> {
    let mut items = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- [") { continue; }

        // Extract status marker
        let status = if let Some(bracket_end) = trimmed.find("] ") {
            let inner = &trimmed[3..bracket_end];
            match inner {
                "✓" | "x" | "X" => "approved",
                "-" => "rejected",
                "?" => "ask",
                _ => "pending",
            }
        } else { continue };

        // Extract body after "] "
        let body = match trimmed.split_once("] ") {
            Some((_, b)) => b,
            None => continue,
        };

        // Split: "P1: Title — Description。Deps: ...。Effort: ... | comment"
        let (id, rest) = match body.split_once(": ") {
            Some((i, r)) => (i.trim().to_string(), r.trim()),
            None => continue,
        };

        // Extract title (before ' — ')
        let (title, tail) = match rest.split_once(" — ") {
            Some((t, r)) => (t.trim().to_string(), r.to_string()),
            None => (rest.to_string(), String::new()),
        };

        // Extract comment (after last '|')
        let (_description, comment) = if let Some(pos) = tail.rfind(" | ") {
            (tail[..pos].trim().to_string(), tail[pos+3..].trim().to_string())
        } else {
            (tail, String::new())
        };

        items.push(PlanItem {
            id,
            title,
            status: status.to_string(),
            comment: comment.clone(),
            actions: if comment.is_empty() { Vec::new() } else { vec![comment] },
        });
    }
    items
}

/// Map a Ui2Agent variant to a short name for logging.
fn agent2ui_event_name_for_ui(event: &Ui2Agent) -> &'static str {
    match event {
        Ui2Agent::UserInput { .. } => "user_input",
        Ui2Agent::ToolCall { .. } => "tool_call",
        Ui2Agent::CreateSession => "create_session",
        Ui2Agent::Cancel => "cancel",
        Ui2Agent::Shutdown => "shutdown",
        Ui2Agent::ReloadConfig => "reload_config",
        Ui2Agent::UndoTurn { .. } => "undo_turn",
        Ui2Agent::Compact => "compact",
        Ui2Agent::ResumeSession { .. } => "resume_session",
        Ui2Agent::NewSession => "new_session",
        Ui2Agent::LoadMoreTurns { .. } => "load_more_turns",
        _ => "unknown",
    }
}
