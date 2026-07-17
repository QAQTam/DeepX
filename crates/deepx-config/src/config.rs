use deepx_types::{
    ConfigStore, PersistentConfig, PersistentDatabaseConfig, PersistentSubagentConfig,
};
use std::collections::HashMap; // still used by profiles

/// Subagent default configuration.
///
/// These are defaults applied when spawning sub-agents. Individual
/// `spawn_subagent` tool calls can override these on a per-instance basis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubagentConfig {
    /// Override model. Empty = inherit from parent agent config.
    #[serde(default)]
    pub model: String,
    /// Override API base URL. Empty = inherit.
    #[serde(default)]
    pub base_url: String,
    /// Override API key. Empty = inherit.
    #[serde(default)]
    pub api_key: String,
    /// Max output tokens for subagent responses. Default: 4096.
    #[serde(default = "default_subagent_max_tokens")]
    pub max_tokens: u32,
    /// Maximum lifetime in seconds before the subagent is killed. Default: 120.
    #[serde(default = "default_subagent_timeout")]
    pub timeout_secs: u64,
    /// Default tool allowlist. Empty = all tools available.
    #[serde(default)]
    pub default_tools: Vec<String>,
}

fn default_subagent_max_tokens() -> u32 {
    4096
}
fn default_subagent_timeout() -> u64 {
    120
}

impl Default for SubagentConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            max_tokens: 4096,
            timeout_secs: 120,
            default_tools: vec!["file".into(), "exec".into(), "explore".into()],
        }
    }
}

/// Database mirror configuration (Turso local SQLite database).
///
/// When enabled, session messages are dual-written to both JSONL and a
/// local SQLite database for fast querying from external tools.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DatabaseConfig {
    /// Whether the database mirror is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Path to the local Turso database file. `None` = default location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: None,
        }
    }
}

/// Runtime agent configuration built from PersistentConfig + registry.
///
/// This is the fully-resolved config used by the agent at runtime. It combines
/// user settings from config.toml with provider registry defaults and profile
/// overrides. All fields are concrete (no Option wrapping).
#[derive(Debug, Clone)]
pub struct Config {
    /// API key for the selected provider.
    pub api_key: String,
    /// Base URL for API requests (from provider registry).
    pub base_url: String,
    /// Active model identifier.
    pub model: String,
    /// Max output tokens per turn.
    pub max_tokens: u32,
    /// Maximum context window size in tokens.
    pub context_limit: u32,
    /// Selected provider ID (e.g. "deepseek", "qwen").
    pub provider_id: String,
    /// Selected endpoint within the provider (e.g. "openai").
    pub endpoint: String,
    /// Reasoning effort: "high", "max", or empty.
    pub reasoning_effort: String,
    /// Named profiles for quick config switching.
    pub profiles: HashMap<String, deepx_types::ProfileConfig>,
    /// Currently active profile name.
    pub active_profile: String,
    /// UI language preference.
    pub lang: Option<String>,
    /// Default configuration for sub-agent spawning.
    pub subagent: SubagentConfig,
    /// Whether the content filter is active.
    pub compliance_enabled: bool,
    /// Additional banned keywords for the content filter.
    pub compliance_extra_keywords: Vec<String>,
    /// Whitelisted patterns exempt from content filtering.
    pub compliance_allowlist: Vec<String>,
    /// Local Turso SQLite database mirror configuration.
    pub database: DatabaseConfig,
    /// Agent permission level:
    /// 1 = MaxLockdown, 2 = ReadFree, 3 = WorkspaceFree, 4 = Unrestricted.
    pub permission_level: u8,
    /// Path to a HuggingFace tokenizer.json. `None` = use heuristic fallback.
    pub tokenizer_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let (provider_id, endpoint) = crate::registry::first_provider_endpoint();
        let base_url = crate::registry::base_url_for(&provider_id, &endpoint);
        let model = crate::registry::default_model_for(&provider_id, &endpoint);

