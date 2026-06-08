use dsx_proto::{Agent2Ui, DocInfo, RoundBlock, RoundDeltaKind, TurnData};
use dsx_types::{ConfigStore, PersistentConfig, SessionMeta};
use crate::markdown::MarkdownRenderer;

// ── Active screen ──

#[derive(PartialEq)]
pub enum Screen {
    Session,
    Chat,
    Menu,
}

// ── Setup wizard state ──

pub struct SetupState {
    pub step: usize,
    pub lang: crate::i18n::Lang,
    pub api_key: String,
    pub model: String,
    pub model_list: Vec<String>,
    pub model_index: usize,
    pub context_limit: String,
    pub error: String,
    pub status: String,
    pub models_loaded: bool,
}

impl SetupState {
    pub fn new() -> Self {
        let (pid, ep) = dsx_agent::gate::registry::first_provider_endpoint();
        let def_model = dsx_agent::gate::registry::default_model_for(&pid, &ep);
        let model_list = dsx_agent::gate::registry::find_endpoint(&pid, &ep)
            .map(|e| if e.models.is_empty() { vec![e.default_model.clone()] } else { e.models.clone() })
            .unwrap_or_else(|| vec![def_model.clone()]);
        let model_index = model_list.iter().position(|m| m == &def_model).unwrap_or(0);
        Self {
            step: 0,
            lang: crate::i18n::Lang::En,
            api_key: String::new(),
            model: def_model,
            model_list,
            model_index,
            context_limit: String::from("1000000"),
            error: String::new(),
            status: String::new(),
            models_loaded: false,
        }
    }

    pub fn total_steps(&self) -> usize { 4 }

    pub fn fill_from_store(&mut self, store: &ConfigStore) {
        if let Some(key) = store.load_api_key() {
            self.api_key = key;
        }
        if let Some(v) = store.load_value() {
            if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
                self.model = m.to_string();
                if let Some(idx) = self.model_list.iter().position(|n| n == m) {
                    self.model_index = idx;
                }
            }
            if let Some(c) = v.get("context_limit").and_then(|c| c.as_u64()) {
                self.context_limit = c.to_string();
            }
            if let Some(l) = v.get("lang").and_then(|l| l.as_str()) {
                self.lang = crate::i18n::Lang::from_str(l);
            }
        }
    }

    pub fn current_value(&self) -> &str {
        match self.step {
            0 => self.lang.as_str(),
            1 => &self.api_key,
            2 => &self.model,
            3 => &self.context_limit,
            _ => "",
        }
    }

    fn current_value_mut(&mut self) -> &mut String {
        match self.step {
            1 => &mut self.api_key,
            2 => &mut self.model,
            3 => &mut self.context_limit,
            _ => &mut self.error,
        }
    }

    pub fn backspace(&mut self) {
        if self.step >= 1 {
            self.current_value_mut().pop();
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.step >= 1 {
            self.current_value_mut().push(c);
        }
    }

    pub fn clear_field(&mut self) {
        if self.step >= 1 {
            self.current_value_mut().clear();
        }
    }

    pub fn next(&mut self) -> bool {
        self.validate();
        if !self.error.is_empty() {
            return false;
        }
        self.step += 1;
        self.step >= self.total_steps()
    }

    fn validate(&mut self) {
        self.error.clear();
        match self.step {
            0 => {} // language always valid
            1 => {
                self.api_key = self.api_key.trim().to_string();
                if self.api_key.is_empty() {
                    self.error = self.lang.t_key_invalid().to_string();
                }
            }
            2 => {
                self.model = self.model.trim().to_string();
                if self.model.is_empty() {
                    self.error = self.lang.t_setup_model_required().to_string();
                }
            }
            3 => {
                if let Ok(v) = self.context_limit.parse::<u32>() {
                    if v < 1024 {
                        self.error = self.lang.t_setup_context_min().to_string();
                    }
                } else {
                    self.error = self.lang.t_setup_invalid_number().to_string();
                }
            }
            _ => {}
        }
    }

    /// Validate API key by checking connectivity. Model list already comes from registry presets.
    /// Returns true if the key appears valid (non-empty + connects).
    pub fn fetch_models(&mut self, provider_id: &str) -> bool {
        let key = self.api_key.trim();
        if key.is_empty() {
            return false;
        }
        let ep_id = dsx_agent::gate::registry::first_endpoint_for(provider_id)
            .map(|e| e.id).unwrap_or_else(|| "openai".into());
        let url = match dsx_agent::gate::registry::models_url_for(provider_id, &ep_id) {
            Some(u) => u,
            None => return false,
        };
        let resp = ureq::get(&url)
            .header("Authorization", &format!("Bearer {}", key))
            .call();
        match resp {
            Ok(_r) => {
                self.models_loaded = true;
                true
            }
            Err(_) => false,
        }
    }

    pub fn cursor_row_offset(&self) -> u16 {
        match self.step {
            0 => 8,
            1 => 6,
            2 => {
                if self.models_loaded {
                    let n = self.model_list.len().min(6);
                    let extra = if self.model_list.len() > 6 { 1 } else { 0 };
                    6 + n as u16 + extra as u16
                } else {
                    3
                }
            }
            3 => 5,
            _ => 8,
        }
    }

    pub fn toggle_lang(&mut self) {
        self.lang = match self.lang {
            crate::i18n::Lang::En => crate::i18n::Lang::Zh,
            crate::i18n::Lang::Zh => crate::i18n::Lang::En,
        };
    }

    pub fn to_persistent_config(&self) -> PersistentConfig {
        let (pid, ep) = dsx_agent::gate::registry::first_provider_endpoint();
        let base_url = dsx_agent::gate::registry::base_url_for(&pid, &ep);
        PersistentConfig {
            api_key: Some(self.api_key.trim().to_string()),
            model: Some(self.model.trim().to_string()),
            base_url: Some(base_url),
            context_limit: Some(self.context_limit.parse().unwrap_or(1_000_000)),
            lang: Some(self.lang.as_str().to_string()),
            ..Default::default()
        }
    }
}

