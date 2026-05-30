use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Phase-specific performance config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhasePerfConfig {
    pub model: String,
    pub context_limit: u32,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl Default for PhasePerfConfig {
    fn default() -> Self {
        Self { model: "deepseek-v4-flash".into(), context_limit: 1_000_000, max_tokens: 8192, effort: Some("high".into()) }
    }
}

pub fn default_phase_configs() -> HashMap<String, PhasePerfConfig> {
    let mut m = HashMap::new();
    m.insert("plan".into(), PhasePerfConfig { model: "deepseek-v4-pro".into(), context_limit: 1_000_000, max_tokens: 4096, effort: Some("max".into()) });
    m.insert("coding".into(), PhasePerfConfig { model: "deepseek-v4-flash".into(), context_limit: 1_000_000, max_tokens: 16384, effort: Some("high".into()) });
    m.insert("debug".into(), PhasePerfConfig { model: "deepseek-v4-pro".into(), context_limit: 1_000_000, max_tokens: 8192, effort: Some("high".into()) });
    m
}

// ── Config persistence ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentConfig {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<HashMap<String, ProfileConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_configs: Option<HashMap<String, PhasePerfConfig>>,
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
}

fn default_base_url() -> String { "https://api.deepseek.com/anthropic".into() }

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
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self, config: &PersistentConfig) -> bool {
        let content = serde_json::to_string_pretty(config).unwrap_or_default();
        let tmp = self.path.with_extension("json.tmp");
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&tmp, &content).is_ok()
            && std::fs::rename(&tmp, &self.path).is_ok()
    }

    pub fn load_api_key(&self) -> Option<String> {
        let data = std::fs::read_to_string(&self.path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&data).ok()?;
        v.get("api_key").and_then(|k| k.as_str()).map(String::from)
    }

    pub fn load_value(&self) -> Option<serde_json::Value> {
        let data = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&data).ok()
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


