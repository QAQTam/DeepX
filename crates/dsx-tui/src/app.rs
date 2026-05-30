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

impl App {
    fn load_messages_from_session(&mut self, seed: &str) {
        use std::fs;
        let dir = dsx_types::platform::data_dir().join("sessions");
        // Try directory format first
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
        // Fallback: flat format
        let flat = dir.join(format!("{}.json", seed));
        if let Ok(data) = fs::read_to_string(&flat) {
            if let Ok(file) = serde_json::from_str::<SessionFile>(&data) {
                self.push_messages_from_file(&file);
            }
        }
    }

    fn push_messages_from_file(&mut self, file: &SessionFile) {
        for msg in &file.messages {
            if msg.role == "system" { continue; }
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        let role = if msg.role == "user" { ChatRole::User } else { ChatRole::Assistant };
                        self.push_msg(role, text);
                    }
                    ContentBlock::Thinking { thinking, .. } => {
                        self.push_msg(ChatRole::Thinking, thinking);
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let args: String = input.as_str().unwrap_or("").into();
                        let short_args: String = args.chars().take(60).collect();
                        self.push_msg(ChatRole::Tool, &format!("{}: {}", name, short_args));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let short: String = content.chars().take(200).collect();
                        self.push_msg(ChatRole::Tool, &format!("result: {}", short));
                    }
                }
            }
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
    pub id: String,
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
            session_tokens: 0,
            cache_hit: 0,
            cache_miss: 0,
            cache_rates: Vec::new(),
            cache_warning: String::new(),
            context_limit: 1_000_000,
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
            // All recent rounds below 30% → warn
            let all_low = self.cache_rates.iter().all(|&r| r < 0.30);
            self.cache_warning = if all_low {
                "⚠ 缓存命中持续过低，建议暂停并排查".into()
            } else if avg < 0.50 {
                "缓存命中偏低".into()
            } else {
                String::new()
            };
        }
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
                let styled_lines = build_tool_lines(&name, &content, args.as_deref());
                let char_count = content.chars().count();
                let trunc_note = if char_count > 200 {
                    format!("  ... (+{} chars)", char_count - 200)
                } else {
                    String::new()
                };
                let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
                    Span::styled(label.clone(), Style::new().fg(Color::Cyan).bold())
                ])];
                lines.extend(styled_lines);
                if !trunc_note.is_empty() {
                    lines.push(Line::from(Span::styled(trunc_note.clone(), Style::new().fg(Color::Gray))));
                }
                self.messages.push(ChatMessage {
                    role: ChatRole::Tool,
                    content: label,
                    lines,
                });
            }
            Agent2Ui::ApiResponse { content, reasoning_content, usage, context_limit, session_tokens, .. } => {
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
                self.session_tokens = session_tokens;
                if let Some(u) = usage {
                    self.tokens = u.total_tokens;
                    self.cache_hit += u.prompt_cache_hit_tokens;
                    self.cache_miss += u.prompt_cache_miss_tokens;
                    self.update_cache(u.prompt_cache_hit_tokens, u.prompt_cache_miss_tokens);
                }
                self.context_limit = context_limit;
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
                self.debug.session_seed = seed.clone();
                self.debug.context_tokens = tokens_used;
                self.session_tokens = tokens_used as u64;
                self.scroll_offset = 0;
                self.load_messages_from_session(&seed);
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
            Agent2Ui::AskUser { id, question, options } => {
                self.ask = Some(AskState {
                    id,
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
            _ => {}
        }
    }
}

fn build_tool_lines(name: &str, content: &str, args: Option<&str>) -> Vec<Line<'static>> {
    match name {
        "read_file" => {
            let json = args.and_then(|a| serde_json::from_str::<serde_json::Value>(a).ok()).unwrap_or_default();
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
                        format!("  ... {} lines total (showing first {max_lines})", total_lines),
                        Style::new().fg(Color::Gray),
                    )));
                }
            }
            out
        }
        "edit_file" | "edit_file_diff" => {
            let json = args.and_then(|a| serde_json::from_str::<serde_json::Value>(a).ok()).unwrap_or_default();
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
        _ => vec![]
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