// ── App state ──

pub struct App {
    pub screen: Screen,
    pub setup: SetupState,
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor: usize,
    /// Previously sent messages for ↑↓ recall.
    pub input_history: Vec<String>,
    /// Current position when browsing history (None = not browsing).
    pub history_idx: Option<usize>,
    /// Draft input saved when entering history browse mode.
    pub draft_input: String,
    pub status: String,
    pub context_tokens: u32,
    pub session_tokens: u64,
    pub cache_hit: u32,
    pub cache_miss: u32,
    pub cache_rates: Vec<f64>,
    pub cache_warning: String,
    pub context_limit: u32,
    pub should_quit: bool,
    pub streaming: bool,
    streaming_kind: ChatRole,
    pub scroll_offset: usize,
    pub frame_count: u64,
    pub sessions: Vec<SessionMeta>,
    pub session_index: usize,
    pub resume_seed: Option<String>,
    pub show_debug: bool,
    pub show_tasks: bool,
    pub show_context: bool,
    pub show_help: bool,
    pub show_thinking: bool,
    /// When the current tool batch started (for elapsed-time animation).
    pub tool_batch_start: Option<std::time::Instant>,
    /// Total tools in the current batch being executed.
    pub tool_batch_total: u32,
    /// Number of tools that have completed in the current batch.
    pub tool_batch_done: u32,
    /// Timestamp of the most recent message push (for pulse-on-new-content).
    pub last_msg_time: std::time::Instant,
    pub debug: DebugState,
    pub ask: Option<AskState>,
    pub balance: String,
    pub validating: bool,
    pub busy: bool,
    streaming_rendered_len: usize,
    draft_round_msg_idx: Option<usize>,
    pending_tail_lines: usize,
    pub last_render: std::time::Instant,
    pub cached_line_count: usize,
    pub line_count_msg_len: usize,
    pub line_count_width: u16,
    md_renderer: Option<MarkdownRenderer>,
    // Input caching
    pub cached_input_lines: Vec<ratatui::text::Line<'static>>,
    pub cached_input_len: usize,
}

pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub lines: Vec<ratatui::text::Line<'static>>,
    pub tool_status: ToolStatus,
    pub tool_id: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ToolStatus {
    None,
    Pending,
    Success,
    Failed,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ChatRole {
    User,
    Thinking,
    Assistant,
    Tool,
    Divider,
    Status,
}

#[derive(Clone)]
pub struct AskState {
    pub question: String,
    pub options: Vec<String>,
    pub selected: usize,
    pub custom_input: String,
}

#[derive(Clone)]
pub struct DebugState {
    pub hp_connected: bool,
    pub session_seed: String,
    pub context_tokens: u32,
    pub tool_calls_total: u32,
    pub tool_failures: u32,
    pub streaming: bool,
    pub dsml_compat_count: u32,
    pub documents: Vec<DocInfo>,
}

#[derive(Clone)]
pub struct MenuState {
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub editing: bool,
    pub edit_buf: String,
    pub status: String,
    pub profiles: std::collections::HashMap<String, dsx_types::ProfileConfig>,
    pub lang: crate::i18n::Lang,
    session_seed: String,
}

#[derive(Clone)]
pub struct MenuItem {
    pub kind: MenuItemKind,
    pub label: String,
    pub value: String,
    pub editable: bool,
    pub key: String,
    pub secret: String,
}

#[derive(Clone, PartialEq)]
pub enum MenuItemKind {
    Section,
    Toggle,
    Value,
    Action,
}

impl MenuState {
    pub fn new(app: &App) -> Self {
        let store = ConfigStore::default_location();
        let config = store.load();
        let l = app.setup.lang;

        let api_key = store.load_api_key().unwrap_or_default();
        // item.value stores the REAL key; masking is done only in render_menu
        let api_key_display = if api_key.is_empty() {
            if l.as_str() == "zh" { "(未设置)" } else { "(not set)" }.into()
        } else if api_key.len() > 3 {
            format!("sk-{}", "●".repeat(api_key.len().saturating_sub(3).min(20)))
        } else {
            api_key.clone()
        };

        let (default_pid, default_ep) = dsx_agent::gate::registry::first_provider_endpoint();
        let default_base = dsx_agent::gate::registry::base_url_for(&default_pid, &default_ep);
        let default_model = dsx_agent::gate::registry::default_model_for(&default_pid, &default_ep);

        let model = config.as_ref().and_then(|c| c.model.clone()).unwrap_or_else(|| default_model.clone());
        let context_limit = config.as_ref().and_then(|c| c.context_limit).unwrap_or(1_000_000);
        let max_tokens = config.as_ref().and_then(|c| c.max_tokens).unwrap_or(16384);
        let provider_id = config.as_ref().and_then(|c| c.provider_id.clone()).unwrap_or_else(|| default_pid.clone());
        let endpoint = config.as_ref().and_then(|c| c.endpoint.clone()).unwrap_or_else(|| default_ep.clone());
        let protocol = dsx_agent::gate::registry::protocol_for(&provider_id, &endpoint);
        let reasoning_effort = config.as_ref().and_then(|c| c.reasoning_effort.clone()).unwrap_or_else(|| "high".into());
        let base_url = config.as_ref().and_then(|c| c.base_url.clone()).unwrap_or_else(|| default_base);
        let active_profile = config.as_ref().and_then(|c| c.active_profile.clone()).unwrap_or_else(|| "default".into());
        let profiles = config.as_ref().and_then(|c| c.profiles.clone()).unwrap_or_default();

        let c7_key = config.as_ref().and_then(|c| c.context7_api_key.clone()).unwrap_or_default();

        let session_seed = app.debug.session_seed.clone();
        let ws_root = if session_seed.is_empty() {
            ".".to_string()
        } else {
            let ws_path = dsx_types::platform::sessions_dir().join(&session_seed).join("workspace.txt");
            std::fs::read_to_string(&ws_path).unwrap_or_default().trim().to_string()
        };
        let ws_root_display = if ws_root.is_empty() { "." } else { &ws_root };

        let mut items: Vec<MenuItem> = Vec::new();
        let mk = |kind, key: &str, label: String, value: &str, editable: bool| MenuItem {
            kind, key: key.into(), label, value: value.into(), editable, secret: String::new(),
        };

        // ── Provider ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_provider().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "provider_id", l.t_menu_provider_id().into(),
            &provider_id, true));
        items.push(mk(MenuItemKind::Toggle, "endpoint", l.t_menu_endpoint().into(),
            &endpoint, true));
        let proto_disp = format!("{} (auto)", protocol);
        items.push(mk(MenuItemKind::Value, "protocol", l.t_menu_protocol().into(),
            &proto_disp, false));
        items.push(mk(MenuItemKind::Value, "base_url", l.t_menu_base_url().into(),
            &base_url, false));

        // ── Agent Behavior ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_agent_behavior().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "reasoning_effort", l.t_menu_reasoning_effort().into(),
            &reasoning_effort, true));
        items.push(mk(MenuItemKind::Value, "context7_api_key", l.t_menu_c7_key().into(),
            if c7_key.is_empty() { "(not set)" } else { "****" }, true));

        // ── Model ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_model_section().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "model", l.t_menu_model().into(), &model, true));
        items.push(mk(MenuItemKind::Toggle, "max_tokens", l.t_menu_max_tokens().into(),
            &max_tokens.to_string(), true));
        items.push(mk(MenuItemKind::Toggle, "context_limit", l.t_menu_context_limit().into(),
            &context_limit.to_string(), true));

        // ── Profiles ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_profiles().into(), "", false));
        let mut profile_names: Vec<String> = profiles.keys().cloned().collect();
        profile_names.sort();
        for name in &profile_names {
            let is_active = name == &active_profile;
            let profile = &profiles[name];
            let active_tag = if is_active {
                if l.as_str() == "zh" { "● 激活" } else { "● active" }
            } else { "" };
            let desc = format!("{} / {}t / {} / {}",
                profile.model, profile.max_tokens,
                profile.effort.as_deref().unwrap_or("high"), active_tag);
            items.push(mk(MenuItemKind::Action, "profile",
                if is_active { format!("▶ {}", name) } else { format!("  {}", name) },
                &desc, true));
        }

        // ── API ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_api().into(), "", false));
        items.push(MenuItem {
            kind: MenuItemKind::Value,
            key: "api_key".into(),
            label: l.t_menu_api_key().into(),
            value: api_key_display,
            secret: api_key,
            editable: true,
        });
        // ── Interface ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_interface().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "language", l.t_menu_language().into(),
            l.as_str(), true));

        // ── Workspace ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_workspace().into(), "", false));
        items.push(mk(MenuItemKind::Value, "workspace_root", l.t_menu_workspace_root().into(),
            ws_root_display, true));

        Self {
            selected: 1,
            editing: false,
            edit_buf: String::new(),
            status: String::new(),
            items,
            profiles,
            lang: l,
            session_seed,
        }
    }

    pub fn toggle(&mut self, app: &mut App) {
        let selected = self.selected;
        let current_key = self.items.get(selected).map(|i| i.key.clone()).unwrap_or_default();
        let current_value = self.items.get(selected).map(|i| i.value.clone()).unwrap_or_default();
        let pid_value = self.items.iter()
            .find(|i| i.key == "provider_id")
            .map(|i| i.value.clone()).unwrap_or_else(|| "deepseek".into());

        // Pre-compute endpoint cycling for the "endpoint" toggle
        let (_endpoints_list, next_endpoint) = if current_key == "endpoint" {
            let providers = dsx_agent::gate::registry::all_providers();
            let eps: Vec<String> = providers.iter()
                .find(|p| p.id == pid_value)
                .map(|p| p.endpoints.iter().map(|e| e.id.clone()).collect())
                .unwrap_or_default();
            let idx = eps.iter().position(|e| *e == current_value).unwrap_or(0);
            let next = eps.get((idx + 1) % eps.len().max(1))
                .cloned().unwrap_or_else(|| "openai".into());
            (eps, next)
        } else {
            (Vec::new(), String::new())
        };

        let item = match self.items.get_mut(self.selected) {
            Some(i) => i,
            None => return,
        };
        if !item.editable { return; }

        match item.key.as_str() {
            "provider_id" => {
                let providers = dsx_agent::gate::registry::all_providers();
                let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
                if let Some(cur) = ids.iter().position(|&p| p == item.value) {
                    item.value = ids[(cur + 1) % ids.len()].to_string();
                }
                let new_pid = &item.value;
                let ep = dsx_agent::gate::registry::first_endpoint_for(new_pid)
                    .map(|e| e.id).unwrap_or_else(|| "openai".into());
                let proto = dsx_agent::gate::registry::protocol_for(new_pid, &ep);
                let burl = dsx_agent::gate::registry::base_url_for(new_pid, &ep);
                let def_model = dsx_agent::gate::registry::default_model_for(new_pid, &ep);
                if let Some(e) = self.items.iter_mut().find(|i| i.key == "endpoint") {
                    e.value = ep;
                }
                if let Some(p) = self.items.iter_mut().find(|i| i.key == "protocol") {
                    p.value = format!("{} (auto)", proto);
                }
                if let Some(u) = self.items.iter_mut().find(|i| i.key == "base_url") {
                    u.value = burl;
                }
                if let Some(m) = self.items.iter_mut().find(|i| i.key == "model") {
                    m.value = def_model;
                }
            }
            "endpoint" => {
                item.value = next_endpoint.clone();
                let proto = dsx_agent::gate::registry::protocol_for(&pid_value, &next_endpoint);
                let burl = dsx_agent::gate::registry::base_url_for(&pid_value, &next_endpoint);
                if let Some(proto_item) = self.items.iter_mut().find(|i| i.key == "protocol") {
                    proto_item.value = format!("{} (auto)", proto);
                }
                if !burl.is_empty() {
                    if let Some(url_item) = self.items.iter_mut().find(|i| i.key == "base_url") {
                        url_item.value = burl;
                    }
                }
            }
            "reasoning_effort" => {
                item.value = if item.value == "high" { "max".into() } else { "high".into() };
            }
            "model" => {
                let ep = dsx_agent::gate::registry::find_endpoint(&pid_value, "");
                let models: Vec<String> = ep
                    .map(|e| if e.models.is_empty() { vec![e.default_model.clone()] } else { e.models })
                    .unwrap_or_else(|| {
                        let def = dsx_agent::gate::registry::default_model_for(&pid_value, "");
                        vec![def]
                    });
                let current = item.value.as_str();
                let idx = models.iter().position(|m| m == current);
                item.value = match idx {
                    Some(i) if i + 1 < models.len() => models[i + 1].clone(),
                    Some(_) => models[0].clone(),
                    None => models.first().cloned().unwrap_or_else(|| "deepseek-v4-flash".into()),
                };
            }
            "max_tokens" => {
                item.value = match item.value.as_str() {
                    "16384" => "40960".into(),
                    "40960" => "81920".into(),
                    "81920" => "163840".into(),
                    "163840" => "200000".into(),
                    "200000" => "384000".into(),
                    _ => "16384".into(),
                };
            }
            "context_limit" => {
                item.value = match item.value.as_str() {
                    "128000" => "256000".into(),
                    "256000" => "512000".into(),
                    "512000" => "1000000".into(),
                    _ => "128000".into(),
                };
            }
            "language" => {
                app.setup.toggle_lang();
                item.value = app.setup.lang.as_str().to_string();
                // Rebuild items with new lang
                *self = Self::new(app);
                self.status = if app.setup.lang.as_str() == "zh" {
                    "语言已切换为中文".into()
                } else {
                    "Language switched to English".into()
                };
            }
            _ => {
                if item.key == "profile" {
                    let name = item.label.trim_start_matches("▶ ").trim_start_matches("  ").to_string();
                    if item.kind == MenuItemKind::Action && !name.is_empty() {
                        self.activate_profile(&name);
                    }
                }
            }
        }
    }


    fn activate_profile(&mut self, name: &str) {
        let store = ConfigStore::default_location();
        let mut config = store.load().unwrap_or_default();
        config.active_profile = Some(name.to_string());
        store.save(&config);
        self.status = self.lang.t_menu_profile_switched(name);

        for item in &mut self.items {
            if item.key == "profile" {
                let n = item.label.trim_start_matches("▶ ").trim_start_matches("  ").to_string();
                let is_active = n == name;
                item.label = if is_active { format!("▶ {}", n) } else { format!("  {}", n) };
            }
        }
    }

    pub fn save_all(&mut self) {
        let store = ConfigStore::default_location();
        let mut config = store.load().unwrap_or_default();

        for item in &self.items {
            match item.key.as_str() {
                "provider_id" => { config.provider_id = Some(item.value.clone()); }
                "endpoint" => { config.endpoint = Some(item.value.clone()); }
                "reasoning_effort" => { config.reasoning_effort = Some(item.value.clone()); }
                "model" => { config.model = Some(item.value.clone()); }
                "context_limit" => {
                    if let Ok(v) = item.value.parse::<u32>() { config.context_limit = Some(v); }
                }
                "max_tokens" => {
                    if let Ok(v) = item.value.parse::<u32>() { config.max_tokens = Some(v); }
                }
                "language" => {
                    config.lang = Some(item.value.clone());
                }
                "api_key" => {
                    let v = item.secret.trim().to_string();
                    if !v.is_empty() { config.api_key = Some(v); }
                }
                "base_url" => {
                    let v = item.value.trim().to_string();
                    if !v.is_empty() { config.base_url = Some(v); }
                }
                "context7_api_key" => {
                    let v = item.value.trim().to_string();
                    if !v.is_empty() && v != "****" {
                        config.context7_api_key = Some(v);
                    }
                }
                "workspace_root" => {
                    let v = item.value.trim().to_string();
                    if !v.is_empty() && !self.save_workspace(&v) {
                        self.status = self.lang.t_menu_save_failed().into();
                    }
                }
                _ => {}
            }
        }        config.profiles = Some(self.profiles.clone());

        if store.save(&config) {
            self.status = self.lang.t_menu_saved().into();
        } else {
            self.status = self.lang.t_menu_save_failed().into();
        }
    }

    pub fn go_back(mut self, app: &mut App) {
        self.save_all();
        app.screen = Screen::Chat;
    }

    fn save_workspace(&self, path: &str) -> bool {
        if self.session_seed.is_empty() { return false; }
        let dir = dsx_types::platform::sessions_dir().join(&self.session_seed);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("workspace.txt"), path).is_ok()
    }
}

