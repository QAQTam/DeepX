use dsx_proto::Agent2Ui;
use dsx_types::{ConfigStore, PersistentConfig, SessionMeta};

// ── Active screen ──

#[derive(PartialEq)]
pub enum Screen {
    Setup,
    Session,
    Chat,
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
                    self.error = "Model name is required".into();
                }
            }
            3 => {
                if let Ok(v) = self.context_limit.parse::<u32>() {
                    if v < 1024 {
                        self.error = "Context limit must be at least 1024".into();
                    }
                } else {
                    self.error = "Invalid number".into();
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
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            context_limit: Some(self.context_limit.parse().unwrap_or(1_000_000)),
            auto_mode: Some(true),
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
    pub phase: String,
    pub tokens: u32,
    pub should_quit: bool,
    pub streaming: bool,
    pub scroll_offset: usize,
    pub frame_count: u64,
    pub sessions: Vec<SessionMeta>,
    pub session_index: usize,
    pub resume_seed: Option<String>,
    pub show_debug: bool,
    pub debug: DebugState,
    block: BlockType,
    pub validating: bool,
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
pub struct DebugState {
    pub hp_connected: bool,
    pub session_seed: String,
    pub context_tokens: u32,
    pub tool_calls_total: u32,
    pub tool_failures: u32,
    pub current_phase: String,
    pub streaming: bool,
}

impl App {
    pub fn new(need_setup: bool) -> Self {
        Self {
            screen: if need_setup { Screen::Setup } else { Screen::Session },
            setup: SetupState::new(),
            messages: Vec::new(),
            input: String::new(),
            status: String::from("Ready"),
            phase: String::from("Coding"),
            tokens: 0,
            should_quit: false,
            streaming: false,
            scroll_offset: 0,
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
                current_phase: String::from("Coding"),
                streaming: false,
            },
            block: BlockType::None,
            validating: false,
        }
    }

    pub fn tick(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    pub fn spinner(&self) -> &str {
        const CHARS: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];
        CHARS[(self.frame_count as usize / 4) % CHARS.len()]
    }

    fn push_msg(&mut self, role: ChatRole, content: &str) {
        let lines = crate::markdown::render_content(content);
        self.messages.push(ChatMessage { role, content: content.to_string(), lines });
    }

    fn append_last(&mut self, content: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(content);
            last.lines = crate::markdown::render_content(&last.content);
        }
    }

    fn switch_block(&mut self, new_block: BlockType) {
        if self.block != new_block {
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
                        self.scroll_offset = 0;
                        self.switch_block(BlockType::Thinking);
                        if self.block == BlockType::Thinking && self.streaming {
                            self.append_last(&r);
                        } else {
                            self.push_msg(ChatRole::Thinking, &r);
                            self.streaming = true;
                        }
                        return;
                    }
                }
                if !delta.is_empty() {
                    self.scroll_offset = 0;
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
                let label = tool_label(&name, &content, args.as_deref());
                let char_count = content.chars().count();
                let preview = if char_count > 200 {
                    let head: String = content.chars().take(200).collect();
                    format!("{}\n  ... (+{} chars)", head, char_count - 200)
                } else {
                    content.clone()
                };
                self.push_msg(ChatRole::Tool, &format!("{label}\n{preview}"));
            }
            Agent2Ui::ApiResponse { content, reasoning_content, usage, context_tokens, .. } => {
                // Only show from ApiResponse if content wasn't already streamed
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
                self.tokens = context_tokens;
                if let Some(u) = usage {
                    self.tokens = u.total_tokens;
                }
                self.status = "Ready".into();
                self.streaming = false;
            }
            Agent2Ui::Error { message } => {
                self.push_msg(ChatRole::Status, &format!("Error: {}", message));
                self.status = format!("Error: {}", message);
            }
            Agent2Ui::PhaseChanged { phase } => {
                self.phase = phase.clone();
                self.debug.current_phase = phase;
            }
            Agent2Ui::Done => {
                self.status = "Ready".into();
                self.streaming = false;
                self.debug.streaming = false;
                self.block = BlockType::None;
            }
            Agent2Ui::Cancelled => {
                self.status = "Cancelled".into();
                self.streaming = false;
                self.debug.streaming = false;
                self.block = BlockType::None;
            }
            Agent2Ui::SessionRestored { seed, message_count, tokens_used, .. } => {
                self.status = format!("Session {} restored ({} msgs, {} tokens)",
                    &seed[..8.min(seed.len())], message_count, tokens_used);
                self.debug.session_seed = seed;
                self.debug.context_tokens = tokens_used;
            }
            Agent2Ui::DebugSnapshot { hp_connected, session_seed, context_tokens,
                tool_calls_total, tool_failures, current_phase, streaming } => {
                self.debug = DebugState {
                    hp_connected,
                    session_seed,
                    context_tokens,
                    tool_calls_total,
                    tool_failures,
                    current_phase,
                    streaming,
                };
            }
            _ => {}
        }
    }
}

fn tool_label(name: &str, content: &str, args: Option<&str>) -> String {
    let path = args.and_then(|a| {
        serde_json::from_str::<serde_json::Value>(a).ok()
            .and_then(|v| v.get("path").or_else(|| v.get("file"))
                .and_then(|p| p.as_str())
                .map(String::from))
    });

    let first_line = content.lines().next().unwrap_or("");

    match name {
        "explore" => {
            if let Some(ref p) = path {
                format!("explore: {}", p)
            } else if let Some(p) = first_line.strip_prefix("[PROJECT_MAP]path: ")
                .or_else(|| first_line.strip_prefix("[DIR] "))
            {
                let short = p.split(" markers:").next().unwrap_or(p);
                format!("explore: {}", short.trim())
            } else {
                "explore".to_string()
            }
        }
        "read_file" => {
            if let Some(ref p) = path {
                format!("read_file: {}", p)
            } else {
                format!("read_file: {} chars", content.chars().count())
            }
        }
        "write_file" | "edit_file" => {
            if let Some(ref p) = path {
                format!("{}: {}", name, p)
            } else {
                format!("{}: {} chars written", name, content.chars().count())
            }
        }
        "glob" | "grep" => {
            if let Some(ref p) = path.or_else(|| {
                args.and_then(|a| serde_json::from_str::<serde_json::Value>(a).ok()
                    .and_then(|v| v.get("pattern").and_then(|p| p.as_str()).map(String::from)))
            }) {
                format!("{}: {}", name, p)
            } else {
                let short: String = first_line.chars().take(60).collect();
                format!("{}: {}", name, short)
            }
        }
        "bash" | "run" => {
            if let Some(ref command) = path {
                format!("{}: {}", name, command)
            } else {
                let short: String = first_line.chars().take(80).collect();
                format!("{}: {}", name, short)
            }
        }
        _ => {
            if let Some(ref p) = path {
                format!("{}: {}", name, p)
            } else {
                format!("{}: {} chars", name, content.chars().count())
            }
        }
    }
}
