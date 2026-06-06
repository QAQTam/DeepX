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
    pub endpoint: String,
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
            endpoint: None,
        });
        Self {
            api_key: String::new(), base_url: "https://api.deepseek.com".into(),
            model: "deepseek-v4-flash".into(), max_tokens: 16384, context_limit: 1_000_000,
            provider_id: "deepseek".into(),
            endpoint: "openai".into(),
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
            // ── Backward compat: migrate old provider_id → new (provider_id, endpoint) ──
            let raw_pid = pc.provider_id.unwrap_or_default();
            let (provider_id, endpoint) = if raw_pid.is_empty() {
                ("deepseek".to_string(), "openai".to_string())
            } else {
                crate::gate::registry::migrate_provider_id(&raw_pid)
            };
            cfg.provider_id = provider_id;
            // New endpoint field takes priority over backward-compat migration
            cfg.endpoint = pc.endpoint
                .filter(|e| !e.is_empty())
                .unwrap_or(endpoint);

            // ── Resolve protocol + base_url from endpoint ──
            cfg.protocol = crate::gate::registry::protocol_for(&cfg.provider_id, &cfg.endpoint);
            let endpoint_base_url = crate::gate::registry::base_url_for(&cfg.provider_id, &cfg.endpoint);

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
            // base_url: user override takes priority, otherwise use endpoint default
            if let Some(u) = pc.base_url {
                if !u.is_empty() && u != endpoint_base_url {
                    cfg.base_url = u;
                } else if u.is_empty() && !endpoint_base_url.is_empty() {
                    cfg.base_url = endpoint_base_url;
                } else {
                    cfg.base_url = u;
                }
            } else if !endpoint_base_url.is_empty() {
                cfg.base_url = endpoint_base_url.clone();
            }
            if let Some(mt) = pc.max_tokens { cfg.max_tokens = mt; }
            if let Some(cl) = pc.context_limit { cfg.context_limit = cl; }
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

        if !cfg.profiles.contains_key("default") {
            cfg.profiles.insert("default".into(), dsx_types::ProfileConfig {
                model: cfg.model.clone(), max_tokens: cfg.max_tokens,
                effort: Some(cfg.reasoning_effort.clone()), context_limit: cfg.context_limit,
                base_url: cfg.base_url.clone(),
                endpoint: Some(cfg.endpoint.clone()),
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
            endpoint: Some(self.endpoint.clone()),
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
            endpoint: Some(self.endpoint.clone()),
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
        if let Some(ref ep) = profile.endpoint {
            self.endpoint = ep.clone();
            self.protocol = crate::gate::registry::protocol_for(&self.provider_id, ep);
        }
        self.active_profile = name.to_string();
        self.save();
        Some(name.to_string())
    }

    pub fn save_profile(&mut self, name: &str) {
        self.profiles.insert(name.to_string(), dsx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: Some(self.reasoning_effort.clone()), context_limit: self.context_limit,
            base_url: self.base_url.clone(),
            endpoint: Some(self.endpoint.clone()),
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