// ── Tool display helpers ──

fn tool_icon(name: &str) -> &'static str {
    match name {
        "read_file" | "file_read" => "📖",
        "write_file" | "file_write" => "📝",
        "edit_file" | "edit_file_diff" => "✏️",
        "file_delete" | "delete_file" => "🗑",
        "file_move" | "move_file" => "📦",
        "file_diff" | "diff" => "🔍",
        "file_glob" | "glob" => "🌐",
        "file_list_dir" | "list_dir" => "📂",
        "file_search" | "search" | "grep" => "🔎",
        "exec" => "⚡",
        "web_fetch" => "🌍",
        "web_search" => "🔗",
        "explore" => "🧭",
        "task_create" | "task_update" => "📋",
        "ask_user" => "❓",
        _ => "🔧",
    }
}

fn format_tool_label(name: &str, args_display: &str) -> String {
    let icon = tool_icon(name);
    match name {
        "exec" => format!("{} exec: {}", icon, args_display),
        _ => format!("{} {} {}", icon, name, args_display),
    }
}

fn format_tool_result_summary(output: &str, success: bool) -> String {
    if !success {
        let first_line = output.lines().next().unwrap_or("failed");
        return format!(" → ✗ {}", first_line.chars().take(80).collect::<String>());
    }
    let first_line = output.lines().next().unwrap_or("done");
    let summary: String = first_line.chars().take(80).collect();
    format!(" → {}", summary)
}


