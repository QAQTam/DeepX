

use deepx_types::{ConfigStore, PersistentConfig, PersistentSubagentConfig};
use std::collections::HashMap; // still used by profiles

/// Subagent default configuration.
/// These are defaults; individual `spawn_subagent` calls can override per-instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubagentConfig {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_subagent_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_subagent_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub default_tools: Vec<String>,
}

fn default_subagent_max_tokens() -> u32 { 4096 }
fn default_subagent_timeout() -> u64 { 120 }

impl Default for SubagentConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            max_tokens: 4096,
            timeout_secs: 120,
            default_tools: vec![
                "file".into(), "exec".into(), "explore".into(),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub context_limit: u32,
    pub provider_id: String,
    pub endpoint: String,
    pub reasoning_effort: String,
    pub profiles: HashMap<String, deepx_types::ProfileConfig>,
    pub active_profile: String,
    pub context7_api_key: Option<String>,
    pub lang: Option<String>,
    pub subagent: SubagentConfig,
}

impl Default for Config {
    fn default() -> Self {
        let (provider_id, endpoint) = crate::registry::first_provider_endpoint();
        let base_url = crate::registry::base_url_for(&provider_id, &endpoint);
        let model = crate::registry::default_model_for(&provider_id, &endpoint);

        let mut profiles = HashMap::new();
        profiles.insert("default".into(), deepx_types::ProfileConfig {
            model: model.clone(), max_tokens: 16384,
            effort: Some("high".into()), context_limit: 1_000_000,
            base_url: base_url.clone(),
            endpoint: None,
        });
        Self {
            api_key: String::new(), base_url,
            model, max_tokens: 16384, context_limit: 1_000_000,
            provider_id,
            endpoint,
            reasoning_effort: "high".into(),
            profiles, active_profile: "default".into(),
            context7_api_key: None,
            lang: None,
            subagent: SubagentConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let store = ConfigStore::default_location();
        let mut cfg = Self::default();

        if let Some(pc) = store.load() {
            // ── Backward compat: migrate old provider_id → new (provider_id, endpoint) ──
            let raw_pid = pc.provider_id.unwrap_or_default();
            let (provider_id, endpoint) = if raw_pid.is_empty() {
                crate::registry::first_provider_endpoint()
            } else {
                crate::registry::migrate_provider_id(&raw_pid)
            };
            cfg.provider_id = provider_id;
            // New endpoint field takes priority over backward-compat migration
            cfg.endpoint = pc.endpoint
                .filter(|e| !e.is_empty())
                .unwrap_or(endpoint);

            // ── Resolve base_url from endpoint (user override takes priority) ──
            let endpoint_base_url = crate::registry::base_url_for(&cfg.provider_id, &cfg.endpoint);
            if !endpoint_base_url.is_empty() {
                cfg.base_url = endpoint_base_url.clone();
            }

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
                    if let Some(ref ep) = profile.endpoint
                        && !ep.is_empty() {
                            cfg.endpoint = ep.clone();
                            let ep_burl = crate::registry::base_url_for(&cfg.provider_id, ep);
                            if !ep_burl.is_empty() && ep_burl != cfg.base_url {
                                cfg.base_url = ep_burl;
                            }
                        }
                }
            }
            if let Some(k) = pc.api_key && !k.is_empty() { cfg.api_key = k; }
            if let Some(m) = pc.model && !m.is_empty() { cfg.model = m; }
            // User base_url override: only apply if differs from all known endpoint defaults
            if let Some(ref u) = pc.base_url
                && !u.is_empty() {
                    let is_ep_default = crate::registry::all_providers().iter()
                        .flat_map(|p| &p.endpoints)
                        .any(|e| e.base_url == *u || e.models_url.as_deref() == Some(u.as_str()));
                    if !is_ep_default {
                        cfg.base_url = u.clone();
                    }
                }
            if let Some(mt) = pc.max_tokens { cfg.max_tokens = mt; }
            if let Some(cl) = pc.context_limit { cfg.context_limit = cl; }
            if let Some(ref re) = pc.reasoning_effort && !re.is_empty() { cfg.reasoning_effort = re.clone(); }
            if let Some(ref k) = pc.context7_api_key && !k.is_empty() { cfg.context7_api_key = Some(k.clone()); }
            if let Some(ref l) = pc.lang && !l.is_empty() { cfg.lang = Some(l.clone()); }
            // ── Subagent defaults ──
            if let Some(ref s) = pc.subagent {
                if let Some(ref m) = s.model && !m.is_empty() { cfg.subagent.model = m.clone(); }
                if let Some(ref u) = s.base_url && !u.is_empty() { cfg.subagent.base_url = u.clone(); }
                if let Some(ref k) = s.api_key && !k.is_empty() { cfg.subagent.api_key = k.clone(); }
                if let Some(mt) = s.max_tokens { cfg.subagent.max_tokens = mt; }
                if let Some(ts) = s.timeout_secs { cfg.subagent.timeout_secs = ts; }
                if let Some(ref tools) = s.default_tools { cfg.subagent.default_tools = tools.clone(); }
            }
        }

