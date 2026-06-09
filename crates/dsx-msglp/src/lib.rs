//! dsx-msglp: minimal message-loop driver.
//!
//! Responsibilities:
//!   1. Receive Ui2Agent events from the frontend
//!   2. Drive `UserInput` through gate → message → tools
//!   3. Propagate `Cancel` to all modules via `Arc<AtomicBool>`
//!   4. Handle session lifecycle (CreateSession, Shutdown)
//!
//! The loop does NOT emit Agent2Ui events — MessageStore and ToolManager
//! each hold a cloned `Sender<Agent2Ui>` and emit their own data.

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dsx_agent::agent::AgentState;
use dsx_agent::runner::{lifecycle, api_turn, ui_emit, turn};
use dsx_proto::{Agent2Ui, Ui2Agent};

// ═══════════════════════════════════════════════════════
// CancelToken — shared abort flag
// ═══════════════════════════════════════════════════════

/// Lightweight cancel token: one `Arc<AtomicBool>` shared across
/// the loop, gate, ToolManager, and MessageStore.
#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self { inner: Arc::new(AtomicBool::new(false)) }
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
// Loop — the minimal driver
// ═══════════════════════════════════════════════════════

pub struct Loop {
    agent: AgentState,
    ui_rx: mpsc::Receiver<Ui2Agent>,
    ui_tx: mpsc::Sender<Agent2Ui>,
    cancel: CancelToken,
}

impl Loop {
    pub fn new(
        agent: AgentState,
        ui_rx: mpsc::Receiver<Ui2Agent>,
        ui_tx: mpsc::Sender<Agent2Ui>,
    ) -> Self {
        let cancel = CancelToken::new();
        Self { agent, ui_rx, ui_tx, cancel }
    }

    /// Run the loop until shutdown or channel close.
    pub fn run(&mut self) {
        // ── Inject UI sender + cancel into modules ──
        self.agent.msg.set_ui_tx(self.ui_tx.clone());
        self.agent.msg.set_cancel(self.cancel.arc());
        dsx_tools::set_cancel_flag(self.cancel.arc());
        dsx_tools::set_ui_tx(self.ui_tx.clone());

        // ── Inject tool executor into MessageStore ──
        self.agent.msg.set_tool_executor(Box::new(|req| {
            dsx_agent::tools::execute_tool_simple(&req)
        }));

        // ── Emit initial dashboard ──
        self.emit_dashboard();

        // ── Auto-resume from seed ──
        if self.agent.session.seed.is_empty()
            && self.agent.session.resume_seed.is_some()
        {
            let seed = self.agent.session.resume_seed.clone();
            if lifecycle::init_session(&mut self.agent, seed.as_deref()) {
                let _ = self.ui_tx.send(Agent2Ui::SessionRestored {
                    seed: self.agent.session.seed.clone(),
                    turns: turn::build_turns_from_context(&self.agent),
                    tokens_used: 0,
                    cache_hit_pct: 0.0,
                });
            }
        }

        // ── Main event loop ──
        loop {
            let frame = match self.ui_rx.recv() {
                Ok(f) => f,
                Err(_) => break,
            };

            match frame {
                Ui2Agent::UserInput { text } => {
                    self.handle_user_input(&text);
                }

                Ui2Agent::Cancel => {
                    self.cancel.set();
                    dsx_tools::CANCEL.store(true, Ordering::SeqCst);
                    self.agent.turn.stream_cancelled = true;
                    dsx_agent::tools::cancel_current_tool();
                    let _ = self.ui_tx.send(Agent2Ui::Cancelled);
                }

                Ui2Agent::CreateSession => {
                    lifecycle::create_session(&mut self.agent);
                    let _ = self.ui_tx.send(Agent2Ui::SessionCreated {
                        seed: self.agent.session.seed.clone(),
                    });
                }

                Ui2Agent::ReloadConfig => {
                    if let Ok(cfg) = dsx_agent::config::Config::load() {
                        self.agent.config.api_key = cfg.api_key;
                        self.agent.config.model = cfg.model;
                        self.agent.config.base_url = cfg.base_url;
                        self.agent.config.endpoint = cfg.endpoint;
                        self.agent.config.provider_id = cfg.provider_id;
                        self.agent.config.reasoning_effort = cfg.reasoning_effort;
                        self.agent.config.max_tokens = cfg.max_tokens;
                        self.agent.config.context_limit = cfg.context_limit;
                        if let Some(ref key) = cfg.context7_api_key {
                            if !key.is_empty() {
                                dsx_agent::tools::set_context7_key(key);
                            }
                        }
                        dsx_agent::tools::load_workspace(&self.agent.session.seed);
                    }
                }

                Ui2Agent::Shutdown => {
                    self.agent.maybe_save_session();
                    let _ = self.ui_tx.send(Agent2Ui::ShutdownAck);
                    break;
                }

                _ => {}
            }
        }

        dsx_agent::tools::shutdown_tools();
        self.agent.maybe_save_session();
        log::info!(
            "dsx-msglp: shutdown complete (session {}, {} turns, {} tokens)",
            self.agent.session.seed,
            self.agent.msg.turn_count(),
            self.agent.session.tokens
        );
    }

    fn handle_user_input(&mut self, text: &str) {
        if self.agent.session.seed.is_empty() {
            let _ = self.ui_tx.send(Agent2Ui::Error {
                message: "No session — create one first".into(),
            });
            return;
        }

        self.cancel.clear();
        dsx_tools::CANCEL.store(false, Ordering::SeqCst);

        // Delegate to the existing turn handler
        // (turn::handle_user_input still handles the full turn lifecycle)
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            turn::handle_user_input(&mut self.agent, text, &self.ui_tx)
        }))
        .unwrap_or(turn::TurnOutcome { usage: None, tool_calls: 0, tool_failures: 0 });

        // Emit runtime dashboard + done signal
        self.emit_dashboard_with(outcome);
        let _ = self.ui_tx.send(Agent2Ui::Done);
    }

    fn emit_dashboard(&self) {
        let _ = self.ui_tx.send(Agent2Ui::Dashboard {
            hp_connected: true,
            session_seed: self.agent.session.seed.clone(),
            context_limit: self.agent.config.context_limit,
            tool_calls_total: 0,
            tool_failures: 0,
            current_phase: "single".into(),
            streaming: false,
            dsml_compat_count: self.agent.dsml_compat_count,
            documents: dsx_agent::runner::build_documents(&self.agent),
            recent_edits: dsx_agent::runner::build_recent_edits(&self.agent),
            tasks: dsx_agent::runner::build_tasks(&self.agent),
            session_title: self.agent.session.title.clone(),
            usage: None,
        });
    }

    fn emit_dashboard_with(&self, outcome: turn::TurnOutcome) {
        let _ = self.ui_tx.send(Agent2Ui::Dashboard {
            hp_connected: true,
            session_seed: self.agent.session.seed.clone(),
            context_limit: self.agent.config.context_limit,
            tool_calls_total: 0,
            tool_failures: 0,
            current_phase: "single".into(),
            streaming: false,
            dsml_compat_count: self.agent.dsml_compat_count,
            documents: dsx_agent::runner::build_documents(&self.agent),
            recent_edits: dsx_agent::runner::build_recent_edits(&self.agent),
            tasks: dsx_agent::runner::build_tasks(&self.agent),
            session_title: self.agent.session.title.clone(),
            usage: outcome.usage,
        });
    }
}
