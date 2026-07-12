//! deepx-msglp: message-loop driver for the agent child process.
//!
//! The [`Loop`] reads [`Ui2Agent`] frames via a channel fed by a background
//! I/O thread, and writes [`Agent2Ui`] frames to a channel consumed by a
//! background writer thread. It drives the full user-input → gate → tools →
//! response pipeline.
//!
//! Responsibilities:
//!   1. Ingest [`Ui2Agent`] frames via channel (background I/O thread)
//!   2. Drive `UserInput` through gate → message → tools
//!   3. Propagate `Cancel` via [`CancelToken`] / `Arc<AtomicBool>`
//!   4. Emit all [`Agent2Ui`] responses via channel
//!   5. Handle session lifecycle (CreateSession, ResumeSession, Shutdown)
//!   6. Check for interrupt commands between rounds (Cancel, session switch)

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

pub mod agent;
use agent::AgentState;
mod conflict;
mod dashboard;
mod lifecycle;
pub mod logger;
mod notification;
mod permission;
mod tool_exec;
mod turn;
#[cfg(windows)]
mod toast_com;
pub mod util;
pub mod new; // new Ring-architecture Loop (primary)
use dashboard::{build_documents, build_recent_edits, build_tasks};
use deepx_proto::{Agent2Ui, Ui2Agent};

/// Number of recent turns sent on session restore for incremental loading.
const INITIAL_LOAD_COUNT: usize = 20;

// ═══════════════════════════════════════════════════════
// CancelToken — shared abort flag
// ═══════════════════════════════════════════════════════

#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.inner.store(false, Ordering::SeqCst);
    }

    pub fn is_set(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }

    pub fn arc(&self) -> Arc<AtomicBool> {
        self.inner.clone()
    }
}

// ═══════════════════════════════════════════════════════
// LoopPhase — what's currently running
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
enum LoopPhase {
    Idle,
    GateRunning,
    ToolsRunning,
}

// ═══════════════════════════════════════════════════════
// Loop — channel-based event loop.
//
// Background I/O threads handle stdin/stdout; the main loop
// uses mpsc channels. This allows Cancel and session-switch
// commands to arrive while the loop is busy processing.
// ═══════════════════════════════════════════════════════

pub struct Loop {
    agent: AgentState,
    cmd_rx: mpsc::Receiver<Ui2Agent>,
    event_tx: mpsc::SyncSender<Agent2Ui>,
    cancel: CancelToken,
    phase: LoopPhase,
    /// Pending session switch requested while busy (seed to resume).
    pending_session: Option<String>,
    /// Pending new-session request while busy.
    pending_new_session: bool,
    /// Pending shutdown.
    pending_shutdown: bool,
    /// Pending ReloadConfig requested while busy (workspace/config change).
    pending_reload_config: bool,
    /// Accumulated code deltas (flushed on save_full/save_append).
    code_stats: Vec<deepx_proto::CodeDeltaRecord>,
    /// Set to true when the writer thread dies (stdout pipe broken).
    /// The main loop checks this and exits gracefully.
    writer_dead: Arc<AtomicBool>,
    /// Dedicated notification thread to keep COM alive across notifications.
    notify: notification::NotificationThread,
    /// Agent operating mode (0=Normal, 1=Plan, 2=Code).
    mode: u8,
    /// Tool calls waiting for permission approval from user dialog.
    /// Keyed by tool_call_id for O(1) lookup on PermissionResponse.
    pending_approvals: HashMap<String, PendingApproval>,
    /// Persisted trusted folders for cross-workspace access (Level 3).
    trusted_folders: deepx_tools::permission::TrustedFolderSet,
    /// Saved LLM turn state when suspended for pending permission approvals.
    saved_turn: Option<TurnResumeState>,
}

/// Tool call suspended while waiting for user permission.
/// Holds the immutable challenge — only the stored fields are used for
/// authorization; the approval response must not supply replacement values.
struct PendingApproval {
    challenge: deepx_tools::bridge::PermissionChallenge,
    is_llm_tool: bool,
}

