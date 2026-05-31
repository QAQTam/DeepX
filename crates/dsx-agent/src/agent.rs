//! AgentState: the core agent session state, shared between TUI and agent loop.

use crate::config;
use crate::assembly::ContextAssembler;
use crate::health::DsAgentsHealthPlatform;
use crate::session;
use dsx_types::{Message, UsageInfo};

// ── AgentState ──

pub struct AgentState {
    /// Canonical conversation context with strict alternation guarantees.
    pub ctx: ContextAssembler,

    // ── Configuration ──
    pub config: crate::config::Config,

    // ── Token tracking ──
    pub token_estimate: u32,
    pub api_usage: Option<UsageInfo>,
    pub session_tokens: u64,

    // ── Explore-before-read state machine ──
    pub has_explored: bool,
    pub turns_since_last_read: u32,

    /// After a file write/edit, forces a re-read before other tools.
    pub re_read_required: Option<String>,

    // ── Tool results ──
    pub tool_results: Vec<(String, String)>,

    // ── Session persistence ──
    pub session_seed: String,
    pub session_start: u64,
    pub resume_seed: Option<String>,

    // ── Tool chain safety ──
    pub tool_failures: u32,
    pub tool_calls_this_turn: u32,
    /// Cumulative count of tool calls successfully parsed via DSML/XML (DeepSeek compat).
    pub dsml_compat_count: u32,
    pub auto_verify: Vec<String>,

    // ── Registered tool definitions (from dsx-tools) ──
    pub tool_defs: Vec<dsx_types::ToolDef>,

    // ── ask_user flow ──
    pub pending_ask_user: Option<String>,

    // ── Mode flags ──

    // ── Health / monitoring ──
    pub health: DsAgentsHealthPlatform,
    pub files_written_this_turn: Vec<String>,

    /// Per-turn annotations collected during tool execution,
    /// rendered into the system prompt tail, then cleared after build_context().
    pub turn_annotations: Vec<String>,

    // ── Tool round limits ──
    pub max_tool_rounds: u32,

    // ── Streaming state ──
    pub stream_content: String,
    pub stream_reasoning: String,
    pub stream_cancelled: bool,

    // ── Document context cache ──
    /// Named context messages injected before conversation history in
    /// `build_context()`. Key = label (used as `name` field), Value = content.
    /// Insertion order is preserved for deterministic KV cache prefix.
    pub context_messages: Vec<(String, String)>,
}

impl AgentState {
    pub fn new(config: crate::config::Config) -> Self {
        let prompt = config::system_prompt();
        let mut ctx = ContextAssembler::new();
        ctx.push_system(Message::system(&prompt));

        let max_tool_rounds = config.max_tool_rounds.unwrap_or(10);

        let state = Self {
            ctx,
            config,
            token_estimate: 0,
            api_usage: None,
            session_tokens: 0,
            has_explored: false,
            turns_since_last_read: 0,
            re_read_required: None,
            tool_results: Vec::new(),
            session_seed: String::new(),
            session_start: 0,
            resume_seed: None,
            tool_failures: 0,
            tool_calls_this_turn: 0,
            dsml_compat_count: 0,
            auto_verify: Vec::new(),
            tool_defs: Vec::new(),
            pending_ask_user: None,
            health: DsAgentsHealthPlatform::new(),
            files_written_this_turn: Vec::new(),
            turn_annotations: Vec::new(),
            stream_content: String::new(),
            stream_reasoning: String::new(),
            max_tool_rounds,
            stream_cancelled: false,
            context_messages: Vec::new(),
        };

        state
    }

    // ── Token helpers ──

    pub fn tokens_used(&self) -> u32 {
        self.api_usage.as_ref().map(|u| u.total_tokens).unwrap_or(self.token_estimate)
    }

    // ── Context helpers ──

    /// Unified system note entry. Stored in turn_annotations for inclusion
    /// in the system prompt tail by build_context().
    pub fn system_note(&mut self, tag: &str, msg: String) {
        self.turn_annotations.push(format!("[{}] {}", tag, msg));
    }

    // ── Document context cache ──

    /// Insert or update a named context message. These are injected before
    /// the conversation history in `build_context()` as `role: "user"` messages
    /// with the `name` field set. Same label replaces previous content.
    /// The content should be stable across turns for KV cache reuse.
    pub fn push_context(&mut self, label: &str, content: &str) {
        if let Some(entry) = self.context_messages.iter_mut().find(|(k, _)| k == label) {
            entry.1 = content.to_string();
        } else {
            self.context_messages.push((label.to_string(), content.to_string()));
        }
    }

    /// Remove a named context message by label.
    pub fn remove_context(&mut self, label: &str) {
        self.context_messages.retain(|(k, _)| k != label);
    }

    // ── Persist ──

    /// Save session to disk if seeded and non-empty, AND no pending tool calls.
    pub fn maybe_save_session(&mut self) {
        if self.ctx.has_pending_tools() { return; }
        let msgs = self.ctx.to_vec();
        if msgs.len() > 1 && !self.session_seed.is_empty() {
            session::finalize_session(
                &self.session_seed,
                &msgs,
                &self.config.model,
                self.config.effort.as_deref(),
            );
        }
    }
}

// ── ToolResultAppender ──
//
// Unified entry point for writing tool results to the context.

use crate::orchestrator::tracker;
use crate::tools::wrap_tool_result;

pub struct ToolResultAppender<'a> {
    pub state: &'a mut AgentState,
}

impl<'a> ToolResultAppender<'a> {
    pub fn new(state: &'a mut AgentState) -> Self {
        Self { state }
    }

    /// Append a tool result to the context and record all side effects.
    pub fn append(&mut self, tool_name: &str, tc_id: &str, args: &str, raw: &str) -> bool {
        // Global size gate: any tool result > 50K chars gets truncated
        // to prevent LLM context bloat regardless of per-tool limits.
        const MAX_TOOL_RESULT_CHARS: usize = 50_000;
        let truncated = if raw.len() > MAX_TOOL_RESULT_CHARS {
            let mut t = raw[..MAX_TOOL_RESULT_CHARS].to_string();
            t.push_str(&format!("\n...[TRUNCATED: {} total chars, showing first {MAX_TOOL_RESULT_CHARS}]", raw.len()));
            t
        } else {
            raw.to_string()
        };

        let failed = raw.starts_with("[ERROR]") || raw.starts_with("[FAIL]");
        let result = wrap_tool_result(tool_name, &truncated);

        if let Err(e) = self.state.ctx.push_tool_result(tc_id, &result) {
            log::warn!("ToolResultAppender: push_tool_result failed for {}: {:?}", tc_id, e);
            let _ = self.state.ctx.push_tool_result_for(tc_id, &result);
        }

        self.state.tool_results.push((tool_name.to_string(), result.clone()));

        if !failed && (tool_name == "write_file" || tool_name == "edit_file") {
            tracker::track_file_written(self.state, args);
            if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                self.state.re_read_required = Some(path);
            }
        }

        if raw.starts_with("[OK]") && (tool_name == "write_file" || tool_name == "edit_file") {
            if !self.state.auto_verify.contains(&"cargo check".to_string()) {
                if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                    if path.ends_with(".rs") {
                        self.state.auto_verify.push("cargo check".to_string());
                    }
                }
            }
        }

        !failed
    }
}
