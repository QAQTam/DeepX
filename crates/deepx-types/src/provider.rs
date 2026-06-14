//! Provider / endpoint model.
//!
//! Provider = the company/service (e.g. DeepSeek).
//! Endpoint = a concrete protocol endpoint for that provider (e.g. OpenAI-compatible, Anthropic-native).
//!
//! The user picks (provider_id, endpoint_id) and the rest auto-fills:
//!   protocol + base_url from EndpointSpec
//!   models from GET /models (with fallback to default_model)

#[derive(Debug, Clone)]
pub enum UserSendMode {
    Body,
}

#[derive(Debug, Clone)]
pub enum ThinkingParamMode {
    /// thinking: {type: "enabled"} at top-level body. DeepSeek, GLM, Kimi, MiMo, Doubao, OpenAI.
    OpenAi,
    /// enable_thinking: true at top-level body. Qwen.
    QwenEnableThinking,
    /// thinking: {type: "adaptive"} + reasoning_split: true at top-level. MiniMax.
    MiniMaxAdaptive,
}

#[derive(Debug, Clone)]
pub enum CacheTokenField {
    /// usage.prompt_cache_hit_tokens + usage.prompt_cache_miss_tokens. DeepSeek.
    PromptCacheHitTokens,
    /// usage.prompt_tokens_details.cached_tokens (nested). Qwen, GLM.
    PromptDetailsCached,
    /// usage.cached_tokens (top-level, single value). Kimi.
    UsageCachedTokens,
    /// No cache info returned. MiMo, MiniMax.
    None,
}

impl Default for ThinkingParamMode {
    fn default() -> Self { Self::OpenAi }
}

impl Default for CacheTokenField {
    fn default() -> Self { Self::PromptCacheHitTokens }
}

#[derive(Debug, Clone)]
pub struct EndpointSpec {
    pub id: String,
    pub display: String,
    pub protocol: String,
    pub base_url: String,
    pub default_model: String,
    pub models: Vec<String>,
    pub models_url: Option<String>,
    pub user_id_mode: Option<UserSendMode>,

    // ── Multi-provider adaptation fields ──
    /// Chat completions path override (without base_url). None → "/chat/completions".
    pub chat_path: Option<String>,
    /// Balance query path override. None → "/user/balance".
    pub balance_path: Option<String>,
    /// Thinking parameter format. Default: OpenAi.
    pub thinking_mode: ThinkingParamMode,
    /// Cache token field location in usage response. Default: PromptCacheHitTokens.
    pub cache_field: CacheTokenField,
    /// Whether balance endpoint exists. Default: true.
    pub has_balance: bool,
    /// Whether thinking parameter is supported. Default: true.
    pub supports_thinking: bool,
}

impl Default for EndpointSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            display: String::new(),
            protocol: "openai".into(),
            base_url: String::new(),
            default_model: String::new(),
            models: Vec::new(),
            models_url: None,
            user_id_mode: None,
            chat_path: None,
            balance_path: None,
            thinking_mode: ThinkingParamMode::default(),
            cache_field: CacheTokenField::default(),
            has_balance: true,
            supports_thinking: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub id: String,
    pub display: String,
    pub endpoints: Vec<EndpointSpec>,
}