        let mut profiles = HashMap::new();
        profiles.insert(
            "default".into(),
            deepx_types::ProfileConfig {
                model: model.clone(),
                max_tokens: 16384,
                effort: Some("high".into()),
                context_limit: 1_000_000,
                base_url: base_url.clone(),
                endpoint: None,
            },
        );
        Self {
            api_key: String::new(),
            base_url,
            model,
            max_tokens: 16384,
            context_limit: 1_000_000,
            provider_id,
            endpoint,
            reasoning_effort: "high".into(),
            profiles,
            active_profile: "default".into(),
            lang: None,
            subagent: SubagentConfig::default(),
            compliance_enabled: true,
            compliance_extra_keywords: Vec::new(),
            compliance_allowlist: Vec::new(),
            database: DatabaseConfig::default(),
            permission_level: 4, // Unrestricted — backward compat
            tokenizer_path: None,
        }
    }
}

impl Config {
    /// Load config from disk (TOML, with optional Turso DB dual-read).
    ///
    /// # Loading order
    /// 1. Read config.toml (always)
    /// 2. If database.enabled, try reading config.db (SQLite mirror, may be newer)
    /// 3. Apply provider registry defaults for missing fields
    /// 4. Apply active profile overrides
    pub fn load() -> Result<Self, String> {
        let store = ConfigStore::default_location();
        let mut cfg = Self::default();

        // Step 1: always read TOML to get database.enabled flag (bootstrap)
        let pc_toml = store.load();

        // Step 2: if database mirror is enabled, try ConfigDb (which may have newer data)
        let db_enabled = pc_toml
            .as_ref()
            .and_then(|pc| pc.database.as_ref())
            .and_then(|db| db.enabled)
            .unwrap_or(true); // default: enabled

        // Clone TOML data before potential move
        let pc_toml_for_override = pc_toml.clone();

        let pc = if db_enabled {
            #[cfg(feature = "turso-backend")]
            {
                match Self::try_load_from_db() {
                    Ok(Some(db_pc)) => {
                        log::info!("[Config] loaded from config.db");
                        Some(db_pc)
                    }
                    Ok(None) => {
                        // ConfigDb has no data yet (first boot after enabling)
                        pc_toml
                    }
                    Err(e) => {
                        log::warn!("[Config] config.db load failed: {e}, falling back to TOML");
                        pc_toml
                    }
                }
            }
            #[cfg(not(feature = "turso-backend"))]
            pc_toml
        } else {
            pc_toml
        };

        if let Some(pc) = pc {
            // ── Backward compat: migrate old provider_id → new (provider_id, endpoint) ──
            let raw_pid = pc.provider_id.unwrap_or_default();
            let (provider_id, endpoint) = if raw_pid.is_empty() {
                crate::registry::first_provider_endpoint()
            } else {
                crate::registry::migrate_provider_id(&raw_pid)
            };
            cfg.provider_id = provider_id;
            // New endpoint field takes priority over backward-compat migration
            cfg.endpoint = pc.endpoint.filter(|e| !e.is_empty()).unwrap_or(endpoint);

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
                        && !ep.is_empty()
                    {
                        cfg.endpoint = ep.clone();
                        let ep_burl = crate::registry::base_url_for(&cfg.provider_id, ep);
                        if !ep_burl.is_empty() && ep_burl != cfg.base_url {
                            cfg.base_url = ep_burl;
                        }
                    }
                }
            }
            if let Some(k) = pc.api_key
                && !k.is_empty()
            {
                cfg.api_key = k;
            }
            if let Some(m) = pc.model
                && !m.is_empty()
            {
                cfg.model = m;
            }
            // User base_url override: only apply if differs from all known endpoint defaults
            if let Some(ref u) = pc.base_url
                && !u.is_empty()
            {
                let is_ep_default = crate::registry::all_providers()
                    .iter()
                    .flat_map(|p| &p.endpoints)
                    .any(|e| e.base_url == *u || e.models_url.as_deref() == Some(u.as_str()));
                if !is_ep_default {
                    cfg.base_url = u.clone();
                }
            }
            if let Some(mt) = pc.max_tokens {
                cfg.max_tokens = mt;
            }
            if let Some(cl) = pc.context_limit {
                cfg.context_limit = cl;
            }
            if let Some(ref l) = pc.lang
                && !l.is_empty()
            {
                cfg.lang = Some(l.clone());
            }
            if let Some(ref l) = pc.lang
                && !l.is_empty()
            {
                cfg.lang = Some(l.clone());
            }
            // ── Subagent defaults ──
            if let Some(ref s) = pc.subagent {
                if let Some(ref m) = s.model
                    && !m.is_empty()
                {
                    cfg.subagent.model = m.clone();
                }
                if let Some(ref u) = s.base_url
                    && !u.is_empty()
                {
                    cfg.subagent.base_url = u.clone();
                }
                if let Some(ref k) = s.api_key
                    && !k.is_empty()
                {
                    cfg.subagent.api_key = k.clone();
                }
                if let Some(mt) = s.max_tokens {
                    cfg.subagent.max_tokens = mt;
                }
                if let Some(ts) = s.timeout_secs {
                    cfg.subagent.timeout_secs = ts;
                }
                if let Some(ref tools) = s.default_tools {
                    cfg.subagent.default_tools = tools.clone();
                }
            }

