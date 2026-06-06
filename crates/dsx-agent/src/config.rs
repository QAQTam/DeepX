pub use crate::prompt::system_prompt;

use dsx_types::{ConfigStore, PersistentConfig};
use std::collections::HashMap; // still used by profiles

#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub context_limit: u32,
    pub provider_id: String,
    pub protocol: String,
    pub reasoning_effort: String,
    pub profiles: HashMap<String, dsx_types::ProfileConfig>,
    pub active_profile: String,
    pub max_tool_rounds: Option<u32>,
    pub context7_api_key: Option<String>,
    pub lang: Option<String>,
    pub mcp_servers: Vec<dsx_tools::mcp_bridge::McpServerConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert("default".into(), dsx_types::ProfileConfig {
            model: "deepseek-v4-flash".into(), max_tokens: 16384,
            effort: Some("high".into()), context_limit: 1_000_000,
            base_url: "https://api.deepseek.com".into(),
        });
        Self {
            api_key: String::new(), base_url: "https://api.deepseek.com".into(),
            model: "deepseek-v4-flash".into(), max_tokens: 16384, context_limit: 1_000_000,
            provider_id: "deepseek-openai".into(),
            protocol: "openai".into(),
            reasoning_effort: "high".into(),
            profiles, active_profile: "default".into(),
            max_tool_rounds: None,
            context7_api_key: None,
            lang: None,
            mcp_servers: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let store = ConfigStore::default_location();
        let mut cfg = Self::default();

        if let Some(pc) = store.load() {
            if let Some(profiles) = pc.profiles {
                cfg.profiles = profiles;
            }
            if let Some(ref active) = pc.active_profile {
                cfg.active_profile = active.clone();
                if let Some(profile) = cfg.profiles.get(active) {
                    cfg.model = profile.model.clone();
                    cfg.max_tokens = profile.max_tokens;
                    cfg.reasoning_effort = profile.effort.clone().unwrap_or_else(|| "high".into());
                    cfg.context_limit = profile.context_limit;
                    cfg.base_url = profile.base_url.clone();
                }
            }
            if let Some(k) = pc.api_key { if !k.is_empty() { cfg.api_key = k; } }
            if let Some(m) = pc.model { if !m.is_empty() { cfg.model = m; } }
            if let Some(u) = pc.base_url { if !u.is_empty() { cfg.base_url = u; } }
            if let Some(mt) = pc.max_tokens { cfg.max_tokens = mt; }
            if let Some(cl) = pc.context_limit { cfg.context_limit = cl; }
            if let Some(ref pid) = pc.provider_id { if !pid.is_empty() { cfg.provider_id = pid.clone(); } }
            if let Some(ref p) = pc.protocol { if !p.is_empty() { cfg.protocol = p.clone(); } }
            if let Some(ref re) = pc.reasoning_effort { if !re.is_empty() { cfg.reasoning_effort = re.clone(); } }
            if let Some(v) = pc.max_tool_rounds { cfg.max_tool_rounds = Some(v); }
            if let Some(ref k) = pc.context7_api_key { if !k.is_empty() { cfg.context7_api_key = Some(k.clone()); } }
            if let Some(ref l) = pc.lang { if !l.is_empty() { cfg.lang = Some(l.clone()); } }
            if let Some(ref mcp) = pc.mcp_servers {
                if let Ok(servers) = serde_json::from_value::<Vec<dsx_tools::mcp_bridge::McpServerConfig>>(mcp.clone()) {
                    cfg.mcp_servers = servers;
                }
            }
        }

        // Resolve preset: auto-fill protocol/base_url/model from provider_id
        {
            let pid = &cfg.provider_id;
            if let Some(preset) = crate::gate::registry::find_preset(pid) {
                if cfg.protocol.is_empty() || cfg.protocol == "openai" && preset.protocol != "openai" {
                    cfg.protocol = preset.protocol.to_string();
                }
                if cfg.base_url.is_empty() || cfg.base_url == "https://api.deepseek.com" && preset.base_url != "https://api.deepseek.com" {
                    cfg.base_url = preset.base_url.to_string();
                }
                // Only set model from preset if not explicitly set (keep user override)
                if cfg.model.is_empty() || cfg.model == "deepseek-v4-flash" && preset.default_model != "deepseek-v4-flash" {
                    cfg.model = preset.default_model.to_string();
                } else if pid == "custom" && cfg.model.is_empty() {
                    cfg.model = "deepseek-v4-flash".to_string();
                }
            }
        }

        if !cfg.profiles.contains_key("default") {
            cfg.profiles.insert("default".into(), dsx_types::ProfileConfig {
                model: cfg.model.clone(), max_tokens: cfg.max_tokens,
                effort: Some(cfg.reasoning_effort.clone()), context_limit: cfg.context_limit,
                base_url: cfg.base_url.clone(),
            });
        }

        Ok(cfg)
    }

    pub fn save(&self) {
        let store = ConfigStore::default_location();
        let mut profiles = self.profiles.clone();
        profiles.insert(self.active_profile.clone(), dsx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: Some(self.reasoning_effort.clone()), context_limit: self.context_limit,
            base_url: self.base_url.clone(),
        });
        let mcp_val = serde_json::to_value(&self.mcp_servers).ok();
        let pc = PersistentConfig {
            api_key: if self.api_key.is_empty() { None } else { Some(self.api_key.clone()) },
            model: Some(self.model.clone()),
            base_url: Some(self.base_url.clone()),
            max_tokens: Some(self.max_tokens),
            context_limit: Some(self.context_limit),
            provider_id: Some(self.provider_id.clone()),
            protocol: Some(self.protocol.clone()),
            reasoning_effort: Some(self.reasoning_effort.clone()),
            profiles: Some(profiles),
            active_profile: Some(self.active_profile.clone()),
            max_tool_rounds: self.max_tool_rounds,
            lang: self.lang.clone(),
            context7_api_key: self.context7_api_key.clone(),
            mcp_servers: mcp_val,
    };
        if !store.save(&pc) {
            log::error!("Failed to save config");
        }
    }

    pub fn apply_profile(&mut self, name: &str) -> Option<String> {
        let profile = self.profiles.get(name)?.clone();
        self.model = profile.model;
        self.max_tokens = profile.max_tokens;
        self.reasoning_effort = profile.effort.unwrap_or_else(|| "high".into());
        self.context_limit = profile.context_limit;
        self.base_url = profile.base_url;
        self.active_profile = name.to_string();
        self.save();
        Some(name.to_string())
    }

    pub fn save_profile(&mut self, name: &str) {
        self.profiles.insert(name.to_string(), dsx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: Some(self.reasoning_effort.clone()), context_limit: self.context_limit,
            base_url: self.base_url.clone(),
        });
        self.active_profile = name.to_string();
        self.save();
    }

    pub fn delete_profile(&mut self, name: &str) -> bool {
        if name == "default" { return false; }
        if self.profiles.remove(name).is_some() {
            self.save();
            true
        } else { false }
    }

    pub fn is_ready(&self) -> bool { !self.api_key.is_empty() }
}
