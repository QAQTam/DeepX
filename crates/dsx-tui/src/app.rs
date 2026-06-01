use dsx_proto::Agent2Ui;
use dsx_types::{ConfigStore, PersistentConfig, SessionMeta, SessionFile, ContentBlock};
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Style};

// ── Active screen ──

#[derive(PartialEq)]
pub enum Screen {
    Setup,
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
    pub scroll_offset: usize,
    pub frame_count: u64,
    pub sessions: Vec<SessionMeta>,
    pub session_index: usize,
    pub resume_seed: Option<String>,
    pub show_debug: bool,
    pub debug: DebugState,
    pub ask: Option<AskState>,
    pub balance: String,
    block: BlockType,
    pub validating: bool,
    streaming_rendered_len: usize,
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
    pub current_phase: String,
    pub streaming: bool,
    pub dsml_compat_count: u32,
    pub explored: bool,
    pub declared_files: Vec<String>,
    pub read_files: Vec<String>,
    pub written_this_turn: Vec<String>,
    pub tool_progress: String,
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
}

#[derive(Clone)]
pub struct MenuItem {
    pub kind: MenuItemKind,
    pub label: String,
    pub value: String,
    pub editable: bool,
    pub key: String,
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
        let api_key_masked = if api_key.len() > 3 {
            format!("sk-{}", "●".repeat(api_key.len().saturating_sub(3).min(20)))
        } else if api_key.is_empty() {
            if l.as_str() == "zh" { "(未设置)" } else { "(not set)" }.into()
        } else {
            api_key.clone()
        };

        let model = config.as_ref().and_then(|c| c.model.clone()).unwrap_or_else(|| "deepseek-v4-flash".into());
        let context_limit = config.as_ref().and_then(|c| c.context_limit).unwrap_or(1_000_000);
        let max_tokens = config.as_ref().and_then(|c| c.max_tokens).unwrap_or(16000);
        let effort = config.as_ref().and_then(|c| c.effort.clone()).unwrap_or_else(|| "high".into());
        let base_url = config.as_ref().and_then(|c| c.base_url.clone()).unwrap_or_else(|| "https://api.deepseek.com".into());
        let active_profile = config.as_ref().and_then(|c| c.active_profile.clone()).unwrap_or_else(|| "default".into());
        let profiles = config.as_ref().and_then(|c| c.profiles.clone()).unwrap_or_default();

        let max_tool_rounds = config.as_ref().and_then(|c| c.max_tool_rounds).unwrap_or(10);
        let c7_key = config.as_ref().and_then(|c| c.context7_api_key.clone()).unwrap_or_default();
        let mut items: Vec<MenuItem> = Vec::new();
        let mk = |kind, key: &str, label: String, value: &str, editable: bool| MenuItem {
            kind, key: key.into(), label, value: value.into(), editable,
        };

        // ── Agent Behavior ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_agent_behavior().into(), "", false));
        items.push(mk(MenuItemKind::Value, "effort", l.t_menu_reasoning_effort().into(),
            &effort, true));
        items.push(mk(MenuItemKind::Toggle, "max_tool_rounds", l.t_menu_max_tool_rounds().into(),
            &max_tool_rounds.to_string(), true));
        items.push(mk(MenuItemKind::Value, "context7_api_key", l.t_menu_c7_key().into(),
            if c7_key.is_empty() { "(not set)" } else { "****" }, true));

        // ── Model ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_model_section().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "model", l.t_menu_model().into(), &model, false));
        items.push(mk(MenuItemKind::Toggle, "max_tokens", l.t_menu_max_tokens().into(),
            &max_tokens.to_string(), false));
        items.push(mk(MenuItemKind::Toggle, "context_limit", l.t_menu_context_limit().into(),
            &context_limit.to_string(), false));

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
        items.push(mk(MenuItemKind::Value, "api_key", l.t_menu_api_key().into(),
            &api_key_masked, true));
        items.push(mk(MenuItemKind::Value, "base_url", l.t_menu_base_url().into(),
            &base_url, true));

        // ── Interface ──
        items.push(mk(MenuItemKind::Section, "", l.t_menu_interface().into(), "", false));
        items.push(mk(MenuItemKind::Toggle, "language", l.t_menu_language().into(),
            l.as_str(), true));