impl App {
    pub fn new(_need_setup: bool) -> Self {
        Self {
            screen: Screen::Session,
            setup: SetupState::new(),
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            input_history: Vec::new(),
            history_idx: None,
            draft_input: String::new(),
            status: String::new(), // will be set after setup knows lang
            context_tokens: 0,
            session_tokens: 0,
            cache_hit: 0,
            cache_miss: 0,
            cache_rates: Vec::new(),
            cache_warning: String::new(),
            context_limit: 1_000_000,
            should_quit: false,
            streaming: false,
            streaming_kind: ChatRole::Assistant,
            scroll_offset: 0,
            streaming_rendered_len: 0,
            draft_round_msg_idx: None,
            pending_tail_lines: 0,
            last_render: std::time::Instant::now(),
            cached_line_count: 0,
            line_count_msg_len: 0,
            line_count_width: 0,
            md_renderer: None,
            cached_input_lines: Vec::new(),
            cached_input_len: 0,
            frame_count: 0,
            sessions: Vec::new(),
            session_index: 0,
            resume_seed: None,
            show_debug: false,
            show_tasks: false,
            show_context: false,
            show_help: false,
            show_thinking: true,
            tool_batch_start: None,
            tool_batch_total: 0,
            tool_batch_done: 0,
            last_msg_time: std::time::Instant::now(),
            debug: DebugState {
                hp_connected: false,
                session_seed: String::new(),
                context_tokens: 0,
                tool_calls_total: 0,
                tool_failures: 0,
                streaming: false,
                dsml_compat_count: 0,
                documents: Vec::new(),
            },
            ask: None,
            balance: String::new(),
            validating: false,
            busy: false,
        }
    }

