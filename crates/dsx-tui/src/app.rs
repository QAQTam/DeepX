use dsx_proto::Agent2Ui;
use dsx_types::{ConfigStore, PersistentConfig};

// ── Active screen ──

#[derive(PartialEq)]
pub enum Screen {
    Setup,
    Chat,
}

// ── Setup wizard state ──

pub struct SetupState {
    pub step: usize,
    pub lang: crate::i18n::Lang,
    pub api_key: String,
    pub model: String,
    pub model_list: Vec<String>,
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
    pub scroll: usize,
    pub streaming: bool,
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

impl App {
    pub fn new(need_setup: bool) -> Self {
        Self {
            screen: if need_setup { Screen::Setup } else { Screen::Chat },
            setup: SetupState::new(),
            messages: Vec::new(),
            input: String::new(),
            status: String::from("Ready"),
            phase: String::from("Coding"),
            tokens: 0,
            should_quit: false,
            scroll: 0,
            streaming: false,
            block: BlockType::None,
            validating: false,
        }
    }

    fn push_msg(&mut self, role: ChatRole, content: &str) {
        self.messages.push(ChatMessage { role, content: content.to_string() });
    }

    fn append_last(&mut self, content: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(content);
        }
    }

    fn switch_block(&mut self, new_block: BlockType) {
        if self.block != new_block {
            self.block = new_block;
            self.push_msg(ChatRole::Divider, "");
        }
    }

    pub fn handle_frame(&mut self, frame: Agent2Ui) {
        match frame {
            Agent2Ui::ContentDelta { delta, reasoning } => {
                if let Some(r) = reasoning {
                    if !r.is_empty() {
                        self.switch_block(BlockType::Thinking);
                        self.push_msg(ChatRole::Thinking, &r);
                        return;
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
            Agent2Ui::ToolResult { name, content, .. } => {
                self.switch_block(BlockType::Tool);
                self.push_msg(ChatRole::Tool, &format!("{}: {}", name, content));
            }
            Agent2Ui::ApiResponse { content, reasoning_content, usage, context_tokens, .. } => {
                if let Some(ref rc) = reasoning_content {
                    if !rc.is_empty() {
                        self.switch_block(BlockType::Thinking);
                        self.push_msg(ChatRole::Thinking, rc);
                    }
                }
                if !content.is_empty() && self.block != BlockType::Text {
                    self.switch_block(BlockType::Text);
                    self.push_msg(ChatRole::Assistant, &content);
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
                self.phase = phase;
            }
            Agent2Ui::Done => {
                self.status = "Ready".into();
                self.streaming = false;
                self.block = BlockType::None;
                self.scroll = self.messages.len().saturating_sub(1);
            }
            Agent2Ui::Cancelled => {
                self.status = "Cancelled".into();
                self.streaming = false;
                self.block = BlockType::None;
            }
            Agent2Ui::SessionRestored { seed, message_count, tokens_used, .. } => {
                self.status = format!("Session {} restored ({} msgs, {} tokens)",
                    &seed[..8.min(seed.len())], message_count, tokens_used);
            }
            _ => {}
        }
    }
}