        if !cfg.profiles.contains_key("default") {
            cfg.profiles.insert("default".into(), deepx_types::ProfileConfig {
                model: cfg.model.clone(), max_tokens: cfg.max_tokens,
                effort: Some(cfg.reasoning_effort.clone()), context_limit: cfg.context_limit,
                base_url: cfg.base_url.clone(),
                endpoint: Some(cfg.endpoint.clone()),
            });
        }

        Ok(cfg)
    }

    pub fn save(&self) -> Result<(), String> {
        let store = ConfigStore::default_location();
        let mut profiles = self.profiles.clone();
        profiles.insert(self.active_profile.clone(), deepx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: Some(self.reasoning_effort.clone()), context_limit: self.context_limit,
            base_url: self.base_url.clone(),
            endpoint: Some(self.endpoint.clone()),
        });
        let pc = PersistentConfig {
            api_key: if self.api_key.is_empty() { None } else { Some(self.api_key.clone()) },
            model: Some(self.model.clone()),
            base_url: Some(self.base_url.clone()),
            max_tokens: Some(self.max_tokens),
            context_limit: Some(self.context_limit),
            provider_id: Some(self.provider_id.clone()),
            endpoint: Some(self.endpoint.clone()),
            reasoning_effort: Some(self.reasoning_effort.clone()),
            profiles: Some(profiles),
            active_profile: Some(self.active_profile.clone()),
            lang: self.lang.clone(),
            context7_api_key: self.context7_api_key.clone(),
            subagent: Some(PersistentSubagentConfig {
                model: if self.subagent.model.is_empty() { None } else { Some(self.subagent.model.clone()) },
                base_url: if self.subagent.base_url.is_empty() { None } else { Some(self.subagent.base_url.clone()) },
                api_key: if self.subagent.api_key.is_empty() { None } else { Some(self.subagent.api_key.clone()) },
                max_tokens: Some(self.subagent.max_tokens),
                timeout_secs: Some(self.subagent.timeout_secs),
                default_tools: if self.subagent.default_tools.is_empty() { None } else { Some(self.subagent.default_tools.clone()) },
            }),
        };
        if !store.save(&pc) {
            Err(format!("Failed to save config to {}", deepx_types::platform::config_path().display()))
        } else {
            Ok(())
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
            let ep_burl = crate::registry::base_url_for(&self.provider_id, ep);
            if !ep_burl.is_empty() && ep_burl != self.base_url {
                self.base_url = ep_burl;
            }
        }
        self.active_profile = name.to_string();
        let _ = self.save();
        Some(name.to_string())
    }

    pub fn save_profile(&mut self, name: &str) {
        self.profiles.insert(name.to_string(), deepx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: Some(self.reasoning_effort.clone()), context_limit: self.context_limit,
            base_url: self.base_url.clone(),
            endpoint: Some(self.endpoint.clone()),
        });
        self.active_profile = name.to_string();
        let _ = self.save();
    }

    pub fn delete_profile(&mut self, name: &str) -> bool {
        if name == "default" { return false; }
        if self.profiles.remove(name).is_some() {
            let _ = self.save();
            true
        } else { false }
    }

    pub fn is_ready(&self) -> bool { !self.api_key.is_empty() }

    /// Protocol derived from (provider_id, endpoint) in the registry.
    pub fn protocol(&self) -> String {
        crate::registry::protocol_for(&self.provider_id, &self.endpoint)
    }
}