    pub fn tick(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    fn update_cache(&mut self, hit: u32, miss: u32) {
        let total = hit + miss;
        let rate = if total > 0 { hit as f64 / total as f64 } else { 1.0 };
        self.cache_rates.push(rate);
        if self.cache_rates.len() > 5 { self.cache_rates.remove(0); }

        if self.cache_rates.len() >= 3 {
            let avg: f64 = self.cache_rates.iter().sum::<f64>() / self.cache_rates.len() as f64;
            let all_low = self.cache_rates.iter().all(|&r| r < 0.30);
            self.cache_warning = if all_low {
                self.setup.lang.t_cache_warn_low().into()
            } else if avg < 0.50 {
                self.setup.lang.t_cache_warn_moderate().into()
            } else {
                String::new()
            };
        }
    }

    pub fn spinner(&self) -> &str {
        const CHARS: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];
        CHARS[(self.frame_count as usize / 2) % CHARS.len()]
    }

    /// A dot-pulse indicator: "⠁⠂⠄⡀⢀⠠⠐⠈" cycling faster for idle-wait feel.
    pub fn pulse(&self) -> &str {
        const DOTS: &[&str] = &["⠁", "⠂", "⠄", "⡀", "⠠", "⠐", "⠈"];
        DOTS[(self.frame_count as usize / 2) % DOTS.len()]
    }

    pub fn tasks(&self) -> Vec<(String, String)> {
        if self.debug.session_seed.is_empty() {
            return Vec::new();
        }
        let path = dsx_types::platform::sessions_dir()
            .join(&self.debug.session_seed)
            .join("tasks.md");
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        content
            .lines()
            .filter(|l| l.starts_with("- [") && (l.contains("[pending]") || l.contains("[in_progress]") || l.contains("[completed]")))
            .map(|l| {
                let s = l.trim_start_matches("- ");
                let status = if s.contains("[completed]") { "✓" }
                    else if s.contains("[in_progress]") { "●" }
                    else { "○" };
                (status.to_string(), s.to_string())
            })
            .collect()
    }

    pub fn push_msg(&mut self, role: ChatRole, content: &str) {
        // Flush previous renderer state before starting a new message
        self.finalize_last_message();

        // Trim old messages by total character count to prevent unbounded memory.
        // 250 messages can hold 12+ MB with large tool outputs — cap at ~500k chars.
        const MAX_TOTAL_CHARS: usize = 500_000;
        let mut total: usize = self.messages.iter().map(|m| m.content.chars().count()).sum();
        while total > MAX_TOTAL_CHARS && !self.messages.is_empty() {
            let removed = self.messages.remove(0);
            total = total.saturating_sub(removed.content.chars().count());
            self.scroll_offset = 0; // content shifted — reset scroll
        }

        const MAX_PER_MSG: usize = 50_000;
        let content = if content.chars().count() > MAX_PER_MSG {
            let truncated: String = content.chars().take(MAX_PER_MSG).collect();
            format!("{}...[TRUNCATED]", truncated)
        } else {
            content.to_string()
        };
        let mut renderer = MarkdownRenderer::new();
        let mut lines = Vec::new();
        for line in content.lines() {
            for l in renderer.push_line(line) {
                lines.push(l);
            }
        }
        for l in renderer.flush() {
            lines.push(l);
        }
        self.streaming_rendered_len = content.len();
        self.md_renderer = None;
        self.last_msg_time = std::time::Instant::now();
        self.messages.push(ChatMessage { role, content, lines, tool_status: ToolStatus::None, tool_id: String::new() });
    }

    /// Push a message placeholder for streaming — rendering is deferred to append_last().
    fn push_streaming_msg(&mut self, role: ChatRole, content: &str) {
        self.streaming_rendered_len = 0;
        self.md_renderer = None;  // Reset markdown state to avoid state pollution
        self.pending_tail_lines = 0;
        self.messages.push(ChatMessage { role, content: content.to_string(), lines: Vec::new(), tool_status: ToolStatus::None, tool_id: String::new() });
    }

