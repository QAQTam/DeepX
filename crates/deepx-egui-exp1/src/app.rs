//! Application state: messages, session management, agent event handling.

use crate::agent;
use deepx_proto::{Agent2Ui, RoundDeltaKind, TaskInfo, Ui2Agent};
use std::collections::{HashMap, VecDeque};

// ── View / Navigation ──

#[derive(Clone, PartialEq)]
pub(crate) enum View {
    Chat,
    Settings,
}
impl Default for View {
    fn default() -> Self {
        View::Chat
    }
}

// ── Message types ──

#[derive(Clone)]
pub(crate) struct Message {
    pub role: Role,
    pub text: String,
    /// Tool call ID for linking call ↔ result
    pub tool_id: Option<String>,
    /// Tool result text (filled when result arrives)
    pub tool_result: Option<String>,
    /// Whether tool succeeded
    pub tool_ok: Option<bool>,
    /// Streaming exec output (accumulated from ToolExecDelta)
    pub exec_draft: Option<String>,
    /// false = streaming (show cursor), true = complete
    pub finalized: bool,
}

#[derive(Clone, PartialEq)]
pub(crate) enum Role {
    User,
    Assistant,
    #[allow(dead_code)]
    Thinking,
    ToolCall,
    ToolResult,
    System,
}

#[derive(Clone)]
pub(crate) struct SessionEntry {
    pub seed: String,
    pub summary: String,
}

/// Audit record from agent (tool usage log).
#[derive(Clone)]
pub(crate) struct ActivityEntry {
    pub tool_name: String,
    pub summary: String,
    pub success: bool,
}

/// Balance info from agent.
#[derive(Clone, Default)]
pub(crate) struct BalanceInfo {
    pub is_available: bool,
    pub total_balance: String,
    pub currency: String,
}

// ── AppState ──

#[derive(Clone)]
pub(crate) struct AppState {
    pub view: View,
    pub messages: VecDeque<Message>,
    pub input: String,
    pub connected: bool,
    pub streaming: bool,
    /// Set to true when new events arrive; cleared after repaint.
    pub dirty: bool,
    pub active_seed: Option<String>,
    pub error: Option<String>,

    // Dashboard data
    pub model: String,
    pub context_tokens: u32,
    pub context_limit: u32,
    pub prompt_cache_hit: u32,
    pub prompt_cache_miss: u32,
    pub total_tokens: u32,

    // Tasks from Dashboard
    pub tasks: Vec<TaskInfo>,

    // Recent file edits from Dashboard
    pub recent_edits: Vec<String>,

    // Tool audit log
    pub activity_log: Vec<ActivityEntry>,

    // Accumulated exec output per tool_call_id (from ToolExecDelta)
    exec_buffers: HashMap<String, String>,

    // Pagination
    pub has_more: bool,

    // Compact state
    pub is_compacting: bool,
    pub compact_pct: u32,
    pub compact_result: Option<String>,

    // Balance
    pub balance: Option<BalanceInfo>,

    // Internal cache
    cached_sessions: Vec<SessionEntry>,
    sessions_dirty: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            view: View::default(),
            messages: VecDeque::new(),
            input: String::new(),
            connected: false,
            streaming: false,
            dirty: false,
            active_seed: None,
            error: None,
            model: String::new(),
            context_tokens: 0,
            context_limit: 0,
            prompt_cache_hit: 0,
            prompt_cache_miss: 0,
            total_tokens: 0,
            tasks: Vec::new(),
            recent_edits: Vec::new(),
            activity_log: Vec::new(),
            exec_buffers: HashMap::new(),
            has_more: false,
            is_compacting: false,
            compact_pct: 0,
            compact_result: None,
            balance: None,
            cached_sessions: Vec::new(),
            sessions_dirty: true,
        }
    }
}

impl AppState {
    // ── Session list (cached) ──

    pub(crate) fn session_list(&mut self) -> Vec<SessionEntry> {
        if self.sessions_dirty {
            self.cached_sessions = deepx_session::SessionManager::global()
                .list()
                .into_iter()
                .map(|m| SessionEntry {
                    seed: m.seed.clone(),
                    summary: m.seed.chars().take(12).collect(),
                })
                .collect();
            self.sessions_dirty = false;
        }
        self.cached_sessions.clone()
    }

