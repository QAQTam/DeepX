//! AgentState: the core agent session state, shared between TUI and agent loop.

use crate::config;
use crate::assembly::ContextAssembler;

use dsx_types::Message;

pub mod result;
pub mod file_tracker;
pub mod turn_state;
pub mod session_meta;
pub use result::ToolResultAppender;
pub use file_tracker::FileTracker;
pub use turn_state::TurnState;
pub use session_meta::SessionMeta;

// ── AgentState ──

pub struct AgentState {
    /// Canonical conversation context with strict alternation guarantees.
    pub ctx: ContextAssembler,

    // ── Configuration ──
    pub config: crate::config::Config,

    // ── Explore-before-read state machine ──
    pub files: FileTracker,

    // ── Tool results ──
    pub tool_results: Vec<(String, String)>,

    // ── Session identity ──
    pub session: SessionMeta,

    // ── Turn-scoped state ──
    pub turn: TurnState,

    /// Cumulative count of tool calls successfully parsed via DSML/XML (DeepSeek compat).
    pub dsml_compat_count: u32,

    // ── Registered tool definitions (from dsx-tools) ──
    pub tool_defs: Vec<dsx_types::ToolDef>,

    // ── Interrupt / resume support ──
    
    /// Monotonic turn counter (incremented after each completed turn).
    pub turn_count: u32,
}

impl AgentState {
    pub fn new(config: crate::config::Config) -> Self {
        let prompt = config::system_prompt();
        let mut ctx = ContextAssembler::new();
        ctx.push_system(Message::system(&prompt));

        let state = Self {
            ctx,
            config,
            files: FileTracker::new(),
            tool_results: Vec::new(),
            session: SessionMeta::new(),
            turn: TurnState::new(),
            dsml_compat_count: 0,
            tool_defs: Vec::new(),
                        turn_count: 0,
        };

        state
    }

    /// Full initialization: load config, init tools, context7, tool defs.
    /// `caller` is a label for the tools subsystem ("tui", "tauri", "pipe").
    pub fn init(caller: &str) -> Self {
        let config = crate::config::Config::load().unwrap_or_default();
        let mcp_configs = config.mcp_servers.clone();
        crate::tools::init_tools(caller, &mcp_configs);
        if let Some(ref key) = config.context7_api_key {
            if !key.is_empty() {
                crate::tools::set_context7_key(key);
            }
        }
        let mut agent = Self::new(config);
        agent.tool_defs = crate::tools::all_tools();
        agent
    }

    // ── Convenience delegators for FileTracker ──

    /// Mark a file as just read (delegates to FileTracker).
    pub fn touch_file(&mut self, path: &str) {
        self.files.touch_file(path);
    }

    /// Mark a file as just written (delegates to FileTracker).
    pub fn mark_file_written(&mut self, path: &str) {
        self.files.mark_file_written(path);
    }

    /// Check if a file is stale (delegates to FileTracker).
    pub fn is_file_stale(&self, path: &str) -> bool {
        self.files.is_file_stale(path)
    }

    /// Cache file snapshot after read (delegates to FileTracker).
    pub fn cache_file(&mut self, path: &str) -> bool {
        self.files.cache_file(path)
    }

    /// Unified system note entry. Delegates to TurnState.
    pub fn system_note(&mut self, tag: &str, msg: String) {
        self.turn.note(tag, msg);
    }

    // ── Context building ──

    /// Build context for the next API request.
    ///
    /// Layer 1: System prompt (static — KV cached).
    /// Layer 2: Conversation history.
    /// Turn annotations appended to the last user message.
    pub fn build_context(&mut self) -> Vec<Message> {
        let mut messages: Vec<Message>;

        if self.session.from_resume {
            messages = self.ctx.system_messages().to_vec();
        } else {
            let mut sys = String::new();

            if self.config.reasoning_effort == "max" {
                sys.push_str(crate::prompt::THINK_MAX);
                sys.push('\n');
            }

            sys.push_str(&crate::config::system_prompt());
            sys.push_str("\n\n");

            if self.config.provider_id == "deepseek" {
                sys.push_str(crate::prompt::DSML_SCHEMA);
            }

            messages = vec![Message::system(&sys)];
        }

        let mut conv = self.ctx.build();

        let mut dyn_suffix = String::new();
        if !self.turn.annotations.is_empty() {
            let ann = self.turn.annotations.join("\n");
            dyn_suffix.push_str("\n\n## Notes\n");
            dyn_suffix.push_str(&ann);
        }

        if !dyn_suffix.is_empty() {
            if let Some(last_user) = conv.iter_mut().rev().find(|m| m.role == "user") {
                let existing = last_user.content.iter_mut().find_map(|b| {
                    if let dsx_types::ContentBlock::Text { ref mut text } = b {
                        Some(text)
                    } else {
                        None
                    }
                });
                if let Some(text) = existing {
                    text.push_str(&dyn_suffix);
                } else {
                    last_user.content.push(dsx_types::ContentBlock::text(&dyn_suffix));
                }
            }
        }

        messages.extend(conv);
        self.turn.annotations.clear();
        messages
    }


