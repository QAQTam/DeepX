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
pub struct EndpointSpec {
    pub id: String,
    pub display: String,
    pub protocol: String,
    pub base_url: String,
    pub default_model: String,
    pub models_url: Option<String>,
    pub user_id_mode: Option<UserSendMode>,
}

#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub id: String,
    pub display: String,
    pub endpoints: Vec<EndpointSpec>,
}