    // ── Session actions ──

    pub(crate) fn new_session(&mut self) {
        self.clear_session_state();
        deepx_session::SessionManager::global().clear_active();
        self.send_raw(Ui2Agent::NewSession);
    }

    pub(crate) fn resume_session(&mut self, seed: &str) {
        self.clear_session_state();
        deepx_session::SessionManager::global().set_active_seed(seed);
        self.send_raw(Ui2Agent::ResumeSession {
            seed: seed.to_string(),
        });
    }

    pub(crate) fn delete_session(&mut self, seed: &str) {
        let _ = deepx_session::SessionManager::global().delete(seed);
        if self.active_seed.as_deref() == Some(seed) {
            self.active_seed = None;
            self.clear_session_state();
        }
        self.sessions_dirty = true;
    }

    /// Clear all per-session data (messages, tasks, edits, etc.)
    fn clear_session_state(&mut self) {
        self.messages.clear();
        self.exec_buffers.clear();
        self.tasks.clear();
        self.recent_edits.clear();
        self.activity_log.clear();
        self.has_more = false;
        self.is_compacting = false;
        self.compact_result = None;
    }

    // ── IPC helpers ──

    pub(crate) fn send_raw(&self, frame: Ui2Agent) {
        agent::CH.with(|c| {
            if let Some((ref tx, _, _)) = *c.borrow() {
                let _ = tx.send(frame);
            }
        });
    }

    pub(crate) fn send(&self, text: &str) {
        self.send_raw(Ui2Agent::UserInput {
            text: text.to_string(),
        });
    }

    pub(crate) fn send_cancel(&self) {
        self.send_raw(Ui2Agent::Cancel);
    }

    pub(crate) fn send_compact(&self) {
        self.send_raw(Ui2Agent::Compact);
    }

    pub(crate) fn send_load_more(&self, before_turn_id: &str) {
        self.send_raw(Ui2Agent::LoadMoreTurns {
            before_turn_id: before_turn_id.to_string(),
            count: 20,
        });
    }

    pub(crate) fn clear_error(&mut self) {
        self.error = None;
    }

    // ── Connection lifecycle ──

    pub(crate) fn connect(&mut self) {
        match agent::spawn_agent() {
            Ok((tx, rx, child)) => {
                agent::CH.with(|c| *c.borrow_mut() = Some((tx, rx, child)));
                self.connected = true;
                self.messages.push_back(Message::system("已连接"));
            }
            Err(e) => {
                self.messages
                    .push_back(Message::system(&format!("连接失败: {e}")));
            }
        }
    }

    pub(crate) fn disconnect(&mut self) {
        agent::CH.with(|c| {
            if let Some((tx, _, mut child)) = c.borrow_mut().take() {
                let _ = tx.send(Ui2Agent::Shutdown);
                let _ = child.kill();
            }
        });
        self.connected = false;
        self.streaming = false;
    }

    // ── Event loop ──

    /// Drain all pending events from the agent channel.
    pub(crate) fn poll_agent(&mut self) {
        self.dirty = false;
        let events: Vec<Agent2Ui> = agent::CH.with(|c| {
            let g = c.borrow();
            let mut v = Vec::new();
            if let Some((_, ref rx, _)) = *g {
                while let Ok(e) = rx.try_recv() {
                    v.push(e);
                }
            }
            v
        });
        if !events.is_empty() {
            self.dirty = true;
        }
        for e in events {
            self.handle(e);
        }
    }

