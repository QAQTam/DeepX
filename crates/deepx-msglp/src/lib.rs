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

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

pub mod agent;
use agent::AgentState;
mod dashboard;
mod lifecycle;
pub mod logger;
mod notification;
#[cfg(windows)]
mod toast_com;
pub mod util;
use dashboard::{build_documents, build_recent_edits, build_tasks};
use deepx_message::Effect;
use deepx_proto::{Agent2Ui, RoundDeltaKind, Ui2Agent};
use deepx_session::SessionManager;

/// Number of recent turns sent on session restore for incremental loading.
const INITIAL_LOAD_COUNT: usize = 20;

/// Structured template for compaction summary output.
/// Forces the LLM to produce sections that preserve file paths, errors, and next actions.
const COMPACT_TEMPLATE: &str = "\
Output exactly the Markdown structure shown inside <template> and keep the section order unchanged. \
Do not include the <template> tags in your response.\n\
<template>\n\
## Objective\n\
- [one or two brief sentences describing what the user is trying to accomplish]\n\n\
## Important Details\n\
- [constraints/preferences, decisions and why, important facts/assumptions, \
exact context needed to continue, or \"(none)\"]\n\n\
## File Inventory\n\
- Added: [new files with paths, or \"(none)\"]\n\
- Modified: [changed files with paths and what changed, or \"(none)\"]\n\
- Deleted: [removed files with paths, or \"(none)\"]\n\n\
## Decision Log\n\
- [key trade-offs made: why approach A over B, rejected alternatives and rationale; otherwise \"(none)\"]\n\n\
## Key Symbols\n\
- [function signatures, type names, trait impls, API routes, config keys that are essential to resume work; otherwise \"(none)\"]\n\n\
## Work State\n\
- Completed: [finished work, verified facts, or FILES created/modified/deleted with paths; otherwise \"(none)\"]\n\
- Active: [current work, partial changes, or investigation state; otherwise \"(none)\"]\n\
- Blocked: [blockers, errors encountered and resolutions, or unknowns; otherwise \"(none)\"]\n\n\
## Next Move\n\
1. [immediate concrete action, or \"(none)\"]\n\
2. [next action if known, or \"(none)\"]\n\
</template>\n\n\
Rules:\n\
- Keep every section, even when empty.\n\
- Use terse bullets, not prose paragraphs.\n\
- Preserve exact file paths, symbols, commands, error strings, URLs, and identifiers when known.\n\
- Put relevant files and symbols inside the section where they matter; do not add extra sections.\n\
- Do not mention the summary process or that context was compacted.";

/// Convert epoch seconds to human-readable UTC date.
fn epoch_to_date(epoch_secs: u64) -> String {
    use deepx_types::platform::civil_from_days;
    let total_days = (epoch_secs / 86400) as i64;
    let (y, m, d) = civil_from_days(total_days);
    format!("{y:04}-{m:02}-{d:02}")
}

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
/// Extract file paths that a tool writes to (mutates).
/// Returns empty vec for read-only and non-file tools.
fn file_write_paths(tool_name: &str, args: &serde_json::Value) -> Vec<String> {
    if tool_name != "file" {
        return Vec::new();
    }
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let mut paths = Vec::new();
    // All actions that modify files
    match action {
        "write" | "edit" | "edit_diff" | "delete" => {
            if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
            if let Some(arr) = args.get("paths").and_then(|v| v.as_array()) {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        paths.push(s.to_string());
                    }
                }
            }
        }
        "move" | "copy" => {
            // Both source and dest are affected; dest is the write target
            if let Some(p) = args.get("dest").and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
            if let Some(p) = args.get("source").and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
        }
        _ => {}
    }
    paths
}

