//! AgentState: the core agent session state, shared between TUI and agent loop.

use crate::config;
use crate::assembly::ContextAssembler;
use crate::health::DsAgentsHealthPlatform;
use crate::session;
use dsx_types::{Message, UsageInfo};

pub mod result;
pub use result::ToolResultAppender;
use result::FileSnapshot;

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
    /// Per-file turn counters since last read/edit. Keyed by file path.
    /// Blocked at >= 7 turns without refreshing.
    pub file_last_read: std::collections::HashMap<String, u32>,

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
    pub session_title: Option<String>,

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

    // ── File hash cache ──
    /// File path → (lines, hash, last_modified). Used to skip re-reads
    /// of unchanged files and serve cached diffs across turns.
    pub file_cache: std::collections::HashMap<String, FileSnapshot>,
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
            file_last_read: std::collections::HashMap::new(),
            re_read_required: None,
            tool_results: Vec::new(),
            session_seed: String::new(),
            session_start: 0,
            resume_seed: None,
            tool_failures: 0,
            tool_calls_this_turn: 0,
            dsml_compat_count: 0,
            session_title: None,
            tool_defs: Vec::new(),
            pending_ask_user: None,
            health: DsAgentsHealthPlatform::new(),
            files_written_this_turn: Vec::new(),
            turn_annotations: Vec::new(),
            stream_content: String::new(),
            stream_reasoning: String::new(),
            max_tool_rounds,
            stream_cancelled: false,
            file_cache: std::collections::HashMap::new(),
        };

        state
    }

    // ── Token helpers ──

    pub fn tokens_used(&self) -> u32 {
        self.api_usage.as_ref().map(|u| u.total_tokens).unwrap_or(self.token_estimate)
    }

    // ── Context helpers ──

    /// Mark a file as just read or edited (resets stale counter).
    pub fn touch_file(&mut self, path: &str) {
        self.file_last_read.insert(path.to_string(), 0);
    }

    /// Check if a file is stale (>= 7 turns since last touch).
    pub fn is_file_stale(&self, path: &str) -> bool {
        self.file_last_read.get(path).copied().unwrap_or(10) >= 7
    }

    /// Increment all file counters at end of turn.
    pub fn age_files(&mut self) {
        for v in self.file_last_read.values_mut() {
            *v = v.saturating_add(1);
        }
    }

    /// Cache file snapshot after read. Returns true if file has changed since last cache.
    pub fn cache_file(&mut self, path: &str) -> bool {
        let new = FileSnapshot {
            lines: 0,
            hash: FileSnapshot::hash_of(path),
            last_read_turn: 0,
        };
        if let Some(old) = self.file_cache.get(path) {
            if old.hash == new.hash { return false; }
        }
        self.file_cache.insert(path.to_string(), new);
        true
    }

    /// Unified system note entry. Stored in turn_annotations for inclusion
    /// in the system prompt tail by build_context().
    pub fn system_note(&mut self, tag: &str, msg: String) {
        self.turn_annotations.push(format!("[{}] {}", tag, msg));
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

    // ── Task progress injection ──

    /// Refresh task progress, injected into turn annotations.
    /// Called each turn before build_context() so the model always sees
    /// current task state without re-reading task files.
    pub fn refresh_progress_context(&mut self) {

        // ── Tasks ──
        if !self.session_seed.is_empty() {
            if let Ok(session_entries) = std::fs::read_dir(dsx_types::platform::sessions_dir()) {
                for entry in session_entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() { continue; }
                    if !path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with(&self.session_seed)).unwrap_or(false) { continue; }
                    let tasks_path = path.join("memory").join("tasks.md");
                    if let Ok(content) = std::fs::read_to_string(&tasks_path) {
                        let mut pending = 0u32;
                        let mut in_progress = 0u32;
                        let mut completed = 0u32;
                        let mut items: Vec<String> = Vec::new();
                        for line in content.lines() {
                            let t = line.trim();
                            if t.starts_with("- [pending]") {
                                pending += 1;
                                items.push(format!("[ ] {}", t.trim_start_matches("- [pending] ").trim()));
                            } else if t.starts_with("- [in_progress]") {
                                in_progress += 1;
                                items.push(format!("[>] {}", t.trim_start_matches("- [in_progress] ").trim()));
                            } else if t.starts_with("- [completed]") {
                                completed += 1;
                                items.push(format!("[✓] {}", t.trim_start_matches("- [completed] ").trim()));
                            } else if t.starts_with("- [cancelled]") {
                                items.push(format!("[x] {}", t.trim_start_matches("- [cancelled] ").trim()));
                            }
                        }
                        if pending + in_progress + completed > 0 {
                            let status_line = format!("pending:{}, progress:{}, done:{}", pending, in_progress, completed);
                            let mut text = format!("{}\n{}", status_line, items.join("\n"));
                            if text.len() > 2000 {
                                text = format!("{}\n{}...", status_line, &items.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
                            }
                            self.turn_annotations.push(format!("[task] {}", text));
                        }
                    }
                    break; // only one session dir matches the seed
                }
            }
        }
    }
}
