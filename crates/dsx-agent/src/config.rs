pub use crate::prompt::system_prompt;

use dsx_types::PersistentConfig;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub context_limit: u32,
    pub effort: Option<String>,
    pub prompt_lang: String,
    pub profiles: HashMap<String, dsx_types::ProfileConfig>,
    pub active_profile: String,
    pub auto_mode: bool,
    pub max_tool_rounds: Option<u32>,
    pub phase_configs: HashMap<String, dsx_types::PhasePerfConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert("default".into(), dsx_types::ProfileConfig {
            model: "deepseek-v4-flash".into(), max_tokens: 16000,
            effort: Some("high".into()), context_limit: 1_000_000,
            base_url: "https://api.deepseek.com/anthropic".into(), prompt_lang: "en".into(),
        });
        Self {
            api_key: String::new(), base_url: "https://api.deepseek.com/anthropic".into(),
            model: "deepseek-v4-flash".into(), max_tokens: 16000, context_limit: 1_000_000,
            effort: None, prompt_lang: "en".into(),
            profiles, active_profile: "default".into(),
            auto_mode: true,
            max_tool_rounds: None,
            phase_configs: dsx_types::default_phase_configs(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut cfg = Self::default();

        let path = config_path();
        if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(pc) = serde_json::from_str::<PersistentConfig>(&data) {
                // 1. Load profiles
                if let Some(profiles) = pc.profiles {
                    cfg.profiles = profiles;
                }
                // 2. Apply active profile
                if let Some(ref active) = pc.active_profile {
                    cfg.active_profile = active.clone();
                    if let Some(profile) = cfg.profiles.get(active) {
                        cfg.model = profile.model.clone();
                        cfg.max_tokens = profile.max_tokens;
                        cfg.effort = profile.effort.clone();
                        cfg.context_limit = profile.context_limit;
                        cfg.base_url = profile.base_url.clone();
                        cfg.prompt_lang = profile.prompt_lang.clone();
                    }
                }
                // 3. Overlay flat fields (backward compat — take precedence)
                if let Some(k) = pc.api_key { if !k.is_empty() { cfg.api_key = k; } }
                if let Some(m) = pc.model { if !m.is_empty() { cfg.model = m; } }
                if let Some(u) = pc.base_url { if !u.is_empty() { cfg.base_url = u; } }
                if let Some(mt) = pc.max_tokens { cfg.max_tokens = mt; }
                if let Some(cl) = pc.context_limit { cfg.context_limit = cl; }
                if let Some(ref e) = pc.effort { if !e.is_empty() { cfg.effort = Some(e.clone()); } }
                if let Some(pl) = pc.prompt_lang { if !pl.is_empty() { cfg.prompt_lang = pl; } }
                if let Some(am) = pc.auto_mode { cfg.auto_mode = am; }
                if let Some(pc2) = pc.phase_configs { cfg.phase_configs = pc2; }
            }
        }
    }

        // 4. Env vars (highest priority)
        if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") { let k = k.trim().to_string(); if !k.is_empty() { cfg.api_key = k; } }
        if let Ok(m) = std::env::var("DEEPSEEK_MODEL") { cfg.model = m; }
        if let Ok(u) = std::env::var("DEEPSEEK_BASE_URL") { cfg.base_url = u; }
        if let Ok(mt) = std::env::var("DEEPSEEK_MAX_TOKENS") { if let Ok(v) = mt.parse() { cfg.max_tokens = v; } }
        if let Ok(cl) = std::env::var("DEEPSEEK_CONTEXT_LIMIT") { if let Ok(v) = cl.parse() { cfg.context_limit = v; } }
        if let Ok(e) = std::env::var("DEEPSEEK_EFFORT") { let e = e.to_lowercase(); if e == "high" || e == "max" { cfg.effort = Some(e); } }
        if let Ok(pl) = std::env::var("DEEPSEEK_PROMPT_LANG") { let pl = pl.to_lowercase(); if pl == "zh" || pl == "en" { cfg.prompt_lang = pl; } }

        // Ensure default profile exists
        if !cfg.profiles.contains_key("default") {
            cfg.profiles.insert("default".into(), dsx_types::ProfileConfig {
                model: cfg.model.clone(), max_tokens: cfg.max_tokens,
                effort: cfg.effort.clone(), context_limit: cfg.context_limit,
                base_url: cfg.base_url.clone(), prompt_lang: cfg.prompt_lang.clone(),
            });
        }

        Ok(cfg)
    }

    pub fn save(&self) {
        let path = config_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        // Sync current values into active profile
        let mut profiles = self.profiles.clone();
        profiles.insert(self.active_profile.clone(), dsx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: self.effort.clone(), context_limit: self.context_limit,
            base_url: self.base_url.clone(), prompt_lang: self.prompt_lang.clone(),
        });
        let pc = PersistentConfig {
            api_key: if self.api_key.is_empty() { None } else { Some(self.api_key.clone()) },
            model: Some(self.model.clone()),
            base_url: Some(self.base_url.clone()),
            max_tokens: Some(self.max_tokens),
            context_limit: Some(self.context_limit),
            thinking: None,
            effort: self.effort.clone(),
            prompt_lang: Some(self.prompt_lang.clone()),
            profiles: Some(profiles),
            active_profile: Some(self.active_profile.clone()),
            auto_mode: Some(self.auto_mode),
            phase_configs: Some(self.phase_configs.clone()),
        };
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&pc).unwrap_or_default());
    }

    pub fn apply_profile(&mut self, name: &str) -> Option<String> {
        let profile = self.profiles.get(name)?.clone();
        self.model = profile.model;
        self.max_tokens = profile.max_tokens;
        self.effort = profile.effort;
        self.context_limit = profile.context_limit;
        self.base_url = profile.base_url;
        self.prompt_lang = profile.prompt_lang;
        self.active_profile = name.to_string();
        self.save();
        Some(name.to_string())
    }

    pub fn save_profile(&mut self, name: &str) {
        self.profiles.insert(name.to_string(), dsx_types::ProfileConfig {
            model: self.model.clone(), max_tokens: self.max_tokens,
            effort: self.effort.clone(), context_limit: self.context_limit,
            base_url: self.base_url.clone(), prompt_lang: self.prompt_lang.clone(),
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

fn config_path() -> PathBuf {
    dsx_types::platform::config_path()
}