    // ── Persist ──

    /// Save session to disk if seeded and non-empty, AND no pending tool calls.
    pub fn maybe_save_session(&mut self) {
        if self.ctx.has_pending_tools() { return; }
        let msgs = self.ctx.to_vec();
        if msgs.len() > 1 && !self.session.seed.is_empty() {
            dsx_session::SessionManager::global().save(
                &self.session.seed,
                &msgs,
                &self.config.model,
                Some(&self.config.reasoning_effort),
            );
        }
    }

    // ── Task progress injection ──

    /// Refresh task progress, injected into turn annotations.
    /// Called each turn before build_context() so the model always sees
    /// current task state without re-reading task files.
    pub fn refresh_progress_context(&mut self) {

        // ── Tasks ──
        if !self.session.seed.is_empty() {
            if let Ok(session_entries) = std::fs::read_dir(dsx_types::platform::sessions_dir()) {
                for entry in session_entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() { continue; }
                    if !path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with(&self.session.seed)).unwrap_or(false) { continue; }
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
                            self.turn.annotations.push(format!("[task] {}", text));
                        }
                    }
                    break; // only one session dir matches the seed
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_agent() -> AgentState {
        AgentState::new(Config::default())
    }

    #[test]
    fn stale_only_when_written_after_read() {
        let mut a = test_agent();

        // Simulate: read foo.rs at turn 0
        a.touch_file("foo.rs");
        // Simulate: read bar.rs at turn 0
        a.touch_file("bar.rs");

        a.files.staleness_epoch = 5;
        // Write bar.rs — bar.rs is now stale, foo.rs is NOT
        a.mark_file_written("bar.rs");

        assert!(!a.is_file_stale("foo.rs"), "foo.rs was never written → not stale");
        assert!(a.is_file_stale("bar.rs"), "bar.rs was written after read → stale");
    }

    #[test]
    fn no_writes_nothing_stale() {
        let mut a = test_agent();
        a.touch_file("foo.rs");
        a.touch_file("bar.rs");
        a.touch_file("baz.rs");

        // 5 turns pass, but no writes → not stale (within 10-turn window)
        a.files.staleness_epoch = 5;

        assert!(!a.is_file_stale("foo.rs"));
        assert!(!a.is_file_stale("bar.rs"));
        assert!(!a.is_file_stale("baz.rs"));
    }

    #[test]
    fn stale_after_many_turns_without_reread() {
        let mut a = test_agent();
        a.touch_file("old.rs"); // read at turn 0
        a.files.staleness_epoch = 15; // 15 turns later, never written
        assert!(a.is_file_stale("old.rs"), "15 turns without re-read → stale by time decay");
    }

    #[test]
    fn write_then_read_makes_not_stale() {
        let mut a = test_agent();
        a.files.staleness_epoch = 1;
        a.mark_file_written("foo.rs"); // written at turn 1
        a.files.staleness_epoch = 2;
        a.touch_file("foo.rs"); // re-read at turn 2
        assert!(!a.is_file_stale("foo.rs"), "re-read after write → not stale");
    }

    #[test]
    fn untracked_file_not_stale() {
        let a = test_agent();
        assert!(!a.is_file_stale("never_seen.rs"));
    }

    #[test]
    fn delete_file_removes_tracking() {
        let mut a = test_agent();
        a.files.staleness_epoch = 1;
        a.touch_file("foo.rs");
        a.files.staleness_epoch = 2;
        a.mark_file_written("foo.rs");
        assert!(a.is_file_stale("foo.rs"));

        a.files.file_read_at.remove("foo.rs");
        a.files.file_written_at.remove("foo.rs");
        assert!(!a.is_file_stale("foo.rs"));
    }
}