    /// Route a single Agent2Ui event to state mutation.
    fn handle(&mut self, e: Agent2Ui) {
        match e {
            // ── Turn lifecycle ──
            Agent2Ui::TurnStart { .. } => {
                self.streaming = true;
            }
            Agent2Ui::RoundDelta { kind, delta, .. } => match kind {
                RoundDeltaKind::Answering => {
                    // O(1): draft is always at back, or we push one
                    if !self
                        .messages
                        .back()
                        .map(|m| m.role == Role::Assistant && !m.finalized)
                        .unwrap_or(false)
                    {
                        self.messages.push_back(Message {
                            role: Role::Assistant,
                            text: String::new(),
                            finalized: false,
                            ..Default::default()
                        });
                    }
                    if let Some(msg) = self.messages.back_mut() {
                        if msg.role == Role::Assistant && !msg.finalized {
                            msg.text.push_str(&delta);
                        }
                    }
                }
                RoundDeltaKind::Thinking => {
                    if !self
                        .messages
                        .back()
                        .map(|m| m.role == Role::Thinking && !m.finalized)
                        .unwrap_or(false)
                    {
                        self.messages.push_back(Message {
                            role: Role::Thinking,
                            text: String::new(),
                            finalized: false,
                            ..Default::default()
                        });
                    }
                    if let Some(msg) = self.messages.back_mut() {
                        if msg.role == Role::Thinking && !msg.finalized {
                            msg.text.push_str(&delta);
                        }
                    }
                }
                RoundDeltaKind::ToolCalling => {
                    // Tool calling notice — could show in status bar
                }
            },
            Agent2Ui::RoundComplete {
                answer, blocks, ..
            } => {
                if !blocks.is_empty() {
                    // Remove draft (unfinalized) messages — blocks replace them
                    while self
                        .messages
                        .back()
                        .map(|m| !m.finalized)
                        .unwrap_or(false)
                    {
                        self.messages.pop_back();
                    }
                    for b in blocks {
                        match b {
                            deepx_proto::RoundBlock::Reasoning { content } => {
                                if !content.is_empty() {
                                    self.messages.push_back(Message {
                                        role: Role::Thinking,
                                        text: content,
                                        finalized: true,
                                        ..Default::default()
                                    });
                                }
                            }
                            deepx_proto::RoundBlock::Text { content } => {
                                if !content.is_empty() {
                                    self.messages.push_back(Message {
                                        role: Role::Assistant,
                                        text: content,
                                        finalized: true,
                                        ..Default::default()
                                    });
                                }
                            }
                            deepx_proto::RoundBlock::Tool { card } => {
                                let exec_draft =
                                    self.exec_buffers.remove(&card.id);
                                self.messages.push_back(Message {
                                    tool_id: Some(card.id.clone()),
                                    role: Role::ToolCall,
                                    text: format!(
                                        "{} {}",
                                        card.name, card.args_display
                                    ),
                                    exec_draft,
                                    finalized: true,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                } else {
                    // No blocks — finalize draft messages, use answer if provided
                    let answer_text = answer.clone();
                    for msg in self.messages.iter_mut().rev() {
                        if !msg.finalized {
                            if msg.role == Role::Assistant
                                && answer_text.is_some()
                                && msg.text.is_empty()
                            {
                                msg.text =
                                    answer_text.clone().unwrap_or_default();
                            }
                            msg.finalized = true;
                        }
                    }
                    // If no draft existed but answer provided, push it
                    if let Some(a) = answer.filter(|a| !a.is_empty()) {
                        if !self
                            .messages
                            .iter()
                            .rev()
                            .any(|m| m.role == Role::Assistant && m.text == a)
                        {
                            self.messages.push_back(Message {
                                role: Role::Assistant,
                                text: a,
                                finalized: true,
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            Agent2Ui::TurnEnd { .. } => {
                self.streaming = false;
                // Finalize any remaining draft messages
                for msg in self.messages.iter_mut() {
                    msg.finalized = true;
                }
                self.exec_buffers.clear();
            }

            // ── Tool execution ──
            Agent2Ui::ToolResults { results, .. } => {
                for tr in results {
                    // Consume any remaining exec buffer
                    let exec_output = self.exec_buffers.remove(&tr.tool_call_id);
                    if let Some(msg) = self.messages.iter_mut().rev().find(|m| {
                        m.role == Role::ToolCall
                            && m.tool_id.as_deref() == Some(&tr.tool_call_id)
                    }) {
                        msg.tool_result = Some(tr.output.clone());
                        msg.tool_ok = Some(tr.success);
                        if exec_output.is_some() {
                            msg.exec_draft = exec_output;
                        }
                    } else {
                        self.messages.push_back(Message {
                            tool_id: None,
                            tool_result: None,
                            tool_ok: None,
                            exec_draft: None,
                            finalized: true,
                            role: Role::ToolResult,
                            text: format!(
                                "{} {}",
                                if tr.success { "✓" } else { "✗" },
                                tr.output
                            ),
                        });
                    }
                }
            }
            Agent2Ui::ToolExecDelta {
                tool_call_id, delta, ..
            } => {
                // Accumulate into buffer; also push to matching message's exec_draft
                self.exec_buffers
                    .entry(tool_call_id.clone())
                    .or_default()
                    .push_str(&delta);
                if let Some(msg) = self.messages.iter_mut().rev().find(|m| {
                    m.role == Role::ToolCall
                        && m.tool_id.as_deref() == Some(&tool_call_id)
                }) {
                    msg.exec_draft
                        .get_or_insert_default()
                        .push_str(&delta);
                }
            }

            // ── Session lifecycle ──
            Agent2Ui::SessionCreated { seed } => {
                deepx_session::SessionManager::global().set_active_seed(&seed);
                self.active_seed = Some(seed.clone());
                self.sessions_dirty = true;
                self.messages.push_back(Message::system(&format!(
                    "会话: {}",
                    &seed[..8.min(seed.len())]
                )));
            }
            Agent2Ui::SessionRestored {
                seed,
                turns,
                tokens_used,
                cache_hit_pct,
                has_more,
                ..
            } => {
                self.active_seed = Some(seed);
                self.sessions_dirty = true;
                self.has_more = has_more;
                // Reconstruct messages from restored turns
                self.messages.clear();
                for turn in &turns {
                    self.messages.push_back(Message::user(&turn.user_text));
                    for round in &turn.rounds {
                        if let Some(ref thinking) = round.thinking {
                            if !thinking.is_empty() {
                                self.messages.push_back(Message {
                                    role: Role::Thinking,
                                    text: thinking.clone(),
                                    ..Default::default()
                                });
                            }
                        }
                        if let Some(ref answer) = round.answer {
                            if !answer.is_empty() {
                                self.messages
                                    .push_back(Message::assistant(answer));
                            }
                        }
                        for tc in &round.tool_calls {
                            let result = round
                                .tool_results
                                .iter()
                                .find(|r| r.tool_call_id == tc.id);
                            self.messages.push_back(Message {
                                tool_id: Some(tc.id.clone()),
                                role: Role::ToolCall,
                                text: format!("{} {}", tc.name, tc.args_display),
                                tool_result: result.map(|r| r.output.clone()),
                                tool_ok: result.map(|r| r.success),
                                ..Default::default()
                            });
                        }
                    }
                }
                self.messages.push_back(Message::system(&format!(
                    "已恢复 {} turns · {} tokens · {:.0}% cache hit",
                    turns.len(),
                    tokens_used,
                    cache_hit_pct * 100.0
                )));
            }
            Agent2Ui::MoreTurns { turns, has_more } => {
                self.has_more = has_more;
                // Prepend older turns to the message list
                let mut prepended: VecDeque<Message> = VecDeque::new();
                for turn in &turns {
                    prepended.push_back(Message::user(&turn.user_text));
                    for round in &turn.rounds {
                        if let Some(ref thinking) = round.thinking {
                            if !thinking.is_empty() {
                                prepended.push_back(Message {
                                    role: Role::Thinking,
                                    text: thinking.clone(),
                                    ..Default::default()
                                });
                            }
                        }
                        if let Some(ref answer) = round.answer {
                            if !answer.is_empty() {
                                prepended.push_back(Message::assistant(answer));
                            }
                        }
                        for tc in &round.tool_calls {
                            let result = round
                                .tool_results
                                .iter()
                                .find(|r| r.tool_call_id == tc.id);
                            prepended.push_back(Message {
                                tool_id: Some(tc.id.clone()),
                                role: Role::ToolCall,
                                text: format!("{} {}", tc.name, tc.args_display),
                                tool_result: result.map(|r| r.output.clone()),
                                tool_ok: result.map(|r| r.success),
                                ..Default::default()
                            });
                        }
                    }
                }
                // Prepend to existing messages
                while let Some(msg) = self.messages.pop_front() {
                    prepended.push_back(msg);
                }
                self.messages = prepended;
            }

            // ── Dashboard ──
            Agent2Ui::Dashboard {
                model,
                session_seed,
                usage,
                context_limit,
                tasks,
                recent_edits,
                ..
            } => {
                if let Some(m) = model {
                    self.model = m;
                }
                if !session_seed.is_empty() {
                    self.active_seed = Some(session_seed);
                }
                if let Some(u) = usage {
                    self.context_tokens = u.prompt_tokens;
                    self.total_tokens = u.total_tokens;
                    self.prompt_cache_hit = u.prompt_cache_hit_tokens;
                    self.prompt_cache_miss = u.prompt_cache_miss_tokens;
                }
                if context_limit > 0 {
                    self.context_limit = context_limit;
                }
                if !tasks.is_empty() {
                    self.tasks = tasks;
                }
                if !recent_edits.is_empty() {
                    self.recent_edits = recent_edits;
                }
            }

            // ── System events ──
            Agent2Ui::Ready => {
                // Auto-resume last session or start fresh
                if let Some(seed) =
                    deepx_session::SessionManager::global().active_seed()
                {
                    self.resume_session(&seed);
                } else {
                    self.send_raw(Ui2Agent::NewSession);
                }
            }
            Agent2Ui::Done => {
                // Agent finished processing, input already re-enabled by TurnEnd
            }
            Agent2Ui::Error { message } => {
                self.error = Some(message.clone());
                self.messages
                    .push_back(Message::system(&format!("Error: {message}")));
            }
            Agent2Ui::ToolNotice { message, level } => {
                let prefix = match level.as_str() {
                    "error" => "⚠ ",
                    _ => "ℹ ",
                };
                self.messages
                    .push_back(Message::system(&format!("{prefix}{message}")));
            }
            Agent2Ui::Balance {
                is_available,
                total_balance,
                currency,
            } => {
                self.balance = Some(BalanceInfo {
                    is_available,
                    total_balance,
                    currency,
                });
            }
            Agent2Ui::Cancelled => {
                self.streaming = false;
                self.error = None;
                self.exec_buffers.clear();
                // Remove any draft (unfinalized) messages
                while self
                    .messages
                    .back()
                    .map(|m| !m.finalized)
                    .unwrap_or(false)
                {
                    self.messages.pop_back();
                }
                self.messages.push_back(Message::system("已取消"));
            }
            Agent2Ui::ShutdownAck => {
                self.connected = false;
                self.streaming = false;
            }

            // ── Compact ──
            Agent2Ui::CompactStart { .. } => {
                self.is_compacting = true;
                self.compact_pct = 0;
                self.compact_result = None;
            }
            Agent2Ui::CompactEnd {
                summary_chars,
                turns_compacted,
                ..
            } => {
                self.is_compacting = false;
                if summary_chars > 0 {
                    self.compact_result = Some(format!(
                        "Compacted {} turns → {} chars",
                        turns_compacted, summary_chars
                    ));
                }
            }

            // ── Audit ──
            Agent2Ui::AuditRecord {
                tool_name,
                result_summary,
                success,
            } => {
                self.activity_log.push(ActivityEntry {
                    tool_name,
                    summary: result_summary,
                    success,
                });
                // Keep log bounded
                if self.activity_log.len() > 50 {
                    self.activity_log.remove(0);
                }
            }

            // ── Catch-all for unknown future events ──
            _ => {}
        }
    }
}

// ── Message constructors ──

impl Message {
    pub(crate) fn system(text: &str) -> Self {
        Self {
            role: Role::System,
            text: text.to_string(),
            ..Default::default()
        }
    }

    fn user(text: &str) -> Self {
        Self {
            role: Role::User,
            text: text.to_string(),
            ..Default::default()
        }
    }

    fn assistant(text: &str) -> Self {
        Self {
            role: Role::Assistant,
            text: text.to_string(),
            ..Default::default()
        }
    }
}

impl Default for Message {
    fn default() -> Self {
        Self {
            role: Role::System,
            text: String::new(),
            tool_id: None,
            tool_result: None,
            tool_ok: None,
            exec_draft: None,
            finalized: true,
        }
    }
}