        Self {
            selected: 1,
            editing: false,
            edit_buf: String::new(),
            status: String::new(),
            items,
            profiles,
            lang: l,
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
                    "4096" => "8192".into(),
                    "8192" => "16384".into(),
                    "16384" => "32000".into(),
                    "32000" => "96000".into(),
                    _ => "4096".into(),
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
                    let v = item.value.trim().to_string();
                    if !v.is_empty() && !v.starts_with("sk-") { return; }
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
}

impl App {
    pub fn new(need_setup: bool) -> Self {
        Self {
            screen: if need_setup { Screen::Setup } else { Screen::Session },
            setup: SetupState::new(),
            messages: Vec::new(),
            input: String::new(),
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
            scroll_offset: 0,
            streaming_rendered_len: 0,
            frame_count: 0,
            sessions: Vec::new(),
            session_index: 0,
            resume_seed: None,
            show_debug: false,
            debug: DebugState {
                hp_connected: false,
                session_seed: String::new(),
                context_tokens: 0,
                tool_calls_total: 0,
                tool_failures: 0,
                current_phase: String::from("plan"),
                streaming: false,
                dsml_compat_count: 0,
                explored: false,
                declared_files: Vec::new(),
                read_files: Vec::new(),
                written_this_turn: Vec::new(),
                tool_progress: String::new(),
            },
            ask: None,
            balance: String::new(),
            block: BlockType::None,
            validating: false,
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

    pub fn push_msg(&mut self, role: ChatRole, content: &str) {
        const MAX_STORED: usize = 50_000;
        let content = if content.chars().count() > MAX_STORED {
            let truncated: String = content.chars().take(MAX_STORED).collect();
            format!("{}...[TRUNCATED]", truncated)
        } else {
            content.to_string()
        };
        let lines = crate::markdown::render_content(&content);
        self.streaming_rendered_len = content.len();
        self.messages.push(ChatMessage { role, content, lines });
    }

    fn append_last(&mut self, content: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(content);
            // Re-render when content has grown by 30+ chars (smooth updates, minimal overhead)
            if last.content.len() >= self.streaming_rendered_len + 30 {
                last.lines = crate::markdown::render_content(&last.content);
                self.streaming_rendered_len = last.content.len();
            }
        }
    }

    fn switch_block(&mut self, new_block: BlockType) {
        if self.block != new_block {
            // Finalize streaming message before switching
            if let Some(last) = self.messages.last_mut() {
                if !last.content.is_empty() {
                    last.lines = crate::markdown::render_content(&last.content);
                }
            }
            self.streaming_rendered_len = 0;
            self.block = new_block;
            self.streaming = false;
            self.push_msg(ChatRole::Divider, "");
        }
    }

    pub fn handle_frame(&mut self, frame: Agent2Ui) {
        match frame {
            Agent2Ui::ContentDelta { delta, reasoning } => {
                self.debug.streaming = true;
                if let Some(r) = reasoning {
                    if !r.is_empty() {
                        self.switch_block(BlockType::Thinking);
                        if self.block == BlockType::Thinking && self.streaming {
                            self.append_last(&r);
                        } else {
                            self.push_msg(ChatRole::Thinking, &r);
                            self.streaming = true;
                        }
                    }
                }
                if !delta.is_empty() {
                    self.switch_block(BlockType::Text);
                    if self.block == BlockType::Text && self.streaming {
                        self.append_last(&delta);
                    } else {
                        self.push_msg(ChatRole::Assistant, &delta);
                        self.streaming = true;
                    }
                }
            }
            Agent2Ui::ToolResult { name, content, args, success, .. } => {
                self.debug.tool_calls_total += 1;
                if !success { self.debug.tool_failures += 1; }
                self.switch_block(BlockType::Tool);
                let lang = self.setup.lang;
                let label = tool_status(lang, &name, args.as_deref());
                let styled_lines = build_tool_lines(lang, &name, &content, args.as_deref());
                // Skip char truncation for exec tools (handled by line limit in build_tool_lines)
                let trunc_note = if matches!(name.as_str(), "bash" | "run" | "exec") {
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
                });
            }
            Agent2Ui::ApiResponse { content, reasoning_content, usage, context_limit, session_tokens, context_tokens, .. } => {
                if !self.streaming {
                    if let Some(ref rc) = reasoning_content {
                        if !rc.is_empty() {
                            self.push_msg(ChatRole::Thinking, rc);
                        }
                    }
                    if !content.is_empty() {
                        self.push_msg(ChatRole::Assistant, &content);
                    }
                }
                self.context_tokens = context_tokens;
                self.session_tokens = session_tokens;
                self.context_limit = context_limit;
                if let Some(u) = usage {
                    self.cache_hit += u.prompt_cache_hit_tokens;
                    self.cache_miss += u.prompt_cache_miss_tokens;
                    self.update_cache(u.prompt_cache_hit_tokens, u.prompt_cache_miss_tokens);
                }
                self.status = self.setup.lang.t_chat_ready().to_string();
                self.streaming = false;
            }
            Agent2Ui::Error { message } => {
                let status_text = format!("{}: {}", self.setup.lang.t_chat_error(), message);
                self.push_msg(ChatRole::Status, &status_text);
                self.status = status_text;
            }
            Agent2Ui::Done => {
                self.status = self.setup.lang.t_chat_ready().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.block = BlockType::None;
                if let Some(last) = self.messages.last_mut() {
                    last.lines = crate::markdown::render_content(&last.content);
                }
            }
            Agent2Ui::Cancelled => {
                self.status = self.setup.lang.t_chat_cancelled().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.block = BlockType::None;
                if let Some(last) = self.messages.last_mut() {
                    last.lines = crate::markdown::render_content(&last.content);
                }
            }
            Agent2Ui::SessionRestored { seed, message_count, tokens_used, .. } => {
                self.status = self.setup.lang.t_session_restored(&seed, message_count, tokens_used);
                self.debug.session_seed = seed.clone();
                self.debug.context_tokens = tokens_used;
                self.session_tokens = tokens_used as u64;
                self.scroll_offset = 0;
                self.load_messages_from_session(&seed);
            }
            Agent2Ui::DebugSnapshot { hp_connected, session_seed, context_tokens,
                tool_calls_total, tool_failures, current_phase, streaming, dsml_compat_count, .. } => {
                self.debug.hp_connected = hp_connected;
                self.debug.session_seed = session_seed;
                self.debug.context_tokens = context_tokens;
                self.debug.tool_calls_total = tool_calls_total;
                self.debug.tool_failures = tool_failures;
                self.debug.current_phase = current_phase;
                self.debug.streaming = streaming;
                self.debug.dsml_compat_count = dsml_compat_count;
            }
            Agent2Ui::AskUser { question, options, .. } => {
                self.ask = Some(AskState {
                    question,
                    options: options.unwrap_or_default(),
                    selected: 0,
                    custom_input: String::new(),
                });
            }
            Agent2Ui::Balance { is_available, total_balance, currency } => {
                let status = if is_available { "✓" } else { "✗" };
                self.balance = format!("{} {}{} {}", status, if currency == "CNY" { "¥" } else { "$" }, total_balance, currency);
            }
            Agent2Ui::ToolProgress { content, .. } => {
                self.debug.tool_progress = content;
            }
            Agent2Ui::ToolState { explored, declared_files, read_files, written_this_turn, .. } => {
                self.debug.explored = explored;
                self.debug.declared_files = declared_files;
                self.debug.read_files = read_files;
                self.debug.written_this_turn = written_this_turn;
            }
            Agent2Ui::CachePrediction { hit_rate } => {
                self.cache_rates.push(hit_rate);
                if hit_rate < 0.3 {
                    self.cache_warning = format!("cache miss: {:.1}%", (1.0 - hit_rate) * 100.0);
                }
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

    fn load_messages_from_session(&mut self, seed: &str) {
        use std::fs;
        let dir = dsx_types::platform::data_dir().join("sessions");
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with(seed) { continue; }
            let session_path = path.join("session.json");
            if let Ok(data) = fs::read_to_string(&session_path) {
                if let Ok(file) = serde_json::from_str::<SessionFile>(&data) {
                    self.push_messages_from_file(&file);
                    return;
                }
            }
        }
        let flat = dir.join(format!("{}.json", seed));
        if let Ok(data) = fs::read_to_string(&flat) {
            if let Ok(file) = serde_json::from_str::<SessionFile>(&data) {
                self.push_messages_from_file(&file);
            }
        }
    }

    fn push_messages_from_file(&mut self, file: &SessionFile) {
        let mut pending_tool_use: Option<(String, String)> = None;
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
                        pending_tool_use = Some((name.clone(), args));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let lang = self.setup.lang;
                        if let Some((name, args)) = pending_tool_use.take() {
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
                            });
                        } else {
                            let short: String = content.chars().take(200).collect();
                            self.push_msg(ChatRole::Tool, &short);
                        }
                    }
                }
            }
        }
        if let Some((name, args)) = pending_tool_use.take() {
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
            let start = json.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let end = json.get("end_line").and_then(|v| v.as_u64());
            let max_lines = 40usize;
            let total_lines = content.lines().count();

            let mut out = Vec::new();
            for (i, line) in content.lines().take(max_lines).enumerate() {
                let ln = start + i;
                out.push(Line::from(vec![
                    Span::styled(format!(" {:>4} │ ", ln), Style::new().fg(Color::Rgb(80, 90, 100))),
                    Span::styled(line.to_string(), Style::new().fg(Color::Rgb(180, 190, 200))),
                ]));
            }
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
