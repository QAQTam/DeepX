use dsx_proto::{Agent2Ui, DocInfo, RoundDeltaKind, TurnData};
use dsx_types::{ConfigStore, PersistentConfig, SessionMeta, SessionFile, ContentBlock};
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Style};
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
        Self {
            step: 0,
            lang: crate::i18n::Lang::En,
            api_key: String::new(),
            model: String::from("deepseek-v4-flash"),
            model_list: vec![
                "deepseek-v4-flash".into(),
                "deepseek-v4-pro".into(),
            ],
            model_index: 0,
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

    /// Fetch model list from DeepSeek API using the provided key.
    /// Returns true if successful (populates model_list).
    pub fn fetch_models(&mut self) -> bool {
        let key = self.api_key.trim();
        if key.is_empty() {
            return false;
        }
        let url = "https://api.deepseek.com/models";
        let resp = ureq::get(url)
            .header("Authorization", &format!("Bearer {}", key))
            .call();
        match resp {
            Ok(r) => {
                let body = r.into_body().read_to_string().unwrap_or_default();
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                        let ids: Vec<String> = data
                            .iter()
                            .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(String::from))
                            .collect();
                        if !ids.is_empty() {
                            if !ids.contains(&self.model) {
                                self.model = ids[0].clone();
                                self.model_index = 0;
                            } else {
                                self.model_index = ids.iter().position(|n| n == &self.model).unwrap_or(0);
                            }
                            self.model_list = ids;
                            self.models_loaded = true;
                             return true;
                        }
                    }
                }
                false
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
        PersistentConfig {
            api_key: Some(self.api_key.trim().to_string()),
            model: Some(self.model.trim().to_string()),
            base_url: Some("https://api.deepseek.com".into()),
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
    pub debug: DebugState,
    pub ask: Option<AskState>,
    pub balance: String,
    block: BlockType,
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

#[derive(Clone, Copy, PartialEq)]
enum BlockType {
    None,
    Thinking,
    Text,
    Tool,
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

        let model = config.as_ref().and_then(|c| c.model.clone()).unwrap_or_else(|| "deepseek-v4-flash".into());
        let context_limit = config.as_ref().and_then(|c| c.context_limit).unwrap_or(1_000_000);
        let max_tokens = config.as_ref().and_then(|c| c.max_tokens).unwrap_or(16384);
        let effort = config.as_ref().and_then(|c| c.effort.clone()).unwrap_or_else(|| "high".into());
        let base_url = config.as_ref().and_then(|c| c.base_url.clone()).unwrap_or_else(|| "https://api.deepseek.com".into());
        let active_profile = config.as_ref().and_then(|c| c.active_profile.clone()).unwrap_or_else(|| "default".into());
        let profiles = config.as_ref().and_then(|c| c.profiles.clone()).unwrap_or_default();

        let max_tool_rounds = config.as_ref().and_then(|c| c.max_tool_rounds).unwrap_or(10);
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

        // ── Agent Behavior ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_agent_behavior().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "effort", l.t_menu_reasoning_effort().into(),
            &effort, true));
        items.push(mk(MenuItemKind::Toggle, "max_tool_rounds", l.t_menu_max_tool_rounds().into(),
            &max_tool_rounds.to_string(), true));
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
        items.push(mk(MenuItemKind::Value, "base_url", l.t_menu_base_url().into(),
            &base_url, false));

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
        let item = match self.items.get_mut(self.selected) {
            Some(i) => i,
            None => return,
        };
        if !item.editable { return; }

        match item.key.as_str() {
            "effort" => {
                item.value = if item.value == "high" { "max".into() } else { "high".into() };
            }
            "model" => {
                item.value = match item.value.as_str() {
                    "deepseek-v4-flash" => "deepseek-v4-pro".into(),
                    _ => "deepseek-v4-flash".into(),
                };
            }
            "max_tokens" => {
                item.value = match item.value.as_str() {
                    "16384" => "32000".into(),
                    "32000" => "96000".into(),
                    "96000" => "128000".into(),
                    "128000" => "256000".into(),
                    "256000" => "384000".into(),
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
            "max_tool_rounds" => {
                item.value = match item.value.as_str() {
                    "5" => "10".into(),
                    "10" => "15".into(),
                    "15" => "20".into(),
                    _ => "5".into(),
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
                "effort" => { config.effort = Some(item.value.clone()); }
                "model" => { config.model = Some(item.value.clone()); }
                "context_limit" => {
                    if let Ok(v) = item.value.parse::<u32>() { config.context_limit = Some(v); }
                }
                "max_tokens" => {
                    if let Ok(v) = item.value.parse::<u32>() { config.max_tokens = Some(v); }
                }
                "max_tool_rounds" => {
                    if let Ok(v) = item.value.parse::<u32>() { config.max_tool_rounds = Some(v); }
                }
                "language" => {
                    config.lang = Some(item.value.clone());
                }
                "api_key" => {
                    // secret holds the real key; value is the masked display
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
        }

        config.profiles = Some(self.profiles.clone());

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
            block: BlockType::None,
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
        CHARS[(self.frame_count as usize / 4) % CHARS.len()]
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
        // Trim old messages to cap render cost at ~250 messages
        const MAX_MSGS: usize = 250;
        if self.messages.len() >= MAX_MSGS {
            let trim = self.messages.len() - MAX_MSGS + 50;
            self.messages.drain(0..trim);
            self.scroll_offset = self.scroll_offset.saturating_sub(trim);
        }
        const MAX_STORED: usize = 50_000;
        let content = if content.chars().count() > MAX_STORED {
            let truncated: String = content.chars().take(MAX_STORED).collect();
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

    fn switch_block(&mut self, new_block: BlockType) {
        if self.block != new_block {
            self.finalize_last_message();
            self.streaming_rendered_len = 0;
            self.block = new_block;
            self.streaming = false;
            self.push_msg(ChatRole::Divider, "");
        }
    }

    pub fn handle_frame(&mut self, frame: Agent2Ui) {
        match frame {
            Agent2Ui::TurnStart { turn_id: _, user_text } => {
                self.streaming = false;
                self.debug.streaming = false;
                self.block = BlockType::None;
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
                self.block = BlockType::None;
                self.busy = false;
                self.scroll_offset = 0;
                self.status = self.setup.lang.t_chat_ready().to_string();
            }
            Agent2Ui::RoundDelta { turn_id: _, round_num: _, kind, delta } => {
                self.debug.streaming = true;
                let new_role = match kind {
                    RoundDeltaKind::Thinking => ChatRole::Thinking,
                    RoundDeltaKind::Answering => ChatRole::Assistant,
                    RoundDeltaKind::ToolCalling => ChatRole::Assistant,
                };
                // When kind switches (thinking→answering), finalize old draft and start new
                if self.streaming && self.streaming_kind != new_role {
                    self.finalize_last_message();
                    self.streaming_rendered_len = 0;
                    self.md_renderer = None;
                    self.pending_tail_lines = 0;
                    self.streaming = false;
                }
                self.streaming_kind = new_role;
                if !self.streaming {
                    self.streaming = true;
                    self.draft_round_msg_idx = Some(self.messages.len());
                    self.push_streaming_msg(new_role, "");
                }
                self.append_last(&delta);
            }
            Agent2Ui::RoundComplete { turn_id: _, round_num: _, thinking, answer, tool_calls, is_final } => {
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
                if let Some(ref t) = thinking {
                    if !t.is_empty() {
                        self.push_msg(ChatRole::Thinking, t);
                    }
                }
                if let Some(ref a) = answer {
                    if !a.is_empty() {
                        self.push_msg(ChatRole::Assistant, a);
                    }
                }
                for tc in &tool_calls {
                    self.debug.tool_calls_total += 1;
                    let label = format_tool_label(&tc.name, &tc.args_display);
                    self.push_tool_msg(&tc.id, &label, ToolStatus::Pending);
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
                self.block = BlockType::None;
                self.busy = false;
                self.scroll_offset = 0;
                self.finalize_last_message();
            }
            Agent2Ui::Cancelled => {
                self.status = self.setup.lang.t_chat_cancelled().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.block = BlockType::None;
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

    fn push_messages_from_file(&mut self, file: &SessionFile) {
        let mut pending_tool_uses: Vec<(String, String)> = Vec::new();
        for msg in &file.messages {
            if msg.role == "system" { continue; }
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        let role = if msg.role == "user" { ChatRole::User } else { ChatRole::Assistant };
                        self.push_msg(role, text);
                    }
                    ContentBlock::Reasoning { reasoning, .. } => {
                        self.push_msg(ChatRole::Thinking, reasoning);
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let args: String = serde_json::to_string(&input).unwrap_or_default();
                        pending_tool_uses.push((name.clone(), args));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let lang = self.setup.lang;
                        if let Some((name, args)) = if pending_tool_uses.is_empty() { None } else { Some(pending_tool_uses.remove(0)) } {
                            let label = tool_status(lang, &name, Some(&args));
                            let styled_lines = build_tool_lines(lang, &name, content, Some(&args));
                            let is_exec = matches!(name.as_str(), "bash" | "run" | "exec");
                            let trunc_note = if is_exec {
                                String::new()
                            } else {
                                let char_count = content.chars().count();
                                if char_count > 200 {
                                    lang.t_tool_truncated(char_count - 200)
                                } else { String::new() }
                            };
                            let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
                                Span::styled(label.clone(), Style::new().fg(Color::Cyan).bold())
                            ])];
                            lines.extend(styled_lines);
                            if !trunc_note.is_empty() {
                                lines.push(Line::from(Span::styled(trunc_note, Style::new().fg(Color::Gray))));
                            }
                            self.messages.push(ChatMessage {
                                role: ChatRole::Tool,
                                content: label,
                                lines,
                                tool_status: ToolStatus::Success,
                                tool_id: String::new(),
                            });
                        } else {
                            let short: String = content.chars().take(200).collect();
                            self.push_msg(ChatRole::Tool, &short);
                        }
                    }
                }
            }
        }
        for (name, args) in pending_tool_uses {
            let label = tool_status(self.setup.lang, &name, Some(&args));
            self.push_msg(ChatRole::Tool, &label);
        }
    }
}

fn extract_tool_target(_name: &str, args: Option<&str>) -> Option<String> {
    args.and_then(|a| serde_json::from_str::<serde_json::Value>(a).ok())
        .and_then(|v| {
            v.get("path").or_else(|| v.get("file"))
                .and_then(|p| p.as_str()).map(String::from)
                .or_else(|| v.get("pattern").and_then(|p| p.as_str()).map(String::from))
                .or_else(|| v.get("command").and_then(|c| c.as_str()).map(String::from))
                .or_else(|| v.get("library").and_then(|l| l.as_str()).map(String::from))
                .or_else(|| v.get("title").and_then(|t| t.as_str()).map(String::from))
                .or_else(|| v.get("query").and_then(|q| q.as_str()).map(|q| if q.len() > 40 { format!("{}...", &q[..40]) } else { q.to_string() }))
                .or_else(|| v.get("from").and_then(|f| f.as_str())
                    .and_then(|f| v.get("to").and_then(|t| t.as_str()).map(|t| format!("{} → {}", f, t))))
        })
}

fn tool_status(lang: crate::i18n::Lang, name: &str, args: Option<&str>) -> String {
    let target = extract_tool_target(name, args);
    let label = match name {
        "explore" => lang.t_tool_exploring(),
        "read_file" => lang.t_tool_reading(),
        "write_file" | "edit_file" | "edit_file_diff" => lang.t_tool_writing(),
        "glob" | "search" | "web_fetch" | "web_search" => lang.t_tool_searching(),
        "bash" | "run" | "exec" => lang.t_tool_executing(),
        "delete_file" => lang.t_tool_deleting(),
        "move_file" => lang.t_tool_moving(),
        "copy_file" => lang.t_tool_copying(),
        "list_dir" => lang.t_tool_listing(),
        "diff" => lang.t_tool_diffing(),
        "commit" => lang.t_tool_committing(),
        "git_checkpoint" | "git_status" | "git_diff" | "git_log" | "git_commit" | "git_push" | "git_pull" | "git_fetch" | "git_branch" | "git_checkout" | "git_reset" | "git_stash" | "git_restore" | "git_remote" | "git_init" | "git_merge" => lang.t_tool_git(),
        "task_create" | "plan_create" => lang.t_tool_creating(),
        "task_update" | "plan_update" => lang.t_tool_updating(),
        "context7_resolve" => lang.t_tool_resolving(),
        "context7_query" => lang.t_tool_querying(),
        "ask_user" => lang.t_tool_asking(),
        _ => name,
    };
    match target {
        Some(t) => format!("{}: {}", label, t),
        None => label.to_string(),
    }
}

fn build_tool_lines(lang: crate::i18n::Lang, name: &str, content: &str, args: Option<&str>) -> Vec<Line<'static>> {
    let json = args.and_then(|a| serde_json::from_str::<serde_json::Value>(a).ok()).unwrap_or_default();
    match name {
        "read_file" => {
            let max_lines = 40usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            for line in content.lines().take(max_lines) {
                out.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::new().fg(Color::Rgb(180, 190, 200)),
                )));
            }
            let end = json.get("end_line").and_then(|v| v.as_u64());
            let start = json.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            if let Some(e) = end {
                if (e as usize).saturating_sub(start) >= max_lines {
                    out.push(Line::from(Span::styled(
                        lang.t_tool_lines_total(total_lines, max_lines),
                        Style::new().fg(Color::Gray),
                    )));
                }
            }
            out
        }
        "edit_file" | "edit_file_diff" => {
            let old_str = json.get("old_string").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let new_str = json.get("new_string").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let old_arr = json.get("old_lines").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|l| l.as_str().map(String::from)).collect::<Vec<_>>());
            let new_arr = json.get("new_lines").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|l| l.as_str().map(String::from)).collect::<Vec<_>>());

            let mut out = Vec::new();
            out.push(Line::from(""));

            if let (Some(ol), Some(nl)) = (old_arr, new_arr) {
                for line in &ol {
                    out.push(Line::from(vec![
                        Span::styled(" - ".to_string(), Style::new().fg(Color::Rgb(220, 80, 80)).bold()),
                        Span::styled(line.clone(), Style::new().fg(Color::Rgb(200, 150, 150))),
                    ]));
                }
                for line in &nl {
                    out.push(Line::from(vec![
                        Span::styled(" + ".to_string(), Style::new().fg(Color::Rgb(80, 200, 80)).bold()),
                        Span::styled(line.clone(), Style::new().fg(Color::Rgb(150, 200, 150))),
                    ]));
                }
            } else if !old_str.is_empty() || !new_str.is_empty() {
                for line in old_str.lines() {
                    out.push(Line::from(vec![
                        Span::styled(" - ".to_string(), Style::new().fg(Color::Rgb(220, 80, 80)).bold()),
                        Span::styled(line.to_string(), Style::new().fg(Color::Rgb(200, 150, 150))),
                    ]));
                }
                for line in new_str.lines() {
                    out.push(Line::from(vec![
                        Span::styled(" + ".to_string(), Style::new().fg(Color::Rgb(80, 200, 80)).bold()),
                        Span::styled(line.to_string(), Style::new().fg(Color::Rgb(150, 200, 150))),
                    ]));
                }
            }
            out
        }
        "bash" | "run" | "exec" => {
            let max_lines = 60usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            out.push(Line::from(""));
            let cmd = json.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if !cmd.is_empty() {
                out.push(Line::from(vec![
                    Span::styled(" $ ", Style::new().fg(Color::Rgb(80, 200, 80)).bold()),
                    Span::styled(cmd.to_string(), Style::new().fg(Color::Rgb(180, 200, 180))),
                ]));
                out.push(Line::from(""));
            }
            for line in content.lines().take(max_lines) {
                out.push(Line::from(vec![
                    Span::styled(" │ ", Style::new().fg(Color::Rgb(60, 70, 80))),
                    Span::styled(line.to_string(), Style::new().fg(Color::Rgb(200, 210, 220))),
                ]));
            }
            if total_lines > max_lines {
                out.push(Line::from(Span::styled(
                    lang.t_tool_lines_total(total_lines, max_lines),
                    Style::new().fg(Color::Gray),
                )));
            }
            out
        }
        "explore" => {
            let max_lines = 30usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines().take(max_lines) {
                let style = if line.starts_with("[PROJECT_MAP]") || line.starts_with("path:") {
                    Style::new().fg(Color::Rgb(120, 200, 255)).bold()
                } else if line.starts_with("project markers:") {
                    Style::new().fg(Color::Rgb(180, 180, 100))
                } else if line.starts_with("[DIR]") {
                    Style::new().fg(Color::Rgb(200, 180, 100))
                } else if !line.is_empty() && !line.starts_with(" ") {
                    Style::new().fg(Color::Rgb(180, 200, 180)).bold()
                } else {
                    Style::new().fg(Color::Rgb(140, 150, 160))
                };
                out.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
            if total_lines > max_lines {
                out.push(Line::from(Span::styled(
                    lang.t_tool_lines_total(total_lines, max_lines),
                    Style::new().fg(Color::Gray),
                )));
            }
            out
        }
        "glob" | "search" | "grep" => {
            let max_lines = 20usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines().take(max_lines) {
                out.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::new().fg(Color::Rgb(180, 200, 180)),
                )));
            }
            if total_lines > max_lines {
                out.push(Line::from(Span::styled(
                    format!("  ... {} matches total", total_lines),
                    Style::new().fg(Color::Gray),
                )));
            }
            out
        }
        "list_dir" => {
            let max_lines = 30usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines().take(max_lines) {
                let style = if line.ends_with('/') || line.contains("(dir)") {
                    Style::new().fg(Color::Rgb(120, 180, 255)).bold()
                } else {
                    Style::new().fg(Color::Rgb(180, 190, 200))
                };
                out.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
            if total_lines > max_lines {
                out.push(Line::from(Span::styled(
                    lang.t_tool_lines_total(total_lines, max_lines),
                    Style::new().fg(Color::Gray),
                )));
            }
            out
        }
        "diff" => {
            let max_lines = 40usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines().take(max_lines) {
                let style = if line.starts_with('+') {
                    Style::new().fg(Color::Rgb(100, 200, 120))
                } else if line.starts_with('-') {
                    Style::new().fg(Color::Rgb(220, 100, 100))
                } else if line.starts_with('@') {
                    Style::new().fg(Color::Rgb(120, 180, 255))
                } else {
                    Style::new().fg(Color::Rgb(160, 170, 180))
                };
                out.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
            if total_lines > max_lines {
                out.push(Line::from(Span::styled(
                    lang.t_tool_lines_total(total_lines, max_lines),
                    Style::new().fg(Color::Gray),
                )));
            }
            out
        }
        "commit" => {
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines() {
                let style = if line.starts_with("[OK]") {
                    Style::new().fg(Color::Rgb(100, 220, 100)).bold()
                } else if line.starts_with("[HINT]") {
                    Style::new().fg(Color::Rgb(180, 180, 100))
                } else if line.starts_with("[ERROR]") {
                    Style::new().fg(Color::Red).bold()
                } else {
                    Style::new().fg(Color::Rgb(180, 190, 200))
                };
                out.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
            out
        }
        "web_fetch" | "web_search" | "context7_resolve" | "context7_query" => {
            let max_lines = 20usize;
            let total_lines = content.lines().count();
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines().take(max_lines) {
                out.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::new().fg(Color::Rgb(180, 190, 200)),
                )));
            }
            if total_lines > max_lines {
                out.push(Line::from(Span::styled(
                    lang.t_tool_lines_total(total_lines, max_lines),
                    Style::new().fg(Color::Gray),
                )));
            }
            out
        }
        "task_create" | "task_update" | "plan_create" | "plan_update" | "ask_user" => {
            let mut out = Vec::new();
            out.push(Line::from(""));
            for line in content.lines() {
                let style = if line.starts_with("[OK]") || line.starts_with("[CREATED]") {
                    Style::new().fg(Color::Rgb(100, 220, 100)).bold()
                } else if line.starts_with("[ERROR]") {
                    Style::new().fg(Color::Red).bold()
                } else {
                    Style::new().fg(Color::Rgb(180, 190, 200))
                };
                out.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
            out
        }
        "write_file" => {
            let path = json.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let mut out = vec![Line::from("")];
            if !path.is_empty() {
                out.push(Line::from(Span::styled(format!("  {}", path), Style::new().fg(Color::Rgb(120, 200, 255)))));
            }
            out
        }
        "delete_file" | "move_file" | "copy_file" => {
            let mut out = vec![Line::from("")];
            for line in content.lines().take(5) {
                out.push(Line::from(Span::styled(format!("  {}", line), Style::new().fg(Color::Rgb(180, 190, 200)))));
            }
            out
        }
        "task_list" | "plan_read" | "plan_list" => {
            let mut out = vec![Line::from("")];
            for line in content.lines().take(30) {
                out.push(Line::from(Span::styled(format!("  {}", line), Style::new().fg(Color::Rgb(200, 200, 120)))));
            }
            out
        }
        "git_checkpoint" | "git_status" | "git_diff" | "git_log" | "git_commit" | "git_push" | "git_pull" | "git_fetch" | "git_branch" | "git_checkout" | "git_reset" | "git_stash" | "git_restore" | "git_remote" | "git_init" | "git_merge" => {
            let mut out = vec![Line::from("")];
            for line in content.lines().take(30) {
                let style = if line.starts_with("[OK]") { Style::new().fg(Color::Rgb(100, 220, 100)) }
                    else if line.starts_with("[ERROR]") { Style::new().fg(Color::Red) }
                    else if line.starts_with("M ") || line.starts_with("A ") || line.starts_with("D ") || line.starts_with("R ") { Style::new().fg(Color::Rgb(200, 200, 100)) }
                    else if line.starts_with("?") { Style::new().fg(Color::Rgb(200, 80, 80)) }
                    else { Style::new().fg(Color::Rgb(180, 190, 200)) };
                out.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
            out
        }
        _ => {
            if !content.is_empty() {
                let short: Vec<Line> = content.lines().take(10).map(|l|
                    Line::from(Span::styled(l.to_string(), Style::new().fg(Color::Rgb(180, 190, 200))))
                ).collect();
                let mut out = vec![Line::from("")];
                out.extend(short);
                if content.lines().count() > 10 {
                    out.push(Line::from(Span::styled(
                        lang.t_tool_lines_total(content.lines().count(), 10),
                        Style::new().fg(Color::Gray),
                    )));
                }
                out
            } else {
                vec![]
            }
        }
    }
}