/// Saved state to resume an LLM turn after all pending permission
/// approvals have been resolved.
#[allow(dead_code)]
struct TurnResumeState {
    session_id: String,
    turn_id: String,
    round_num: u32,
    pending_call_ids: Vec<String>,
    usage: Option<deepx_types::UsageInfo>,
}
impl Loop {
    /// Create a Loop backed by real stdin/stdout via background I/O threads.
    ///
    /// Spawns:
    /// - a reader thread that reads JSON-LP from `input` and sends to `cmd_rx`
    /// - a writer thread that receives from `event_tx` and writes JSON-LP to `output`
    ///
    /// For Cancel frames, the reader thread also sets the CancelToken directly
    /// so that an in-progress handle_user_input round exits immediately.
    pub fn new_ipc(
        agent: AgentState,
        input: impl BufRead + Send + 'static,
        output: impl Write + Send + 'static,
    ) -> Self {
        let cancel = CancelToken::new();
        let cancel_for_reader = cancel.clone();

        let (cmd_tx, cmd_rx) = mpsc::sync_channel::<Ui2Agent>(4096);
        let (event_tx, event_rx) = mpsc::sync_channel::<Agent2Ui>(65536);
        let writer_dead = Arc::new(AtomicBool::new(false));
        let writer_dead_for_thread = writer_dead.clone();

        // Reader thread: stdin → cmd_tx
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut reader = std::io::BufReader::new(input);
                loop {
                    match deepx_proto::read_frame(&mut reader) {
                        Ok(Some(frame)) => {
                            let is_interrupt = matches!(
                                frame,
                                Ui2Agent::Cancel
                                    | Ui2Agent::ResumeSession { .. }
                                    | Ui2Agent::NewSession
                                    | Ui2Agent::Shutdown
                            );
                            if is_interrupt {
                                // Set cancel token directly so busy loops see it immediately
                                cancel_for_reader.set();
                                deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                            }
                            // Send through channel for the main loop to handle
                            if cmd_tx.send(frame).is_err() {
                                break; // Loop dropped
                            }
                        }
                        Ok(None) | Err(_) => {
                            log::warn!("[AGENT] reader thread: stdin EOF or read error — exiting");
                            break;
                        }
                    }
                }
            }));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".into()
                };
                log::error!("[AGENT] reader thread panicked: {}", msg);
                eprintln!("[DEEPX AGENT] reader thread panicked: {}", msg);
            }
            log::info!("[AGENT] reader thread exiting");
        });

        // Writer thread: event_rx → stdout (periodic flush, not per-frame)
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut writer = std::io::BufWriter::with_capacity(65536, output);
                let flush_interval = std::time::Duration::from_millis(2);
                loop {
                    match event_rx.recv_timeout(flush_interval) {
                        Ok(event) => {
                            let json = match serde_json::to_string(&event) {
                                Ok(j) => j,
                                Err(e) => {
                                    log::error!("[AGENT] writer thread: serialize error: {e}");
                                    continue;
                                }
                            };
                            if let Err(e) = writeln!(writer, "{}", json) {
                                log::error!("[AGENT] writer thread: write error: {e}");
                                break;
                            }
                            // Drain any backlog without flushing each
                            while let Ok(event) = event_rx.try_recv() {
                                let json = match serde_json::to_string(&event) {
                                    Ok(j) => j,
                                    Err(_) => continue,
                                };
                                if writeln!(writer, "{}", json).is_err() {
                                    break;
                                }
                            }
                            let _ = writer.flush(); // flush after each batch
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Periodic flush even when idle
                            let _ = writer.flush();
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            let _ = writer.flush();
                            break;
                        }
                    }
                }
            }));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".into()
                };
                log::error!("[AGENT] writer thread panicked: {}", msg);
                eprintln!("[DEEPX AGENT] writer thread panicked: {}", msg);
            }
            writer_dead_for_thread.store(true, Ordering::SeqCst);
            log::info!("[AGENT] writer thread exiting");
        });

        Loop {
            agent,
            cmd_rx,
            event_tx,
            cancel,
            phase: LoopPhase::Idle,
            pending_session: None,
            pending_new_session: false,
            pending_shutdown: false,
            pending_reload_config: false,
            code_stats: Vec::new(),
            writer_dead,
            notify: notification::NotificationThread::spawn(),
            mode: 0,
            pending_approvals: HashMap::new(),
            trusted_folders: deepx_tools::permission::TrustedFolderSet::load(""),
            saved_turn: None,
        }
    }

    /// Send a critical event (blocking — must be delivered).
    fn emit(&self, event: Agent2Ui) {
        if self.writer_dead.load(Ordering::SeqCst) {
            // Writer thread already dead — this event and all future events
            // will be silently dropped. The main loop will detect this on its
            // next idle check and exit.
            return;
        }
        if self.event_tx.send(event).is_err() {
            // Receiver dropped — writer thread died.
            log::error!("[AGENT] emit failed: writer thread dead (event_tx disconnected)");
            // Don't set writer_dead here — the writer thread sets it when it exits.
            // The event_tx.send() failure already means the receiver is gone.
        }
    }

    /// Send a delta event (blocking — waits if channel full, to avoid dropped frames).
    /// Use for streaming content that must not be lost (RoundDelta, ExecProgress).
    fn emit_delta(&self, event: Agent2Ui) {
        let _ = self.event_tx.send(event);
    }

    /// Drain all pending commands from the channel (non-blocking).
    /// Interrupt-type commands (Cancel, ResumeSession, NewSession, Shutdown)
    /// are handled immediately. Other commands are dispatched immediately
    /// UNLESS a session switch is pending — in that case, non-interrupt
    /// commands are dropped (the frontend re-sends them after Ready).
    fn drain_pending(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Ui2Agent::Cancel => {
                    self.cancel.set();
                    deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                    if self.phase == LoopPhase::ToolsRunning {
                        deepx_tools::bridge::cancel_current_tool();
                    }
                    self.phase = LoopPhase::Idle;
                    self.emit(Agent2Ui::Cancelled);
                }
                Ui2Agent::ResumeSession { seed } => {
                    self.cancel.set();
                    deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                    self.pending_session = Some(seed);
                    self.emit(Agent2Ui::Cancelled);
                }
                Ui2Agent::NewSession => {
                    self.cancel.set();
                    deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                    self.pending_new_session = true;
                    self.emit(Agent2Ui::Cancelled);
                }
                Ui2Agent::Shutdown => {
                    self.invalidate_pending_authorizations();
                    self.pending_shutdown = true;
                }
                // If a session switch is pending, drop non-interrupt commands
                // to prevent dispatching them to the wrong (old) session.
                // The frontend re-sends UserInput after receiving Ready.
                _other if self.pending_session.is_some() || self.pending_new_session => {
                    log::info!(
                        "[AGENT] dropping non-interrupt command during pending session switch"
                    );
                }
                // For commands that arrive while idle, dispatch immediately
                other => self.dispatch(other),
            }
        }
    }

    /// Check for interrupt commands during long-running operations.
    /// Returns true if the current operation should abort.
    fn check_interrupts(&mut self) -> bool {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Ui2Agent::Cancel => {
                    self.cancel.set();
                    deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                    if self.phase == LoopPhase::ToolsRunning {
                        deepx_tools::bridge::cancel_current_tool();
                    }
                    self.phase = LoopPhase::Idle;
                    self.emit(Agent2Ui::Cancelled);
                    return true;
                }
                Ui2Agent::ResumeSession { seed } => {
                    self.cancel.set();
                    deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                    self.pending_session = Some(seed);
                    self.emit(Agent2Ui::Cancelled);
                    return true;
                }
                Ui2Agent::NewSession => {
                    self.cancel.set();
                    deepx_tools::CANCEL.store(true, Ordering::SeqCst);
                    self.pending_new_session = true;
                    self.emit(Agent2Ui::Cancelled);
                    return true;
                }
                Ui2Agent::Shutdown => {
                    self.invalidate_pending_authorizations();
                    self.pending_shutdown = true;
                    return true;
                }
                Ui2Agent::ReloadConfig => {
                    // Queue for processing when back to idle — do NOT interrupt
                    // the current operation (workspace/config reload is non-destructive).
                    self.pending_reload_config = true;
                }
                // Queue non-interrupt commands for later
                _ => {
                    // Silently drop non-interrupt commands during busy processing;
                    // they will be re-sent by the frontend after Ready.
                    log::info!("[AGENT] dropping non-interrupt command during busy phase");
                }
            }
        }
        false
    }

    /// Dispatch a single command (called when idle).
    fn dispatch(&mut self, frame: Ui2Agent) {
        if self.saved_turn.is_some() {
            match &frame {
                Ui2Agent::PermissionResponse { .. }
                | Ui2Agent::Cancel
                | Ui2Agent::ResumeSession { .. }
                | Ui2Agent::NewSession
                | Ui2Agent::Shutdown => {}
                _ => {
                    log::warn!("[AGENT] dropping command during suspended turn");
                    self.emit(Agent2Ui::Error {
                        message: "Turn is suspended — resolve pending permissions first.".into(),
                    });
                    return;
                }
            }
        }
        match frame {
            Ui2Agent::UserInput { text } => {
                self.handle_user_input(&text);
            }
            Ui2Agent::Cancel => {
                self.handle_cancel();
            }
            Ui2Agent::CreateSession => {
                self.handle_create_session();
            }
            Ui2Agent::ResumeSession { ref seed } => {
                self.handle_resume_session(seed);
            }
            Ui2Agent::LoadMoreTurns {
                ref before_turn_id,
                count,
            } => {
                let total = self.agent.msg.turn_count();
                let idx: usize = before_turn_id
                    .strip_prefix('t')
                    .and_then(|n| n.parse::<usize>().ok())
                    .map(|n| n.saturating_sub(1))
                    .unwrap_or(total);
                let end = idx.min(total);
                let start = end.saturating_sub(count as usize);
                let batch =
                    util::build_turns_from_context(&self.agent, Some(start), Some(count as usize));
                self.emit(Agent2Ui::MoreTurns {
                    turns: batch,
                    has_more: start > 0,
                });
            }
            Ui2Agent::NewSession => {
                self.handle_create_session();
            }
            Ui2Agent::ReloadConfig => {
                self.handle_reload_config();
            }
            Ui2Agent::Shutdown => {
                self.invalidate_pending_authorizations();
                self.flush_meta_and_stats();
                self.emit(Agent2Ui::ShutdownAck);
                self.pending_shutdown = true;
            }
            Ui2Agent::ToolCall {
                id,
                name,
                action,
                args,
            } => {
                self.handle_tool_call(&id, &name, &action, &args);
            }
            Ui2Agent::UndoTurn { ref turn_id } => {
                self.handle_undo_turn(turn_id);
            }
            Ui2Agent::Compact => {
                self.handle_compact();
            }
            Ui2Agent::SetMode { ref mode } => {
                let m: u8 = match mode.as_str() {
                    "plan" => 1,
                    "code" => 2,
                    _ => 0,
                };
                self.mode = m;
                deepx_tools::bridge::set_mode(m);
                // Persist mode to session meta so it survives agent restart
                if !self.agent.session.seed.is_empty() {
                    deepx_session::SessionManager::global()
                        .persist_mode(&self.agent.session.seed, m);
                }
                log::info!("[AGENT] mode set to {mode} (internal={m})");
            }
            Ui2Agent::PermissionResponse {
                tool_call_id,
                approved,
                trust_folder,
            } => {
                log::info!(
                    "[AGENT] permission_response id={tool_call_id} approved={approved} trust={trust_folder}"
                );
                self.handle_permission_response(&tool_call_id, approved, trust_folder);
            }
            _ => {}
        }
    }

    pub fn run(&mut self) {
        self.agent.rebind_store();

        // Auto-init: if seed is pre-set (from --seed or --resume-seed CLI args),
        // create or resume the session immediately instead of waiting for IPC commands.
        let resume_seed = self.agent.session.resume_seed.take();
        let has_seed = !self.agent.session.seed.is_empty();

        if let Some(seed) = resume_seed {
            self.handle_resume_session(&seed);
            self.emit(Agent2Ui::Ready);
        } else if has_seed && !self.agent.session.from_resume {
            // New session with pre-set seed (from --seed)
            lifecycle::create_session_with_seed(&mut self.agent);
            self.agent.rebind_store();
            self.emit(Agent2Ui::SessionCreated {
                seed: self.agent.session.seed.clone(),
            });
            self.emit_dashboard();
            self.emit(Agent2Ui::Ready);
        } else {
            self.emit_dashboard();
            self.emit(Agent2Ui::Ready);
        }

        log::info!("[AGENT] entering main event loop, waiting for Ui2Agent...");
        loop {
            // Process any queued commands first
            self.drain_pending();

            // Handle pending session switch (set during busy period)
            if let Some(seed) = self.pending_session.take() {
                self.handle_resume_session(&seed);
                self.emit(Agent2Ui::Ready);
            }
            if self.pending_new_session {
                self.pending_new_session = false;
                self.handle_create_session();
                self.emit(Agent2Ui::Ready);
            }
            if self.pending_shutdown {
                break;
            }
            if self.pending_reload_config {
                self.pending_reload_config = false;
                self.handle_reload_config();
            }

            // Signal readiness before blocking (for Tauri refresh recovery).
            // Use emit_delta: if channel is full, drop it — Done already implies
            // readiness. Only startup/reconnect need guaranteed delivery.
            self.emit_delta(Agent2Ui::Ready);

            // Check if the writer thread has died (stdout pipe broken).
            // This catches cases where the agent is still processing commands
            // but can no longer communicate with the frontend.
            if self.writer_dead.load(Ordering::SeqCst) {
                log::error!("[AGENT] writer thread died — stdout pipe broken. Exiting main loop.");
                eprintln!("[DEEPX AGENT] writer thread died — stdout pipe broken. Exiting.");
                break;
            }

            // Block waiting for next command
            let frame: Ui2Agent = match self.cmd_rx.recv() {
                Ok(f) => {
                    log::info!("[AGENT] received Ui2Agent frame");
                    f
                }
                Err(_) => {
                    // cmd_rx closed — the reader thread exited, meaning stdin pipe broke.
                    // Log detailed exit reason for debugging agent kill issues.
                    log::error!(
                        "[AGENT] cmd_rx closed — reader thread stopped, stdin pipe broken. Exiting main loop. pending_shutdown={}",
                        self.pending_shutdown
                    );
                    eprintln!(
                        "[DEEPX AGENT] stdin pipe broken — exiting. pending_shutdown={}",
                        self.pending_shutdown
                    );
                    break;
                }
            };

            // Wrap dispatch in catch_unwind so a panic in any command handler
            // (UserInput, Cancel, etc.) doesn't silently kill the agent process.
            // The panic is logged and the main loop exits cleanly.
            let dispatch_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.dispatch(frame);
            }));
            if let Err(e) = dispatch_result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".into()
                };
                log::error!("[AGENT] main loop panic during dispatch: {}", msg);
                eprintln!("[DEEPX AGENT] main loop panic during dispatch: {}", msg);
                let _ = self.event_tx.try_send(Agent2Ui::Error {
                    message: format!("Agent main loop panicked: {}", msg),
                });
                break;
            }
        }

        deepx_tools::bridge::shutdown_tools();
        self.flush_meta_and_stats();
    }

    fn flush_code_stats(&mut self) {
        if self.code_stats.is_empty() {
            return;
        }
        let seed = &self.agent.session.seed;
        if seed.is_empty() {
            return;
        }
        let dir = deepx_types::platform::sessions_dir().join(seed);
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("code_stats.jsonl");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            for delta in self.code_stats.drain(..) {
                let line = serde_json::to_string(&delta).unwrap_or_default();
                let _ = writeln!(f, "{line}");
            }
            let _ = f.flush();
            let _ = f.sync_all();
        }
    }

    fn flush_meta_and_stats(&mut self) {
        self.agent.msg.flush_meta(
            &self.agent.config.model,
            &self.agent.config.reasoning_effort,
        );
        self.flush_code_stats();
    }

    /// Drain tool progress channel with batched emission (at most every 50ms).
    /// Returns true if cancelled during drain.
    fn drain_tool_progress(
        &mut self,
        progress_rx: std::sync::mpsc::Receiver<(String, String)>,
    ) -> bool {
        tool_exec::drain_tool_progress(self, progress_rx)
    }

    fn invalidate_pending_authorizations(&mut self) {
        permission::invalidate_pending_authorizations(self);
    }

    fn handle_cancel(&mut self) {
        self.cancel.set();
        deepx_tools::CANCEL.store(true, Ordering::SeqCst);
        self.invalidate_pending_authorizations();
        match self.phase {
            LoopPhase::ToolsRunning => {
                deepx_tools::bridge::cancel_current_tool();
            }
            _ => {}
        }
        self.phase = LoopPhase::Idle;
        self.emit(Agent2Ui::Cancelled);
    }

    fn handle_create_session(&mut self) {
        lifecycle::create_session(&mut self.agent);
        self.agent.rebind_store();
        self.invalidate_pending_authorizations();
        deepx_tools::bridge::set_runtime_context(
            &self.agent.session.seed,
            self.agent.config.permission_level,
        );
        self.emit(Agent2Ui::SessionCreated {
            seed: self.agent.session.seed.clone(),
        });
        self.emit_dashboard();
    }

    // Slice to the latest INITIAL_LOAD_COUNT turns for incremental loading.
    fn handle_resume_session(&mut self, seed: &str) {
        log::info!("[AGENT] handle_resume_session seed={seed}");
        if lifecycle::init_session(&mut self.agent, Some(seed)) {
            log::info!(
                "[AGENT] init_session succeeded, current_seed={}",
                self.agent.session.seed
            );
            self.agent.rebind_store();
            self.invalidate_pending_authorizations();
            deepx_tools::bridge::set_runtime_context(
                &self.agent.session.seed,
                self.agent.config.permission_level,
            );
            // Restore persisted agent mode (PLAN/CODE) from session meta
            let saved_mode = self.agent.session.mode;
            if saved_mode != 0 {
                self.mode = saved_mode;
                deepx_tools::bridge::set_mode(saved_mode);
                log::info!("[AGENT] restored mode={} from session meta", saved_mode);
            }
            // Reload trusted folders for the resumed session
            self.trusted_folders =
                deepx_tools::permission::TrustedFolderSet::load(&self.agent.session.seed);
            let current_seed = self.agent.session.seed.clone();
            if current_seed == seed {
                let total = self.agent.msg.turn_count() as u32;
                let start = total.saturating_sub(INITIAL_LOAD_COUNT as u32) as usize;
                let recent = util::build_turns_from_context(
                    &self.agent,
                    Some(start),
                    Some(INITIAL_LOAD_COUNT),
                );
                let has_more = start > 0;
                log::info!(
                    "[AGENT] sending SessionRestored, turns.len={} (total={}, has_more={})",
                    recent.len(),
                    total,
                    has_more
                );
                self.emit(Agent2Ui::SessionRestored {
                    seed: current_seed,
                    turns: recent,
                    tokens_used: 0,
                    cache_hit_pct: 0.0,
                    total_turns: total,
                    has_more,
                });
            } else {
                log::info!(
                    "[AGENT] seed changed {} -> {}, sending SessionCreated",
                    seed,
                    current_seed
                );
                self.emit(Agent2Ui::SessionCreated { seed: current_seed });
            }
            self.emit_dashboard();
        } else {
            log::info!("[AGENT] init_session returned false");
            self.emit(Agent2Ui::Error {
                message: format!("Failed to resume session: {seed}"),
            });
        }
    }

    fn handle_reload_config(&mut self) {
        if let Ok(cfg) = deepx_config::Config::load() {
            self.agent.config.api_key = cfg.api_key;
            self.agent.config.model = cfg.model;
            self.agent.config.base_url = cfg.base_url;
            self.agent.config.endpoint = cfg.endpoint;
            self.agent.config.provider_id = cfg.provider_id;
            self.agent.config.reasoning_effort = cfg.reasoning_effort;
            self.agent.config.max_tokens = cfg.max_tokens;
            self.agent.config.context_limit = cfg.context_limit;
            self.agent.config.permission_level = cfg.permission_level;
            if let Some(ref key) = cfg.context7_api_key {
                if !key.is_empty() {
                    deepx_tools::bridge::set_context7_key(key);
                }
            }
            deepx_tools::bridge::load_workspace(&self.agent.session.seed);
            // Hot-reload Turso mirror setting (no restart needed)
            deepx_session::SessionManager::global().set_turso_enabled(cfg.database.enabled);
        }
    }

    fn handle_tool_call(&mut self, id: &str, name: &str, action: &str, args: &serde_json::Value) {
        tool_exec::handle_tool_call(self, id, name, action, args);
    }

    fn handle_permission_response(
        &mut self,
        tool_call_id: &str,
        approved: bool,
        trust_folder: bool,
    ) {
        permission::handle_permission_response(self, tool_call_id, approved, trust_folder);
    }

    fn resume_saved_turn(&mut self) {
        turn::resume_saved_turn(self);
    }

    fn run_llm_turn(
        &mut self,
        turn_id: String,
        round_num: u32,
        last_usage: Option<deepx_types::UsageInfo>,
    ) {
        turn::run_llm_turn(self, turn_id, round_num, last_usage);
    }

    fn handle_undo_turn(&mut self, turn_id: &str) {
        log::info!(
            "[AGENT] UndoTurn {turn_id} — turns before: {}",
            self.agent.msg.turn_count()
        );
        if self.agent.msg.truncate_before_turn(turn_id) {
            log::info!(
                "[AGENT] UndoTurn — truncated, turns after: {}",
                self.agent.msg.turn_count()
            );
            // Full rewrite needed — the JSONL on disk still has the truncated messages.
            self.agent.msg.snapshot_full(
                &self.agent.config.model,
                &self.agent.config.reasoning_effort,
            );
            let total = self.agent.msg.turn_count() as u32;
            let start = total.saturating_sub(INITIAL_LOAD_COUNT as u32) as usize;
            let recent =
                util::build_turns_from_context(&self.agent, Some(start), Some(INITIAL_LOAD_COUNT));
            let has_more = start > 0;
            self.emit(Agent2Ui::SessionRestored {
                seed: self.agent.session.seed.clone(),
                turns: recent,
                tokens_used: 0,
                cache_hit_pct: 0.0,
                total_turns: total,
                has_more,
            });
        } else {
            log::info!("[AGENT] UndoTurn — truncate_before_turn returned false");
        }
    }

    fn handle_compact(&mut self) {
        // Compact now handled by new::engine_compact
    }

    // ── User input handler ──

    fn handle_user_input(&mut self, text: &str) {
        if self.agent.session.seed.is_empty() {
            // Auto-create a session on first user input.
            // The frontend is responsible for ensuring this only happens
            // when the user explicitly starts a new conversation.
            log::info!("[AGENT] seed is empty — auto-creating session on first user input");
            lifecycle::create_session(&mut self.agent);
            self.agent.rebind_store();
            self.emit(Agent2Ui::SessionCreated {
                seed: self.agent.session.seed.clone(),
            });
            self.emit_dashboard();
        }

        self.cancel.clear();
        deepx_tools::CANCEL.store(false, Ordering::SeqCst);
        // Ensure the bridge permission context is current before any tool
        // executes during this turn.
        deepx_tools::bridge::set_runtime_context(
            &self.agent.session.seed,
            self.agent.config.permission_level,
        );

        // ── Compliance content guard ──
        if self.agent.config.compliance_enabled {
            let guard_result = deepx_gate::guard::content_guard(text);
            if let Err(ref reason) = guard_result {
                log::info!("[AGENT] compliance blocked: {reason}");
                self.emit(Agent2Ui::Error {
                    message: reason.clone(),
                });
                self.emit(Agent2Ui::TurnEnd {
                    turn_id: "blocked".into(),
                    stop_reason: Some("compliance_block".into()),
                    usage: None,
                });
                self.emit(Agent2Ui::Done);
                return;
            }
        }

        // ── Inject mode suffix into user text ──
        let full_text = text.to_string();

        self.agent.msg.push_user(&full_text);
        // Flush user message immediately — survive LLM crash
        self.flush_meta_and_stats();

        let turn_id = format!("t{}", self.agent.msg.turn_count());
        self.emit(Agent2Ui::TurnStart {
            turn_id: turn_id.clone(),
            user_text: text.to_string(),
        });

        self.run_llm_turn(turn_id, 0, None);

        // ── Desktop notification: response preview ──
        let preview = self
            .agent
            .msg
            .turns()
            .last()
            .and_then(|t| t.steps.last())
            .map(|s| {
                s.assistant
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        deepx_types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        if !preview.is_empty() {
            let first_20: String = preview
                .split_whitespace()
                .take(20)
                .collect::<Vec<_>>()
                .join(" ");
            let body = if preview.split_whitespace().count() > 20 {
                format!("{}...", first_20)
            } else {
                first_20
            };
            // Send to persistent notification thread (keeps COM alive).
            self.notify.notify(body);
        }
    }

    // ── Dashboard ──

    fn emit_dashboard(&self) {
        // Write live context stats to disk so ContextPanel always has fresh data.
        // Previously only written during compact, leaving the panel at zero otherwise.
        {
            let (
                chat_text,
                thinking,
                tool_calls,
                tool_results,
                tools_schema,
                system_prompt,
                thinking_blocks,
                tool_call_blocks,
            ) = self
                .agent
                .msg
                .compute_context_stats(Some(&self.agent.tool_defs));
            let stats = serde_json::json!({
                "chat_text": chat_text,
                "thinking": thinking,
                "tool_calls": tool_calls,
                "tool_results": tool_results,
                "tools_schema": tools_schema,
                "system_prompt": system_prompt,
                "thinking_blocks": thinking_blocks,
                "tool_call_blocks": tool_call_blocks,
                "messages": 0,
            });
            let stats_dir = deepx_types::platform::sessions_dir().join(&self.agent.session.seed);
            let _ = std::fs::create_dir_all(&stats_dir);
            let stats_path = stats_dir.join("context_stats.json");
            let _ = std::fs::write(&stats_path, stats.to_string());
        }

        self.emit_delta(Agent2Ui::Dashboard {
            hp_connected: true,
            session_seed: self.agent.session.seed.clone(),
            context_limit: self.agent.config.context_limit,
            tool_calls_total: 0,
            tool_failures: 0,
            current_phase: "single".into(),
            streaming: false,
            dsml_compat_count: self.agent.dsml_compat_count,
            documents: build_documents(),
            recent_edits: build_recent_edits(),
            tasks: build_tasks(),
            session_title: self.agent.session.title.clone(),
            usage: None,
            model: Some(self.agent.config.model.clone()),
        });
    }
}
