//! Provider / endpoint model.
//!
//! Provider = the company/service (e.g. DeepSeek).
//! Endpoint = a concrete protocol endpoint for that provider (e.g. OpenAI-compatible, Anthropic-native).
//!
//! The user picks (provider_id, endpoint_id) and the rest auto-fills:
//!   protocol + base_url from EndpointSpec
//!   models from GET /models (with fallback to default_model)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointSpec {
    pub id: String,
    pub display: String,
    pub protocol: String,
    pub base_url: String,
    pub default_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub id: String,
    pub display: String,
    pub endpoints: Vec<EndpointSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub owned_by: String,
}