            // ── Compliance ──
            if let Some(enabled) = pc.compliance_enabled {
                cfg.compliance_enabled = enabled;
            }
            if let Some(ref keywords) = pc.compliance_extra_keywords {
                cfg.compliance_extra_keywords = keywords.clone();
            }
            if let Some(ref allowlist) = pc.compliance_allowlist {
                cfg.compliance_allowlist = allowlist.clone();
            }

            // ── Database (Turso mirror) ──
            if let Some(ref db) = pc.database {
                if let Some(enabled) = db.enabled {
                    cfg.database.enabled = enabled;
                }
                cfg.database.url = db.url.clone();
            }

            // ── Permission ──
            if let Some(pl) = pc.permission_level {
                cfg.permission_level = pl;
            }

            // ── Tokenizer ──
            if let Some(ref tp) = pc.tokenizer_path {
                cfg.tokenizer_path = Some(tp.clone());
            }
        }

        // TOML is authoritative for database.enabled (prevents stale ConfigDb value)
        if let Some(ref pc_toml) = pc_toml_for_override {
            if let Some(ref db) = pc_toml.database {
                if let Some(enabled) = db.enabled {
                    cfg.database.enabled = enabled;
                }
            }
        }

        if !cfg.profiles.contains_key("default") {
            cfg.profiles.insert(
                "default".into(),
                deepx_types::ProfileConfig {
                    model: cfg.model.clone(),
                    max_tokens: cfg.max_tokens,
                    effort: Some(cfg.reasoning_effort.clone()),
                    context_limit: cfg.context_limit,
                    base_url: cfg.base_url.clone(),
                    endpoint: Some(cfg.endpoint.clone()),
                },
            );
        }

        // Initialize tokenizer if configured
        if let Some(ref path) = cfg.tokenizer_path {
            let _ = deepx_types::token::init_tokenizer(path);
        }

        Ok(cfg)
    }

    pub fn save(&self) -> Result<(), String> {
        let store = ConfigStore::default_location();
        let mut profiles = self.profiles.clone();
        profiles.insert(
            self.active_profile.clone(),
            deepx_types::ProfileConfig {
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                effort: Some(self.reasoning_effort.clone()),
                context_limit: self.context_limit,
                base_url: self.base_url.clone(),
                endpoint: Some(self.endpoint.clone()),
            },
        );
        let pc = PersistentConfig {
            api_key: if self.api_key.is_empty() {
                None
            } else {
                Some(self.api_key.clone())
            },
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
            subagent: Some(PersistentSubagentConfig {
                model: if self.subagent.model.is_empty() {
                    None
                } else {
                    Some(self.subagent.model.clone())
                },
                base_url: if self.subagent.base_url.is_empty() {
                    None
                } else {
                    Some(self.subagent.base_url.clone())
                },
                api_key: if self.subagent.api_key.is_empty() {
                    None
                } else {
                    Some(self.subagent.api_key.clone())
                },
                max_tokens: Some(self.subagent.max_tokens),
                timeout_secs: Some(self.subagent.timeout_secs),
                default_tools: if self.subagent.default_tools.is_empty() {
                    None
                } else {
                    Some(self.subagent.default_tools.clone())
                },
            }),
            compliance_enabled: Some(self.compliance_enabled),
            compliance_extra_keywords: if self.compliance_extra_keywords.is_empty() {
                None
            } else {
                Some(self.compliance_extra_keywords.clone())
            },
            compliance_allowlist: if self.compliance_allowlist.is_empty() {
                None
            } else {
                Some(self.compliance_allowlist.clone())
            },
            database: Some(PersistentDatabaseConfig {
                enabled: Some(self.database.enabled),
                url: self.database.url.clone(),
            }),
            permission_level: Some(self.permission_level),
            tokenizer_path: self.tokenizer_path.clone(),
        };
        if !store.save(&pc) {
            return Err(format!(
                "Failed to save config to {}",
                deepx_types::platform::config_path().display()
            ));
        }

        // Dual-write: mirror to SQLite when database is enabled
        if self.database.enabled {
            #[cfg(feature = "turso-backend")]
            {
                let json = serde_json::to_string(&pc).unwrap_or_default();
                if let Err(e) = Self::save_to_db(&json) {
                    log::warn!("[Config] save to config.db failed: {e}");
                }
            }
            #[cfg(not(feature = "turso-backend"))]
            let _ = ();
        }

        Ok(())
    }

    /// Try loading config from config.db. Returns Ok(None) if db is empty/unavailable.
    #[cfg(feature = "turso-backend")]
    fn try_load_from_db() -> Result<Option<PersistentConfig>, String> {
        let db_path = deepx_types::platform::data_dir().join("config.db");
        if !db_path.exists() {
            return Ok(None);
        }
        let db = crate::config_db::ConfigDb::open(&db_path)?;
        if let Err(e) = db.init_table() {
            log::warn!("[Config] config.db init failed: {e}");
            return Ok(None);
        }
        let json_str = match db.load_config() {
            Ok(Some(s)) => s,
            Ok(None) => return Ok(None),
            Err(e) => return Err(e),
        };
        let pc: PersistentConfig = serde_json::from_str(&json_str)
            .map_err(|e| format!("deserialize config from db: {e}"))?;
        Ok(Some(pc))
    }

    #[cfg(not(feature = "turso-backend"))]
    fn try_load_from_db() -> Result<Option<PersistentConfig>, String> {
        Ok(None)
    }

    /// Write config JSON to config.db.
    #[cfg(feature = "turso-backend")]
    fn save_to_db(json: &str) -> Result<(), String> {
        let db_path = deepx_types::platform::data_dir().join("config.db");
        let db = crate::config_db::ConfigDb::open(&db_path)?;
        db.init_table()?;
        db.save_config(json)
    }

    #[cfg(not(feature = "turso-backend"))]
    fn save_to_db(_json: &str) -> Result<(), String> {
        Ok(())
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
        self.profiles.insert(
            name.to_string(),
            deepx_types::ProfileConfig {
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                effort: Some(self.reasoning_effort.clone()),
                context_limit: self.context_limit,
                base_url: self.base_url.clone(),
                endpoint: Some(self.endpoint.clone()),
            },
        );
        self.active_profile = name.to_string();
        let _ = self.save();
    }

    pub fn delete_profile(&mut self, name: &str) -> bool {
        if name == "default" {
            return false;
        }
        if self.profiles.remove(name).is_some() {
            let _ = self.save();
            true
        } else {
            false
        }
    }

    pub fn is_ready(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Whether per-session Turso SQLite mirroring is enabled.
    pub fn turso_enabled(&self) -> bool {
        self.database.enabled
    }

    /// Protocol derived from (provider_id, endpoint) in the registry.
    pub fn protocol(&self) -> String {
        crate::registry::protocol_for(&self.provider_id, &self.endpoint)
    }
}
