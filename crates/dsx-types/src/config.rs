use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Config persistence ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentConfig {
    /// Provider preset: "deepseek"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u32>,
    /// Protocol: "openai" or "anthropic" (deprecated — use endpoint)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Endpoint within the provider: "openai" | "anthropic" | ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Reasoning effort: "high" or "max". Thinking is always enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<HashMap<String, ProfileConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_rounds: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context7_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<serde_json::Value>,
}

// ── Profile / Preferences ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub context_limit: u32,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

fn default_base_url() -> String { "https://api.deepseek.com".into() }

// ── ConfigStore: unified config I/O with atomic writes ──

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_location() -> Self {
        Self::new(crate::platform::config_path())
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn load(&self) -> Option<PersistentConfig> {
        let data = std::fs::read_to_string(&self.path).ok()?;
        toml::from_str(&data).ok()
    }

    pub fn save(&self, config: &PersistentConfig) -> bool {
        let content = match toml::to_string_pretty(config) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ConfigStore: serialization failed: {e}");
                return false;
            }
        };
        let tmp = self.path.with_extension("toml.tmp");
        if let Some(parent) = self.path.parent() {
            drop(std::fs::create_dir_all(parent));
        }
        std::fs::write(&tmp, &content).is_ok()
            && std::fs::rename(&tmp, &self.path).is_ok()
    }

    pub fn load_api_key(&self) -> Option<String> {
        let data = std::fs::read_to_string(&self.path).ok()?;
        let v: toml::Value = toml::from_str(&data).ok()?;
        v.get("api_key").and_then(|k| k.as_str()).map(String::from)
    }

    pub fn load_value(&self) -> Option<serde_json::Value> {
        let data = std::fs::read_to_string(&self.path).ok()?;
        let tv: toml::Value = toml::from_str(&data).ok()?;
        // Convert toml::Value → serde_json::Value for backward compat
        serde_json::to_value(&tv).ok()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BalanceInfo {
    pub is_available: bool,
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}


