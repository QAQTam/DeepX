//! Shared gate types — provider config and unified stream events.

use dsx_types::Message;

// ── Provider ──

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
}

impl ProviderKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "anthropic" => Self::Anthropic,
            _ => Self::OpenAi,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl ProviderConfig {
    pub fn openai(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            kind: ProviderKind::OpenAi,
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    pub fn anthropic(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            kind: ProviderKind::Anthropic,
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

// ── StreamEvent ──

#[derive(Debug, Clone)]
pub enum StreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolCallProgress {
        index: usize,
        id: String,
        name: String,
        args_so_far: String,
    },
    Done {
        raw_message: Message,
        usage: Option<dsx_types::UsageInfo>,
        stop_reason: Option<String>,
    },
    Balance {
        is_available: bool,
        total_balance: String,
        currency: String,
    },
    Error(String),
}
