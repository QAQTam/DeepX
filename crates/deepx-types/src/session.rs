use serde::{Deserialize, Serialize};

/// Session metadata — unified persistence + runtime state.
///
/// Fields marked `#[serde(skip)]` are runtime-only and not persisted to meta.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    // ── Persisted fields ──
    pub seed: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub message_count: usize,
    #[serde(default)]
    pub last_summary: String,
    /// Number of earliest turns compacted (skipped in LLM context).
    #[serde(default)]
    pub compact_skip: usize,

    // ── Runtime fields (not persisted) ──
    /// If set, this seed is passed as a CLI argument to the agent subprocess for auto-restore on startup.
    #[serde(skip)]
    pub resume_seed: Option<String>,
    /// Cumulative tokens consumed across all turns.
    #[serde(skip)]
    pub tokens: u64,
    /// Display title extracted from first user message.
    #[serde(skip)]
    pub title: Option<String>,
    /// True if session was restored from disk — system prompt preserved.
    #[serde(skip)]
    pub from_resume: bool,
}

impl Default for SessionMeta {
    fn default() -> Self {
        Self {
            seed: String::new(),
            created_at: 0,
            updated_at: 0,
            model: String::new(),
            effort: None,
            message_count: 0,
            last_summary: String::new(),
            compact_skip: 0,
            resume_seed: None,
            tokens: 0,
            title: None,
            from_resume: false,
        }
    }
}