/// Detect same-file write conflicts among pending tools and group them
/// into serial execution sets. Returns (serial_groups, serial_after_indices).
fn resolve_write_conflicts(
    pending: &[deepx_message::PendingTool],
) -> (Vec<Vec<usize>>, HashSet<usize>) {
    let mut file_writers: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, tool) in pending.iter().enumerate() {
        for path in file_write_paths(&tool.name, &tool.args) {
            file_writers.entry(path).or_default().push(i);
        }
    }
    let mut serial_groups: Vec<Vec<usize>> = Vec::new();
    {
        let mut visited = vec![false; pending.len()];
        for indices in file_writers.values() {
            if indices.is_empty() {
                continue;
            }
            let rep = indices[0];
            if visited[rep] {
                continue;
            }
            let mut group_set: HashSet<usize> = HashSet::new();
            let mut stack: Vec<usize> = indices.clone();
            while let Some(idx) = stack.pop() {
                if !group_set.insert(idx) {
                    continue;
                }
                visited[idx] = true;
                for other in file_writers.values() {
                    if other.contains(&idx) {
                        for &oi in other {
                            if !group_set.contains(&oi) {
                                stack.push(oi);
                            }
                        }
                    }
                }
            }
            let mut group: Vec<usize> = group_set.into_iter().collect();
            group.sort();
            if group.len() > 1 {
                serial_groups.push(group);
            }
        }
    }
    let mut serial_after: HashSet<usize> = HashSet::new();
    for group in &serial_groups {
        for &idx in &group[1..] {
            serial_after.insert(idx);
        }
    }
    (serial_groups, serial_after)
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
        log::info!("[AGENT] drain loop start");
        let mut batches: HashMap<String, String> = HashMap::new();
        let batch_interval = std::time::Duration::from_millis(50);
        loop {
            if self.cancel.is_set() || deepx_tools::CANCEL.load(Ordering::SeqCst) {
                log::info!("[AGENT] drain loop cancel");
                return true;
            }
            match progress_rx.recv_timeout(batch_interval) {
                Ok((tc_id, chunk)) => {
                    batches.entry(tc_id).or_default().push_str(&chunk);
                    while let Ok((tid, c)) = progress_rx.try_recv() {
                        batches.entry(tid).or_default().push_str(&c);
                    }
                    for (tid, merged) in batches.drain() {
                        log::info!("[AGENT] ExecProgress batch: {} {} chars", tid, merged.len());
                        self.emit_delta(Agent2Ui::ExecProgress {
                            tool_call_id: tid,
                            chunk: merged,
                        });
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !batches.is_empty() {
                        for (tid, merged) in batches.drain() {
                            self.emit_delta(Agent2Ui::ExecProgress {
                                tool_call_id: tid,
                                chunk: merged,
                            });
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    log::info!("[AGENT] drain loop disconnected");
                    for (tid, merged) in batches.drain() {
                        self.emit_delta(Agent2Ui::ExecProgress {
                            tool_call_id: tid,
                            chunk: merged,
                        });
                    }
                    return false;
                }
            }
        }
    }

    fn invalidate_pending_authorizations(&mut self) {
        self.pending_approvals.clear();
        self.saved_turn = None;
        deepx_tools::bridge::clear_runtime_context();
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
        log::info!("[AGENT] handle_tool_call: name={name} action={action} id={id}");

        if self.pending_approvals.contains_key(id) {
            self.emit(Agent2Ui::Error {
                message: format!("Duplicate or replayed tool-call ID rejected: {id}"),
            });
            return;
        }

        let effective_name = util::resolve_effective_name(name, action, args);
        log::info!("[AGENT] resolved effective_name={effective_name}");

        let level =
            deepx_tools::permission::PermissionLevel::from_u8(self.agent.config.permission_level);
        let ws_root = {
            let ws = deepx_tools::CURRENT_WORKSPACE
                .read()
                .expect("CURRENT_WORKSPACE lock")
                .clone();
            if ws.is_empty() || ws == "." {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            } else {
                std::path::PathBuf::from(ws)
            }
        };

        // Ensure the bridge permission context is set so compatibility wrappers
        // also enforce policy.
        deepx_tools::bridge::set_runtime_context(
            &self.agent.session.seed,
            self.agent.config.permission_level,
        );

        let inv = deepx_tools::bridge::ToolInvocation {
            session_id: self.agent.session.seed.clone(),
            call_id: id.to_string(),
            tool_name: effective_name.clone(),
            action: String::new(),
            args: args.clone(),
        };

        match deepx_tools::bridge::admit(
            inv,
            self.agent.config.permission_level,
            &ws_root,
            self.trusted_folders.set(),
        ) {
            deepx_tools::bridge::Admission::Authorized(authorized) => {
                self.emit_tool_result(id, &effective_name, args, authorized);
            }
            deepx_tools::bridge::Admission::ApprovalRequired(challenge) => {
                let cat_str = match challenge.category {
                    deepx_tools::permission::ToolCategory::Read => "read",
                    deepx_tools::permission::ToolCategory::Write => "write",
                    deepx_tools::permission::ToolCategory::Exec => "exec",
                    deepx_tools::permission::ToolCategory::Net => "net",
                };
                self.emit(Agent2Ui::PermissionRequest {
                    tool_call_id: challenge.call_id.clone(),
                    tool_name: challenge.tool_name.clone(),
                    reason: challenge.reason.clone(),
                    paths: challenge
                        .resources
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    category: cat_str.to_string(),
                    level: level.to_u8(),
                });
                self.pending_approvals.insert(
                    challenge.call_id.clone(),
                    PendingApproval {
                        challenge,
                        is_llm_tool: false,
                    },
                );
            }
            deepx_tools::bridge::Admission::Denied(reason) => {
                let turn_id = format!("tc_{id}");
                self.emit(Agent2Ui::TurnStart {
                    turn_id: turn_id.clone(),
                    user_text: format!("tool: {name}"),
                });
                self.emit(Agent2Ui::ToolResults {
                    turn_id: turn_id.clone(),
                    round_num: 0,
                    results: vec![deepx_proto::ToolResultDef {
                        tool_call_id: id.to_string(),
                        output: format!("[DENIED] '{name}' — {reason}",),
                        success: false,
                        file: None,
                    }],
                });
                self.emit(Agent2Ui::TurnEnd {
                    turn_id,
                    stop_reason: None,
                    usage: None,
                });
            }
        }
    }

    /// Execute an authorized tool and emit results. Split into a helper so
    /// both UI and approved-LLM paths share the same execution-and-emit logic.
    fn emit_tool_result(
        &mut self,
        id: &str,
        name: &str,
        args: &serde_json::Value,
        authorized: deepx_tools::bridge::AuthorizedToolCall,
    ) {
        let turn_id = format!("tc_{id}");
        let round_num = 0u32;

        self.emit(Agent2Ui::TurnStart {
            turn_id: turn_id.clone(),
            user_text: format!("tool: {name}"),
        });
        let args_display: String = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .chars()
            .take(80)
            .collect();
        self.emit(Agent2Ui::RoundComplete {
            turn_id: turn_id.clone(),
            round_num,
            thinking: None,
            answer: None,
            tool_calls: vec![deepx_proto::ToolCallDef {
                id: id.to_string(),
                name: name.to_string(),
                args_display: args_display.clone(),
                args_json: args.to_string(),
            }],
            blocks: vec![deepx_proto::RoundBlock::Tool {
                card: deepx_proto::ToolCallDef {
                    id: id.to_string(),
                    name: name.to_string(),
                    args_display,
                    args_json: args.to_string(),
                },
            }],
            is_final: false,
        });

        let (progress_tx, progress_rx) = std::sync::mpsc::channel::<(String, String)>();
        let tool_id = id.to_string();
        let tool_id_for_result = tool_id.clone();
        let tool_id_progress = tool_id.clone();
        let handle = std::thread::Builder::new()
            .stack_size(4 * 1024 * 1024)
            .spawn(move || {
                let result = deepx_tools::bridge::execute_authorized(authorized, Some(progress_tx));
                (tool_id, result.content, result.success, result.code_delta)
            })
            .expect("failed to spawn tool thread");

        let mut pending_chunk = String::new();
        loop {
            match progress_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok((tc_id, chunk)) => {
                    pending_chunk.push_str(&chunk);
                    while let Ok((_, c)) = progress_rx.try_recv() {
                        pending_chunk.push_str(&c);
                    }
                    self.emit_delta(Agent2Ui::ExecProgress {
                        tool_call_id: tc_id,
                        chunk: std::mem::take(&mut pending_chunk),
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !pending_chunk.is_empty() {
                        self.emit_delta(Agent2Ui::ExecProgress {
                            tool_call_id: tool_id_progress.clone(),
                            chunk: std::mem::take(&mut pending_chunk),
                        });
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let (tid, output, success, code_delta) = handle.join().unwrap_or_else(|_| {
            (
                tool_id_for_result,
                "[ERROR] tool thread panicked".into(),
                false,
                None,
            )
        });
        if let Some(ref delta) = code_delta {
            self.code_stats.push(delta.clone());
            self.emit_delta(Agent2Ui::CodeDelta {
                lines_added: delta.lines_added,
                lines_removed: delta.lines_removed,
                files_created: delta.files_created,
                files_deleted: delta.files_deleted,
                file: delta.file.clone(),
            });
        }
        self.emit(Agent2Ui::ToolResults {
            turn_id: turn_id.clone(),
            round_num,
            results: vec![deepx_proto::ToolResultDef {
                tool_call_id: tid,
                output,
                success,
                file: None,
            }],
        });
        self.emit(Agent2Ui::TurnEnd {
            turn_id: turn_id.clone(),
            stop_reason: None,
            usage: None,
        });
    }

    fn handle_permission_response(
        &mut self,
        tool_call_id: &str,
        approved: bool,
        trust_folder: bool,
    ) {
        let pending = match self.pending_approvals.remove(tool_call_id) {
            Some(p) => p,
            None => {
                log::warn!(
                    "[AGENT] PermissionResponse for unknown call_id={tool_call_id} — missing or replayed"
                );
                return;
            }
        };

        // Extract fields before consuming the challenge.
        let call_id = pending.challenge.call_id.clone();
        let tool_name = pending.challenge.tool_name.clone();
        let is_llm = pending.is_llm_tool;
        let approved_resources = pending.challenge.resources.clone();

        match pending.challenge.approve(approved) {
            Ok(authorized) => {
                if trust_folder {
                    for path in &approved_resources {
                        self.trusted_folders.trust(path.parent().unwrap_or(path));
                    }
                    log::info!("[AGENT] trusted folders updated from approved permission response");
                }
                if is_llm {
                    let result = deepx_tools::bridge::execute_authorized(authorized, None);
                    self.agent.msg.push_tool_result_direct(
                        &call_id,
                        &result.content,
                        result.success,
                    );
                    if let Some(ref delta) = result.code_delta {
                        self.code_stats.push(delta.clone());
                        self.emit_delta(Agent2Ui::CodeDelta {
                            lines_added: delta.lines_added,
                            lines_removed: delta.lines_removed,
                            files_created: delta.files_created,
                            files_deleted: delta.files_deleted,
                            file: delta.file.clone(),
                        });
                    }
                } else {
                    let args = authorized.args().clone();
                    self.emit_tool_result(&call_id, &tool_name, &args, authorized);
                }
            }
            Err(deepx_tools::bridge::ApprovalError::Rejected) => {
                if is_llm {
                    self.agent.msg.push_tool_result_direct(
                        &call_id,
                        &format!("[DENIED] '{tool_name}' (user denied permission)"),
                        false,
                    );
                } else {
                    let turn_id = format!("tc_{call_id}");
                    self.emit(Agent2Ui::TurnStart {
                        turn_id: turn_id.clone(),
                        user_text: format!("tool: {tool_name}"),
                    });
                    self.emit(Agent2Ui::ToolResults {
                        turn_id: turn_id.clone(),
                        round_num: 0,
                        results: vec![deepx_proto::ToolResultDef {
                            tool_call_id: call_id.clone(),
                            output: format!("[DENIED] '{tool_name}' (user denied permission)"),
                            success: false,
                            file: None,
                        }],
                    });
                    self.emit(Agent2Ui::TurnEnd {
                        turn_id,
                        stop_reason: None,
                        usage: None,
                    });
                }
            }
            Err(deepx_tools::bridge::ApprovalError::Expired) => {
                if is_llm {
                    self.agent.msg.push_tool_result_direct(
                        &call_id,
                        &format!("[EXPIRED] Permission approval expired for '{tool_name}'."),
                        false,
                    );
                } else {
                    log::warn!("[AGENT] permission approval expired for call_id={call_id}");
                    let turn_id = format!("tc_{call_id}");
                    self.emit(Agent2Ui::TurnStart {
                        turn_id: turn_id.clone(),
                        user_text: format!("tool: {tool_name}"),
                    });
                    self.emit(Agent2Ui::ToolResults {
                        turn_id: turn_id.clone(),
                        round_num: 0,
                        results: vec![deepx_proto::ToolResultDef {
                            tool_call_id: call_id.clone(),
                            output: format!(
                                "[EXPIRED] Permission approval expired for '{tool_name}'."
                            ),
                            success: false,
                            file: None,
                        }],
                    });
                    self.emit(Agent2Ui::TurnEnd {
                        turn_id,
                        stop_reason: None,
                        usage: None,
                    });
                }
            }
            Err(deepx_tools::bridge::ApprovalError::MissingOrReplayed) => {
                log::warn!("[AGENT] permission response for unknown or replayed call");
            }
        }

        // If this was an LLM tool approval, check if we can resume the suspended turn.
        if is_llm {
            if let Some(ref saved) = self.saved_turn {
                let all_resolved = saved
                    .pending_call_ids
                    .iter()
                    .all(|id| !self.pending_approvals.contains_key(id));
                if all_resolved {
                    log::info!(
                        "[AGENT] all pending approvals resolved for turn {}, resuming",
                        saved.turn_id
                    );
                    self.resume_saved_turn();
                }
            }
        }
    }

    fn emit_completed_tool_round(
        &mut self,
        turn_id: &str,
        round_num: u32,
    ) -> Vec<(String, String, String, bool)> {
        let results = self.agent.msg.last_step_tool_results();
        let ts = util::chrono_local_datetime();
        let tool_defs = results
            .iter()
            .map(|(tc_id, tool_name, result_content, success)| {
                let args = self
                    .agent
                    .msg
                    .tool_call_args(tc_id)
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                self.emit_delta(Agent2Ui::AuditRecord {
                    tool_name: tool_name.clone(),
                    result_summary: result_content
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(120)
                        .collect(),
                    success: *success,
                    time: ts.clone(),
                    args,
                });
                deepx_proto::ToolResultDef {
                    tool_call_id: tc_id.clone(),
                    output: result_content.clone(),
                    success: *success,
                    file: None,
                }
            })
            .collect::<Vec<_>>();

        if !tool_defs.is_empty() {
            self.emit(Agent2Ui::ToolResults {
                turn_id: turn_id.to_string(),
                round_num,
                results: tool_defs,
            });
        }
        if results.iter().any(|(_, name, _, _)| name == "plan_submit") {
            self.emit(Agent2Ui::PlanChanged);
        }
        self.emit_dashboard();
        self.flush_meta_and_stats();
        results
    }

    fn resume_saved_turn(&mut self) {
        let saved = match self.saved_turn.take() {
            Some(s) => s,
            None => return,
        };

        if saved.session_id != self.agent.session.seed {
            log::warn!(
                "[AGENT] refusing to resume turn {} from stale session {}",
                saved.turn_id,
                saved.session_id
            );
            return;
        }
        if let Err(reason) = deepx_tools::bridge::verify_active_session(&saved.session_id) {
            log::warn!(
                "[AGENT] refusing to resume turn {}: {}",
                saved.turn_id,
                reason
            );
            return;
        }

        log::info!(
            "[AGENT] resuming turn {} round {}",
            saved.turn_id,
            saved.round_num
        );

        self.emit_completed_tool_round(&saved.turn_id, saved.round_num);

        // Continue the LLM turn by re-entering the gate→tools loop
        self.run_llm_turn(saved.turn_id, saved.round_num + 1, saved.usage);
    }

    fn run_llm_turn(
        &mut self,
        turn_id: String,
        mut round_num: u32,
        mut last_usage: Option<deepx_types::UsageInfo>,
    ) {
        // Rebuild provider from current config (not from saved state)
        let ep = deepx_config::registry::find_endpoint(
            &self.agent.config.provider_id,
            &self.agent.config.endpoint,
        );
        let provider = deepx_gate::ProviderConfig::openai(
            &self.agent.config.base_url,
            &self.agent.config.api_key,
            &self.agent.config.model,
            ep.as_ref().and_then(|e| e.user_id_mode.clone()),
            ep.as_ref().and_then(|e| e.chat_path.clone()),
            ep.as_ref()
                .map(|e| e.thinking_mode.clone())
                .unwrap_or_default(),
            ep.as_ref()
                .map(|e| e.cache_field.clone())
                .unwrap_or_default(),
            ep.as_ref().map(|e| e.supports_thinking).unwrap_or(true),
        )
        .with_stateful(ep.as_ref().map(|e| e.stateful).unwrap_or(false));

        loop {
            // ── Check for interrupt commands between rounds ──
            if self.check_interrupts() {
                self.agent.msg.remove_last_step_if_incomplete();
                self.flush_meta_and_stats();
                break;
            }

            if self.cancel.is_set() || deepx_tools::CANCEL.load(Ordering::SeqCst) {
                self.agent.msg.remove_last_step_if_incomplete();
                self.flush_meta_and_stats();
                break;
            }

            // Check for pending session switch (set by check_interrupts)
            if self.pending_session.is_some() || self.pending_new_session {
                self.agent.msg.remove_last_step_if_incomplete();
                self.flush_meta_and_stats();
                break;
            }

            let messages = self.agent.build_context();

            let tools = Some(self.agent.tool_defs.clone());
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut tool_calls_raw = serde_json::Value::Null;
            let mut had_error = false;

            self.phase = LoopPhase::GateRunning;
            let cancel_arc = self.cancel.arc();
            let result = deepx_gate::chat_stream(
                &provider,
                messages,
                tools,
                self.agent.config.max_tokens,
                Some(self.agent.config.reasoning_effort.clone()),
                Some(self.agent.session.seed.clone()),
                Some(&cancel_arc),
                &mut |event| match event {
                    deepx_gate::StreamEvent::ContentDelta(d) => {
                        if self.cancel.is_set() {
                            return;
                        }
                        content.push_str(&d);
                        self.emit_delta(Agent2Ui::RoundDelta {
                            turn_id: turn_id.clone(),
                            round_num,
                            kind: RoundDeltaKind::Answering,
                            delta: d,
                        });
                    }
                    deepx_gate::StreamEvent::ReasoningDelta(r) => {
                        if self.cancel.is_set() {
                            return;
                        }
                        reasoning.push_str(&r);
                        self.emit_delta(Agent2Ui::RoundDelta {
                            turn_id: turn_id.clone(),
                            round_num,
                            kind: RoundDeltaKind::Thinking,
                            delta: r,
                        });
                    }
                    deepx_gate::StreamEvent::Done {
                        raw_message, usage, ..
                    } => {
                        if let Some(ref u) = usage {
                            self.agent.session.tokens += u.total_tokens as u64;
                            last_usage = usage.clone();
                        }
                        content.clear();
                        reasoning.clear();
                        let mut blocks: Vec<serde_json::Value> = Vec::new();
                        for block in &raw_message.content {
                            match block {
                                deepx_types::ContentBlock::Text { text } => content.push_str(text),
                                deepx_types::ContentBlock::Reasoning { reasoning: r } => {
                                    reasoning.push_str(r)
                                }
                                deepx_types::ContentBlock::ToolUse { id, name, input } => {
                                    blocks.push(serde_json::json!({
                                        "id": id,
                                        "name": name,
                                        "arguments": input.to_string(),
                                    }));
                                }
                                _ => {}
                            }
                        }
                        if !blocks.is_empty() {
                            tool_calls_raw = serde_json::Value::Array(blocks);
                        }
                    }
                    deepx_gate::StreamEvent::ToolCallProgress {
                        index,
                        id,
                        name,
                        args_so_far,
                    } => {
                        self.emit_delta(Agent2Ui::ToolCallPreview {
                            turn_id: turn_id.clone(),
                            round_num,
                            index,
                            id,
                            name,
                            args_so_far,
                        });
                    }
                    deepx_gate::StreamEvent::UsageUpdate(u) => {
                        last_usage = Some(u.clone());
                        self.agent.session.tokens =
                            self.agent.session.tokens.max(u.total_tokens as u64);
                        self.emit_delta(Agent2Ui::Dashboard {
                            hp_connected: true,
                            session_seed: self.agent.session.seed.clone(),
                            context_limit: self.agent.config.context_limit,
                            tool_calls_total: 0,
                            tool_failures: 0,
                            current_phase: "single".into(),
                            streaming: true,
                            dsml_compat_count: self.agent.dsml_compat_count,
                            documents: Vec::new(),
                            recent_edits: Vec::new(),
                            tasks: Vec::new(),
                            session_title: None,
                            usage: Some(u),
                            model: Some(self.agent.config.model.clone()),
                        });
                    }
                    deepx_gate::StreamEvent::Retrying {
                        attempt,
                        max_retries,
                        delay_secs,
                        error,
                    } => {
                        let msg = format!(
                            "API error, retrying ({attempt}/{max_retries}) in {delay_secs}s: {error}"
                        );
                        self.emit(Agent2Ui::Error { message: msg });
                    }
                    deepx_gate::StreamEvent::Error(msg) => {
                        self.emit(Agent2Ui::Error { message: msg });
                        had_error = true;
                    }
                },
            );

            if had_error || result.is_err() {
                self.flush_meta_and_stats();
                break;
            }

            if self.cancel.is_set() || deepx_tools::CANCEL.load(Ordering::SeqCst) {
                self.agent.msg.remove_last_step_if_incomplete();
                self.flush_meta_and_stats();
                break;
            }

            let parsed = util::parse_tool_calls_from_response(
                &content,
                &reasoning,
                &tool_calls_raw,
                &self.agent,
            );
            let assistant_msg = util::build_assistant_message(&content, &reasoning, &parsed);
            let effect = self.agent.msg.push_assistant(assistant_msg.clone());
            self.flush_meta_and_stats();

            util::emit_round_complete(
                &self.event_tx,
                &turn_id,
                round_num,
                &assistant_msg,
                &content,
                &reasoning,
                &parsed,
            );

            match effect {
                Effect::None => {
                    self.phase = LoopPhase::ToolsRunning;

                    let mut round_pending_ids = Vec::new();
                    let pending = self.agent.msg.get_last_step_pending();
                    if !pending.is_empty() {
                        let mut seen_call_ids = HashSet::new();
                        let duplicate_or_reused = pending.iter().any(|tool| {
                            !seen_call_ids.insert(tool.id.clone())
                                || self.pending_approvals.contains_key(&tool.id)
                        });
                        if duplicate_or_reused {
                            log::error!("[AGENT] duplicate or reused LLM tool-call id");
                            self.agent.msg.remove_last_step_if_incomplete();
                            self.emit(Agent2Ui::Error {
                                message: "Model returned a duplicate or reused tool-call ID; no tools were executed."
                                    .into(),
                            });
                            break;
                        }
                        let (serial_groups, serial_after) = resolve_write_conflicts(&pending);
                        let ws_root = {
                            let ws = deepx_tools::CURRENT_WORKSPACE
                                .read()
                                .expect("CURRENT_WORKSPACE lock")
                                .clone();
                            if ws.is_empty() || ws == "." {
                                std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                            } else {
                                std::path::PathBuf::from(ws)
                            }
                        };

                        let mut authorized: Vec<(
                            String,
                            String,
                            deepx_tools::bridge::AuthorizedToolCall,
                        )> = Vec::new();
                        for (i, tool) in pending.iter().enumerate() {
                            if serial_after.contains(&i) {
                                continue;
                            }

                            let inv = deepx_tools::bridge::ToolInvocation {
                                session_id: self.agent.session.seed.clone(),
                                call_id: tool.id.clone(),
                                tool_name: tool.name.clone(),
                                action: String::new(),
                                args: tool.args.clone(),
                            };
                            match deepx_tools::bridge::admit(
                                inv,
                                self.agent.config.permission_level,
                                &ws_root,
                                self.trusted_folders.set(),
                            ) {
                                deepx_tools::bridge::Admission::Authorized(auth) => {
                                    authorized.push((tool.id.clone(), tool.name.clone(), auth));
                                }
                                deepx_tools::bridge::Admission::ApprovalRequired(challenge) => {
                                    let cat_str = match challenge.category {
                                        deepx_tools::permission::ToolCategory::Read => "read",
                                        deepx_tools::permission::ToolCategory::Write => "write",
                                        deepx_tools::permission::ToolCategory::Exec => "exec",
                                        deepx_tools::permission::ToolCategory::Net => "net",
                                    };
                                    let call_id = challenge.call_id.clone();
                                    self.emit(Agent2Ui::PermissionRequest {
                                        tool_call_id: call_id.clone(),
                                        tool_name: challenge.tool_name.clone(),
                                        reason: challenge.reason.clone(),
                                        paths: challenge
                                            .resources
                                            .iter()
                                            .map(|p| p.to_string_lossy().to_string())
                                            .collect(),
                                        category: cat_str.to_string(),
                                        level: deepx_tools::permission::PermissionLevel::from_u8(
                                            self.agent.config.permission_level,
                                        )
                                        .to_u8(),
                                    });
                                    round_pending_ids.push(call_id.clone());
                                    self.pending_approvals.insert(
                                        call_id,
                                        PendingApproval {
                                            challenge,
                                            is_llm_tool: true,
                                        },
                                    );
                                }
                                deepx_tools::bridge::Admission::Denied(reason) => {
                                    self.agent.msg.push_tool_result_direct(
                                        &tool.id,
                                        &format!(
                                            "[timeis: {}]\n[DENIED] {}",
                                            util::chrono_local_datetime(),
                                            reason
                                        ),
                                        false,
                                    );
                                }
                            }
                        }

                        let (progress_tx, progress_rx) =
                            std::sync::mpsc::channel::<(String, String)>();
                        let mut handles: Vec<(
                            String,
                            std::thread::JoinHandle<(
                                String,
                                String,
                                bool,
                                Option<deepx_proto::CodeDeltaRecord>,
                            )>,
                        )> = Vec::new();
                        let mut tool_infos = Vec::new();

                        for (tc_id, tool_name, auth_call) in authorized {
                            let tx = progress_tx.clone();
                            let tc_id_for_closure = tc_id.clone();
                            let tc_id_for_handle = tc_id.clone();
                            tool_infos.push((tc_id, tool_name));
                            let handle = std::thread::Builder::new()
                                .stack_size(4 * 1024 * 1024)
                                .spawn(move || {
                                    let result = deepx_tools::bridge::execute_authorized(
                                        auth_call,
                                        Some(tx),
                                    );
                                    (
                                        tc_id_for_closure,
                                        result.content,
                                        result.success,
                                        result.code_delta,
                                    )
                                })
                                .expect("failed to spawn tool thread");
                            handles.push((tc_id_for_handle, handle));
                        }
                        drop(progress_tx);

                        if !handles.is_empty() {
                            let cancelled = self.drain_tool_progress(progress_rx);

                            if cancelled {
                                log::info!(
                                    "[AGENT] cancelled, pushing placeholder results + background reaper"
                                );
                                let ts = util::chrono_local_datetime();
                                for (tc_id, _tool_name) in &tool_infos {
                                    self.agent.msg.push_tool_result_direct(
                                        tc_id,
                                        &format!("[timeis: {ts}]\n[CANCELLED]"),
                                        false,
                                    );
                                }
                                std::thread::spawn(move || {
                                    for (_id, h) in handles {
                                        let _ = h.join();
                                    }
                                });
                            } else {
                                let ts = util::chrono_local_datetime();
                                for (tc_id, h) in handles {
                                    match h.join() {
                                        Ok((_id, content, success, code_delta)) => {
                                            self.agent.msg.push_tool_result_direct(
                                                &tc_id,
                                                &format!("[timeis: {ts}]\n{content}"),
                                                success,
                                            );
                                            if let Some(ref delta) = code_delta {
                                                self.code_stats.push(delta.clone());
                                                self.emit_delta(Agent2Ui::CodeDelta {
                                                    lines_added: delta.lines_added,
                                                    lines_removed: delta.lines_removed,
                                                    files_created: delta.files_created,
                                                    files_deleted: delta.files_deleted,
                                                    file: delta.file.clone(),
                                                });
                                            }
                                        }
                                        Err(_) => {
                                            log::error!("[AGENT] tool thread panicked for {tc_id}");
                                            self.agent.msg.push_tool_result_direct(
                                                &tc_id,
                                                &format!(
                                                    "[timeis: {ts}]\n[ERROR] tool thread panicked"
                                                ),
                                                false,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // ── Execute serialized follow-up tools (same-file write conflicts) ──
                        if !serial_groups.is_empty() {
                            let ts = util::chrono_local_datetime();
                            for group in &serial_groups {
                                for &idx in &group[1..] {
                                    let tool = &pending[idx];
                                    let inv = deepx_tools::bridge::ToolInvocation {
                                        session_id: self.agent.session.seed.clone(),
                                        call_id: tool.id.clone(),
                                        tool_name: tool.name.clone(),
                                        action: String::new(),
                                        args: tool.args.clone(),
                                    };
                                    match deepx_tools::bridge::admit(
                                        inv,
                                        self.agent.config.permission_level,
                                        &ws_root,
                                        self.trusted_folders.set(),
                                    ) {
                                        deepx_tools::bridge::Admission::Authorized(auth) => {
                                            let result =
                                                deepx_tools::bridge::execute_authorized(auth, None);
                                            self.agent.msg.push_tool_result_direct(
                                                &tool.id,
                                                &format!("[timeis: {ts}]\n{}", result.content),
                                                result.success,
                                            );
                                            if let Some(ref delta) = result.code_delta {
                                                self.code_stats.push(delta.clone());
                                                self.emit_delta(Agent2Ui::CodeDelta {
                                                    lines_added: delta.lines_added,
                                                    lines_removed: delta.lines_removed,
                                                    files_created: delta.files_created,
                                                    files_deleted: delta.files_deleted,
                                                    file: delta.file.clone(),
                                                });
                                            }
                                        }
                                        deepx_tools::bridge::Admission::ApprovalRequired(
                                            challenge,
                                        ) => {
                                            let cat_str = match challenge.category {
                                                deepx_tools::permission::ToolCategory::Read => {
                                                    "read"
                                                }
                                                deepx_tools::permission::ToolCategory::Write => {
                                                    "write"
                                                }
                                                deepx_tools::permission::ToolCategory::Exec => {
                                                    "exec"
                                                }
                                                deepx_tools::permission::ToolCategory::Net => "net",
                                            };
                                            let call_id = challenge.call_id.clone();
                                            self.emit(Agent2Ui::PermissionRequest {
                                                tool_call_id: call_id.clone(),
                                                tool_name: challenge.tool_name.clone(),
                                                reason: challenge.reason.clone(),
                                                paths: challenge.resources.iter().map(|p| p.to_string_lossy().to_string()).collect(),
                                                category: cat_str.to_string(),
                                                level: deepx_tools::permission::PermissionLevel::from_u8(self.agent.config.permission_level).to_u8(),
                                            });
                                            round_pending_ids.push(call_id.clone());
                                            self.pending_approvals.insert(
                                                call_id,
                                                PendingApproval {
                                                    challenge,
                                                    is_llm_tool: true,
                                                },
                                            );
                                        }
                                        deepx_tools::bridge::Admission::Denied(reason) => {
                                            self.agent.msg.push_tool_result_direct(
                                                &tool.id,
                                                &format!("[timeis: {ts}]\n[DENIED] {reason}"),
                                                false,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if !round_pending_ids.is_empty() {
                        self.saved_turn = Some(TurnResumeState {
                            session_id: self.agent.session.seed.clone(),
                            turn_id: turn_id.clone(),
                            round_num,
                            pending_call_ids: round_pending_ids,
                            usage: last_usage.clone(),
                        });
                        log::info!(
                            "[AGENT] suspending turn {turn_id} round {round_num} for {} pending approvals",
                            self.saved_turn.as_ref().unwrap().pending_call_ids.len()
                        );
                        break;
                    }

                    let results = self.emit_completed_tool_round(&turn_id, round_num);

                    // ── ask_user: stop loop, wait for user response ──
                    let has_user_query = results.iter().any(|(_, _, content, _)| {
                        content.starts_with("[USER_QUERY]")
                            || serde_json::from_str::<serde_json::Value>(content)
                                .ok()
                                .and_then(|v| v.get("user_query").and_then(|u| u.as_bool()))
                                .unwrap_or(false)
                    });
                    if has_user_query {
                        log::info!(
                            "[AGENT] ask_user detected — breaking loop to wait for user input"
                        );
                        break;
                    }

                    round_num += 1;
                    continue;
                }
                Effect::TurnComplete => {}
                _ => {}
            }

            self.flush_meta_and_stats();

            if let Some(ref usage) = last_usage {
                util::record_token_usage(usage, &self.agent.config.model);
            }

            self.emit(Agent2Ui::TurnEnd {
                turn_id: turn_id.clone(),
                stop_reason: None,
                usage: last_usage.clone(),
            });

            break;
        }

        // If turn was suspended for pending approvals, return without Done/TurnEnd.
        if self.saved_turn.is_some() {
            return;
        }

        self.emit(Agent2Ui::Done);
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
        const KEEP_TOKENS: usize = 4_000; // token budget for recent context to keep intact
        let turns_total = self.agent.msg.turn_count();
        log::info!("[AGENT] handle_compact: {} turns", turns_total);

        // Build full message list (excluding system messages) for token-driven split.
        let all = self.agent.msg.build_context_for_gate(&[]);
        let msgs: Vec<&deepx_types::Message> = all.iter().filter(|m| m.role != "system").collect();
        if msgs.is_empty() {
            return;
        }

        // Token-driven split: scan from end, accumulate estimated tokens
        let estimate = |s: &str| -> usize { s.chars().count() / 4 };
        let mut kept_idx = msgs.len();
        let mut kept_tokens = 0usize;
        for (i, m) in msgs.iter().enumerate().rev() {
            let t = estimate(&serde_json::to_string(m).unwrap_or_default());
            if kept_tokens + t > KEEP_TOKENS {
                kept_idx = i + 1;
                break;
            }
            kept_tokens += t;
            kept_idx = i;
        }
        let head_msgs = &msgs[..kept_idx]; // messages to summarize
        if head_msgs.is_empty() {
            self.emit_delta(Agent2Ui::ToolNotice {
                message: "Compact skipped: nothing to compact (all within token budget)".into(),
                level: "info".into(),
            });
            return;
        }

        // Count how many turns are in the head (being compacted)
        let head_user_count = head_msgs.iter().filter(|m| m.role == "user").count();
        // Count kept turns
        let kept_user_count = msgs[kept_idx..].iter().filter(|m| m.role == "user").count();

        self.emit(Agent2Ui::CompactStart {
            turns_total: turns_total as u32,
            turns_keeping: kept_user_count as u32,
        });

        // ── Serialize head messages into a dense text format for the compactor LLM ──
        let mut contexts = Vec::new();
        for m in head_msgs {
            let role = &m.role;
            let serialized: Vec<String> = m
                .content
                .iter()
                .filter_map(|b| match b {
                    deepx_types::ContentBlock::Text { text } => {
                        Some(format!("[{}]: {}", role, text))
                    }
                    deepx_types::ContentBlock::Reasoning { reasoning } => Some(format!(
                        "[{} reasoning]: {}",
                        role,
                        &reasoning[..reasoning.floor_char_boundary(reasoning.len().min(500))]
                    )),
                    deepx_types::ContentBlock::ToolUse { name, input, .. } => {
                        let args = serde_json::to_string(input).unwrap_or_default();
                        Some(format!(
                            "[{} tool call]: {}({})",
                            role,
                            name,
                            &args[..args.floor_char_boundary(args.len().min(120))]
                        ))
                    }
                    deepx_types::ContentBlock::ToolResult { content, .. } => {
                        let compact: String = content
                            .lines()
                            .take(5)
                            .map(|l| l.chars().take(200).collect::<String>())
                            .collect::<Vec<_>>()
                            .join(" | ");
                        Some(format!(
                            "[Tool result]: {}",
                            &compact[..compact.floor_char_boundary(compact.len().min(600))]
                        ))
                    }
                })
                .collect();
            if !serialized.is_empty() {
                contexts.push(serialized.join("\n"));
            }
        }
        // Also serialize the TOOL use/results from kept messages for context about what's happening right now
        for m in &msgs[kept_idx..] {
            let role = &m.role;
            if role == "tool" {
                if let Some(deepx_types::ContentBlock::ToolResult { content, .. }) =
                    m.content.first()
                {
                    let compact: String = content
                        .lines()
                        .take(3)
                        .map(|l| l.chars().take(200).collect::<String>())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    contexts.push(format!(
                        "[Tool result (recent)]: {}",
                        &compact[..compact.floor_char_boundary(compact.len().min(400))]
                    ));
                }
            }
        }

        // ── Timeline: session creation time + duration ──
        let timeline = {
            let created = self.agent.session.created_at;
            let updated = self
                .agent
                .session
                .updated_at
                .max(SessionManager::now_epoch());
            let start_str = epoch_to_date(created);
            let dur = updated.saturating_sub(created);
            let dur_hours = dur / 3600;
            let dur_min = (dur % 3600) / 60;
            format!(
                "- Session started: {} (UTC)\n- Session duration: {}h {}m real-time",
                start_str, dur_hours, dur_min
            )
        };

        // ── Incremental summary: detect previous compact for update mode ──
        let previous_summary = self.agent.msg.previous_compact_summary();

        // ── Build prompt ──
        let prompt = if let Some(ref prev) = previous_summary {
            format!(
                "[COMPACT — UPDATE MODE]\n\n\
                 Update the anchored summary below using the stripped conversation history.\n\
                 Preserve still-true details, remove stale details, merge in new facts.\n\n\
                 <previous-summary>\n{}\n</previous-summary>\n\n\
                 --- HISTORY (newer context to merge) ---\n{}\n--- END HISTORY ---\n\n\
                 {}",
                prev,
                contexts.join("\n\n"),
                COMPACT_TEMPLATE,
            )
        } else {
            format!(
                "[COMPACT]\n\n\
                 Create a new anchored summary from the stripped conversation history.\n\n\
                 --- HISTORY ---\n{}\n--- END HISTORY ---\n\n\
                 {}\n\n\
                 {}",
                contexts.join("\n\n"),
                timeline,
                COMPACT_TEMPLATE,
            )
        };

        let provider = deepx_gate::ProviderConfig::openai(
            &self.agent.config.base_url,
            &self.agent.config.api_key,
            &self.agent.config.model,
            None,
            None,
            Default::default(),
            Default::default(),
            false,
        );
        let msgs_vec = vec![deepx_types::Message::user(&prompt)];
        let summary = match deepx_gate::chat_sync(&provider, msgs_vec, 4096) {
            Ok(s) if !s.trim().is_empty() => s,
            Ok(_) => {
                self.emit(Agent2Ui::Error {
                    message: "Compact failed: model returned empty response. Try again.".into(),
                });
                self.emit(Agent2Ui::CompactEnd {
                    summary_chars: 0,
                    turns_compacted: 0,
                });
                return;
            }
            Err(e) => {
                self.emit(Agent2Ui::Error { message: e });
                self.emit(Agent2Ui::CompactEnd {
                    summary_chars: 0,
                    turns_compacted: 0,
                });
                return;
            }
        };

        let chars = summary.chars().count();
        let keep_turns = kept_user_count;
        self.agent.msg.apply_compact(&summary, keep_turns);
        self.agent.msg.snapshot_full(
            &self.agent.config.model,
            &self.agent.config.reasoning_effort,
        );

        // Write post-compact context stats
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
                "messages": self.agent.msg.turn_count(),
                "chat_text": chat_text,
                "thinking": thinking,
                "tool_calls": tool_calls,
                "tool_results": tool_results,
                "tools_schema": tools_schema,
                "system_prompt": system_prompt,
                "thinking_blocks": thinking_blocks,
                "tool_call_blocks": tool_call_blocks,
            });
            let stats_dir = deepx_types::platform::sessions_dir().join(&self.agent.session.seed);
            let _ = std::fs::create_dir_all(&stats_dir);
            let stats_path = stats_dir.join("context_stats.json");
            let _ = std::fs::write(&stats_path, stats.to_string());
        }

        self.emit(Agent2Ui::CompactEnd {
            summary_chars: chars,
            turns_compacted: head_user_count as u32,
        });
        self.emit_delta(Agent2Ui::ToolNotice {
            message: format!(
                "Compacted {} turns -> {} chars, keeping {} turns",
                head_user_count, chars, keep_turns
            ),
            level: "info".into(),
        });
        self.emit_dashboard();
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