    fn append_last(&mut self, content: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(content);
            let renderer = self.md_renderer.get_or_insert_with(MarkdownRenderer::new);
            let start = self.streaming_rendered_len.min(last.content.len());
            let full = &last.content[start..];
            let mut processed = 0;

            // Remove previous pending tail (incomplete line rendered as raw text)
            for _ in 0..self.pending_tail_lines {
                last.lines.pop();
            }
            self.pending_tail_lines = 0;

            // Push complete lines through markdown
            while let Some(nl) = full[processed..].find('\n') {
                let line = &full[processed..processed + nl];
                for l in renderer.push_line(line) {
                    last.lines.push(l);
                }
                processed += nl + 1;
            }
            self.streaming_rendered_len += processed;

            // Render remaining incomplete tail as raw text for character-by-character streaming
            let remaining = &full[processed..];
            if !remaining.is_empty() {
                let prefix = if last.lines.is_empty() { "" } else { "" };
                last.lines.push(ratatui::text::Line::from(
                    ratatui::text::Span::raw(format!("{}{}", prefix, remaining))
                ));
                self.pending_tail_lines = 1;
            }
        }
    }

    /// Push a tool message with status tracking.
    fn push_tool_msg(&mut self, tool_id: &str, label: &str, status: ToolStatus) {
        self.finalize_last_message();
        let content = label.to_string();
        let mut renderer = MarkdownRenderer::new();
        let mut lines = Vec::new();
        for line in content.lines() {
            for l in renderer.push_line(line) {
                lines.push(l);
            }
        }
        for l in renderer.flush() {
            lines.push(l);
        }
        self.last_msg_time = std::time::Instant::now();
        self.messages.push(ChatMessage {
            role: ChatRole::Tool,
            content,
            lines,
            tool_status: status,
            tool_id: tool_id.to_string(),
        });
    }

    /// Flush the incremental renderer and write remaining lines to the last message.
    fn finalize_last_message(&mut self) {
        // Remove pending tail (raw streaming preview) before full markdown render
        if let Some(last) = self.messages.last_mut() {
            for _ in 0..self.pending_tail_lines {
                last.lines.pop();
            }
        }
        self.pending_tail_lines = 0;

        if let Some(mut renderer) = self.md_renderer.take() {
            if let Some(last) = self.messages.last_mut() {
                let start = self.streaming_rendered_len.min(last.content.len());
                let remaining = last.content[start..].to_string();
                if !remaining.is_empty() {
                    for l in renderer.push_line(&remaining) {
                        last.lines.push(l);
                    }
                }
            }
            let flushed = renderer.flush();
            if let Some(last) = self.messages.last_mut() {
                for l in flushed {
                    last.lines.push(l);
                }
            }
        }
    }

    pub fn handle_frame(&mut self, frame: Agent2Ui) {
        match frame {
            Agent2Ui::TurnStart { turn_id: _, user_text } => {
                self.streaming = false;
                self.debug.streaming = false;
                self.tool_batch_start = None;
                self.tool_batch_total = 0;
                self.tool_batch_done = 0;
                self.push_msg(ChatRole::Divider, "");
                self.push_msg(ChatRole::User, &user_text);
                self.scroll_offset = 0;
            }
            Agent2Ui::TurnEnd { turn_id: _, stop_reason: _, usage, context_tokens, context_limit, session_tokens } => {
                self.context_tokens = context_tokens;
                self.session_tokens = session_tokens;
                self.context_limit = context_limit;
                if let Some(u) = usage {
                    self.cache_hit = u.prompt_cache_hit_tokens;
                    self.cache_miss = u.prompt_cache_miss_tokens;
                    self.update_cache(u.prompt_cache_hit_tokens, u.prompt_cache_miss_tokens);
                }
                self.streaming = false;
                self.debug.streaming = false;
                self.busy = false;
                self.scroll_offset = 0;
                self.status = self.setup.lang.t_chat_ready().to_string();
            }
            Agent2Ui::RoundDelta { turn_id: _, round_num: _, kind, delta } => {
                self.debug.streaming = true;
                let new_role = match kind {
                    RoundDeltaKind::Thinking => ChatRole::Thinking,
                    RoundDeltaKind::Answering => ChatRole::Assistant,
                    // ToolCalling is metadata, not a content kind switch.
                    // Don't create a new draft — the tool names are transient;
                    // authoritative tool calls come via RoundComplete.
                    RoundDeltaKind::ToolCalling => return,
                };
                // When kind switches (thinking→answering), finalize old draft and start new
                if self.streaming && self.streaming_kind != new_role {
                    self.finalize_last_message();
                    self.streaming_rendered_len = 0;
                    self.md_renderer = None;
                    self.pending_tail_lines = 0;
                    self.streaming = false;
                    // `draft_round_msg_idx` must NOT be overwritten — it always
                    // points to the FIRST draft of this round so RoundComplete's
                    // truncate removes every draft (thinking + answering + tool).
                }
                self.streaming_kind = new_role;
                if !self.streaming {
                    self.streaming = true;
                    // Only set on first draft of the round
                    if self.draft_round_msg_idx.is_none() {
                        self.draft_round_msg_idx = Some(self.messages.len());
                    }
                    self.push_streaming_msg(new_role, "");
                }
                self.append_last(&delta);
            }
            Agent2Ui::RoundComplete { turn_id: _, round_num: _, thinking: _, answer: _, tool_calls: _, is_final, blocks } => {
                if let Some(idx) = self.draft_round_msg_idx.take() {
                    if idx < self.messages.len() {
                        self.messages.truncate(idx);
                    }
                }
                self.streaming = false;
                self.streaming_rendered_len = 0;
                self.md_renderer = None;
                self.pending_tail_lines = 0;
                self.debug.streaming = false;

                let mut tool_count = 0u32;
                for b in &blocks {
                    match b {
                        RoundBlock::Reasoning { content } => {
                            if !content.is_empty() {
                                self.push_msg(ChatRole::Thinking, content);
                            }
                        }
                        RoundBlock::Text { content } => {
                            if !content.is_empty() {
                                self.push_msg(ChatRole::Assistant, content);
                            }
                        }
                        RoundBlock::Tool { card } => {
                            self.debug.tool_calls_total += 1;
                            tool_count += 1;
                            let label = format_tool_label(&card.name, &card.args_display);
                            self.push_tool_msg(&card.id, &label, ToolStatus::Pending);
                        }
                    }
                }

                if tool_count > 0 {
                    self.tool_batch_start = Some(std::time::Instant::now());
                    self.tool_batch_total = tool_count;
                    self.tool_batch_done = 0;
                } else {
                    self.tool_batch_start = None;
                    self.tool_batch_total = 0;
                    self.tool_batch_done = 0;
                }
                if is_final {
                    self.status = self.setup.lang.t_chat_ready().to_string();
                    self.busy = false;
                    self.scroll_offset = 0;
                }
            }
            Agent2Ui::ToolResults { turn_id: _, round_num: _, results } => {
                for r in &results {
                    if !r.success { self.debug.tool_failures += 1; }
                    if let Some(msg) = self.messages.iter_mut().rev()
                        .find(|m| m.role == ChatRole::Tool && m.tool_id == r.tool_call_id)
                    {
                        let summary = format_tool_result_summary(&r.output, r.success);
                        msg.content = format!("{} {}", msg.content, summary);
                        msg.tool_status = if r.success { ToolStatus::Success } else { ToolStatus::Failed };
                        let mut renderer = MarkdownRenderer::new();
                        let mut new_lines = Vec::new();
                        for line in msg.content.lines() {
                            for l in renderer.push_line(line) {
                                new_lines.push(l);
                            }
                        }
                        for l in renderer.flush() {
                            new_lines.push(l);
                        }
                        msg.lines = new_lines;
                    }
                }
                // Advance tool batch progress for elapsed / gauge animation
                self.tool_batch_done += results.len() as u32;
                if self.tool_batch_done >= self.tool_batch_total {
                    self.tool_batch_start = None;
                    self.tool_batch_total = 0;
                    self.tool_batch_done = 0;
                }
            }
            Agent2Ui::SessionRestored { seed, turns, tokens_used, .. } => {
                self.status = self.setup.lang.t_session_restored(&seed, turns.len() as u64, tokens_used);
                self.debug.session_seed = seed.clone();
                self.debug.context_tokens = tokens_used;
                self.session_tokens = tokens_used as u64;
                self.scroll_offset = 0;
                self.load_turns(&turns);
            }
            Agent2Ui::Error { message } => {
                let status_text = format!("{}: {}", self.setup.lang.t_chat_error(), message);
                self.push_msg(ChatRole::Status, &status_text);
                self.status = status_text;
            }
            Agent2Ui::ToolNotice { ref message, ref level } => {
                let prefix = if level == "error" { "\u{26a0}" } else { "\u{2139}" };
                let text = format!("{prefix} {message}");
                self.push_msg(ChatRole::Status, &text);
            }
            Agent2Ui::Done => {
                self.status = self.setup.lang.t_chat_ready().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.busy = false;
                self.scroll_offset = 0;
                self.finalize_last_message();
            }
            Agent2Ui::Cancelled => {
                self.status = self.setup.lang.t_chat_cancelled().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.busy = false;
                self.scroll_offset = 0;
                self.finalize_last_message();
            }
            Agent2Ui::DebugSnapshot { hp_connected, session_seed, context_tokens,
                tool_calls_total, tool_failures, current_phase: _, streaming, dsml_compat_count, documents, .. } => {
                self.debug.hp_connected = hp_connected;
                self.debug.session_seed = session_seed;
                self.debug.context_tokens = context_tokens;
                self.debug.tool_calls_total = tool_calls_total;
                self.debug.tool_failures = tool_failures;
                self.debug.streaming = streaming;
                self.debug.dsml_compat_count = dsml_compat_count;
                self.debug.documents = documents;
            }
            Agent2Ui::AskUser { question, options, .. } => {
                let mut opts = options.unwrap_or_default();
                opts.push(String::new());
                self.ask = Some(AskState {
                    question,
                    options: opts,
                    selected: 0,
                    custom_input: String::new(),
                });
            }
            Agent2Ui::Balance { is_available, total_balance, currency } => {
                let status = if is_available { "✓" } else { "✗" };
                self.balance = format!("{} {}{} {}", status, if currency == "CNY" { "¥" } else { "$" }, total_balance, currency);
            }
            Agent2Ui::ShutdownAck => {
                self.streaming = false;
                self.debug.streaming = false;
                if self.setup.lang.as_str() == "zh" {
                    self.status = "Agent 已关闭".into();
                } else {
                    self.status = "Agent shut down".into();
                }
            }
            _ => {}
        }
    }

    fn load_turns(&mut self, turns: &[TurnData]) {
        for turn in turns {
            self.push_msg(ChatRole::Divider, "");
            self.push_msg(ChatRole::User, &turn.user_text);
            for round in &turn.rounds {
                if let Some(ref t) = round.thinking {
                    if !t.is_empty() {
                        self.push_msg(ChatRole::Thinking, t);
                    }
                }
                if let Some(ref a) = round.answer {
                    if !a.is_empty() {
                        self.push_msg(ChatRole::Assistant, a);
                    }
                }
                for tc in &round.tool_calls {
                    let label = format_tool_label(&tc.name, &tc.args_display);
                    let mut status = ToolStatus::Success;
                    // Check if there's a matching result
                    if let Some(tr) = round.tool_results.iter().find(|r| r.tool_call_id == tc.id) {
                        if !tr.success { status = ToolStatus::Failed; }
                    }
                    self.push_tool_msg(&tc.id, &label, status);
                }
            }
        }
    }
}
