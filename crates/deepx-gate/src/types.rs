//! Shared gate types — provider config and unified stream events.

use deepx_types::Message;

// ── Provider ──

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    OpenAi,
}

impl ProviderKind {
    pub fn from_str(_s: &str) -> Self {
        Self::OpenAi
    }
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub user_id_mode: Option<deepx_types::UserSendMode>,
}

impl ProviderConfig {
    pub fn openai(base_url: &str, api_key: &str, model: &str, user_id_mode: Option<deepx_types::UserSendMode>) -> Self {
        Self {
            kind: ProviderKind::OpenAi,
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            user_id_mode,
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
        usage: Option<deepx_types::UsageInfo>,
        stop_reason: Option<String>,
    },
    Balance {
        is_available: bool,
        total_balance: String,
        currency: String,
    },
    Error(String),
}
