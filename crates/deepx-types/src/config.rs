use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Config persistence ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentConfig {
    /// Provider ID (e.g. "deepseek", "mimo")
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
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context7_api_key: Option<String>,

    // ── Subagent defaults ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<PersistentSubagentConfig>,

    // ── Compliance / content filter ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_extra_keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_allowlist: Option<Vec<String>>,

    // ── Turso local database mirror ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<PersistentDatabaseConfig>,

    // ── Permission ──
    /// Agent permission level: 1=MaxLockdown, 2=ReadFree, 3=WorkspaceFree, 4=Unrestricted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_level: Option<u8>,
}

/// Persistence-friendly subagent config with all-Option fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentSubagentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_tools: Option<Vec<String>>,
}

/// Persistence-friendly database config mirroring session data to local Turso.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentDatabaseConfig {
    /// Whether the database mirror is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Path to the local Turso database file. If `None`, a default path is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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
        if let Some(parent) = self.path.parent()
            && let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("ConfigStore: create_dir_all({}) failed: {e}", parent.display());
                return false;
            }
        if let Err(e) = std::fs::write(&tmp, &content) {
            eprintln!("ConfigStore: write({}) failed: {e}", tmp.display());
            return false;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            eprintln!("ConfigStore: rename({} -> {}) failed: {e}", tmp.display(), self.path.display());
            return false;
        }
        true
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


