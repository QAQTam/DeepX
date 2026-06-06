//! SessionMeta: session identity and persistence state.
//!
//! Owned by AgentState. Never reassigned after init_session().

#[derive(Debug, Clone)]
pub struct SessionMeta {
    /// Unique session identifier (also sent as user_id to API for KV cache affinity).
    pub seed: String,

    /// UNIX epoch when session was created.
    pub start: u64,

    /// If set, restore this session on first user message.
    pub resume_seed: Option<String>,

    /// Cumulative tokens consumed across all turns.
    pub tokens: u64,

    /// Display title extracted from first user message.
    pub title: Option<String>,

    /// True if session was restored from disk — system prompt preserved.
    pub from_resume: bool,
}

impl SessionMeta {
    pub fn new() -> Self {
        Self {
            seed: String::new(),
            start: 0,
            resume_seed: None,
            tokens: 0,
            title: None,
            from_resume: false,
        }
    }
}

impl Default for SessionMeta {
    fn default() -> Self {
        Self::new()
    }
}
