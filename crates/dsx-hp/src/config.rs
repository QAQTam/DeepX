//! Minimal connection config for the API proxy (DeepSeek Anthropic endpoint only).

#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
    pub api_key: String,
}
