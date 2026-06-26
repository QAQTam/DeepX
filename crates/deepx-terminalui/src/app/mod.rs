use deepx_proto::{Agent2Ui, DocInfo, RoundBlock, RoundDeltaKind, TaskInfo, TurnData};
use deepx_types::{ConfigStore, SessionMeta};
use crate::markdown::{render_markdown, render_diff, parse_diff_rows};

// ── Active screen ──

#[derive(PartialEq)]
pub enum Screen {
    Session,
    Chat,
    Menu,
}

// ── Sub-modules ──
pub mod setup;
pub use setup::SetupState;

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
    pub last_error: String,
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
    /// Streaming tool call names collected for status bar display.
    pub streaming_tool_names: Vec<String>,
    pub debug: DebugState,
    pub ask: Option<AskState>,
    pub balance: String,
    /// Live detail pane for exec output or diffs.
    pub detail_pane: Option<DetailPane>,
    /// Tool execution activity log (max 50 entries).
    pub activity_log: Vec<ActivityEntry>,
    pub validating: bool,
    pub busy: bool,
    streaming_rendered_len: usize,
    draft_round_msg_idx: Option<usize>,
    pending_tail_lines: usize,
    pub last_render: std::time::Instant,
    pub cached_line_count: usize,
    pub line_count_version: u64,
    pub line_count_width: u16,
    /// Incremented whenever message content/lines change — used to invalidate
    /// the line-count cache in ui.rs so scrolling stays accurate.
    pub message_version: u64,
    /// Cached rendered text lines (avoid rebuilding on every scroll).
    pub cached_text_lines: Vec<ratatui::text::Line<'static>>,
    pub cached_text_version: u64,
    pub cached_text_width: u16,
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
    /// Tool label for one-liner display (e.g. "read_file src/main.rs")
    pub tool_label: String,
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

/// A single tool execution record for the activity log.
#[derive(Clone)]
pub struct ActivityEntry {
    pub tool_name: String,
    pub summary: String,
    pub success: bool,
    pub time: std::time::Instant,
}

/// Live PTY output pane state — displays exec stdout/stderr in a fixed bottom pane.
#[derive(Clone)]
pub struct PtyPaneState {
    /// Shell command being executed (e.g. "cargo build")
    pub command: String,
    /// Accumulated raw output (may contain ANSI escape codes).
    pub output: String,
    /// When the command started.
    pub started: std::time::Instant,
    /// Whether the command is still running.
    pub running: bool,
    /// Exit status code, if completed.
    pub exit_code: Option<i32>,
}

impl PtyPaneState {
    pub fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            output: String::new(),
            started: std::time::Instant::now(),
            running: true,
            exit_code: None,
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}

/// Side-by-side diff rendering state.
#[derive(Clone)]
pub struct DiffPaneState {
    /// File path or tool label.
    pub label: String,
    /// Parsed diff lines: (old_ln, new_ln, old_body, new_body, kind)
    /// kind is "del" | "add" | "ctx" | "mod".
    pub rows: Vec<(String, String, String, String, String)>,
    /// Scroll offset from top.
    pub scroll_offset: usize,
}

/// Generic tool output pane — read_file, search, web_fetch, etc.
#[derive(Clone)]
pub struct OutputPaneState {
    /// Tool label (e.g. "read_file src/main.rs")
    pub label: String,
    /// Full output text.
    pub output: String,
    /// Scroll offset from top.
    pub scroll_offset: usize,
}

/// Bottom detail pane — PTY, diff, or generic tool output.
#[derive(Clone)]
pub enum DetailPane {
    Pty(PtyPaneState),
    Diff(DiffPaneState),
    Output(OutputPaneState),
}

impl DetailPane {
    pub fn scroll_up(&mut self, n: usize) {
        match self {
            DetailPane::Diff(d) => d.scroll_offset = d.scroll_offset.saturating_add(n),
            DetailPane::Output(o) => o.scroll_offset = o.scroll_offset.saturating_add(n),
            _ => {}
        }
    }
    pub fn scroll_down(&mut self, n: usize) {
        match self {
            DetailPane::Diff(d) => d.scroll_offset = d.scroll_offset.saturating_sub(n),
            DetailPane::Output(o) => o.scroll_offset = o.scroll_offset.saturating_sub(n),
            _ => {}
        }
    }
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
    pub tasks: Vec<TaskInfo>,
    pub recent_edits: Vec<String>,
}

#[derive(Clone)]
pub struct MenuState {
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub editing: bool,
    pub edit_buf: String,
    pub status: String,
    pub profiles: std::collections::HashMap<String, deepx_types::ProfileConfig>,
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

        let (default_pid, default_ep) = deepx_config::registry::first_provider_endpoint();
        let default_base = deepx_config::registry::base_url_for(&default_pid, &default_ep);
        let default_model = deepx_config::registry::default_model_for(&default_pid, &default_ep);

        let model = config.as_ref().and_then(|c| c.model.clone()).unwrap_or_else(|| default_model.clone());
        let context_limit = config.as_ref().and_then(|c| c.context_limit).unwrap_or(1_000_000);
        let max_tokens = config.as_ref().and_then(|c| c.max_tokens).unwrap_or(16384);
        let provider_id = config.as_ref().and_then(|c| c.provider_id.clone()).unwrap_or_else(|| default_pid.clone());
        let endpoint = config.as_ref().and_then(|c| c.endpoint.clone()).unwrap_or_else(|| default_ep.clone());
        let protocol = deepx_config::registry::protocol_for(&provider_id, &endpoint);
        let reasoning_effort = config.as_ref().and_then(|c| c.reasoning_effort.clone()).unwrap_or_else(|| "high".into());
        let base_url = config.as_ref().and_then(|c| c.base_url.clone()).unwrap_or_else(|| default_base);
        let active_profile = config.as_ref().and_then(|c| c.active_profile.clone()).unwrap_or_else(|| "default".into());
        let profiles = config.as_ref().and_then(|c| c.profiles.clone()).unwrap_or_default();

        let c7_key = config.as_ref().and_then(|c| c.context7_api_key.clone()).unwrap_or_default();

        let session_seed = app.debug.session_seed.clone();
        let ws_root = if session_seed.is_empty() {
            ".".to_string()
        } else {
            let ws_path = deepx_types::platform::sessions_dir().join(&session_seed).join("workspace.txt");
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
            let providers = deepx_config::registry::all_providers();
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
                let providers = deepx_config::registry::all_providers();
                let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
                if let Some(cur) = ids.iter().position(|&p| p == item.value) {
                    item.value = ids[(cur + 1) % ids.len()].to_string();
                }
                let new_pid = &item.value;
                let ep = deepx_config::registry::first_endpoint_for(new_pid)
                    .map(|e| e.id).unwrap_or_else(|| "openai".into());
                let proto = deepx_config::registry::protocol_for(new_pid, &ep);
                let burl = deepx_config::registry::base_url_for(new_pid, &ep);
                let def_model = deepx_config::registry::default_model_for(new_pid, &ep);
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
                let proto = deepx_config::registry::protocol_for(&pid_value, &next_endpoint);
                let burl = deepx_config::registry::base_url_for(&pid_value, &next_endpoint);
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
                let ep = deepx_config::registry::find_endpoint(&pid_value, "");
                let models: Vec<String> = ep
                    .map(|e| if e.models.is_empty() { vec![] } else { e.models })
                    .unwrap_or_default();
                let current = item.value.as_str();
                let idx = models.iter().position(|m| m == current);
                item.value = match idx {
                    Some(i) if i + 1 < models.len() => models[i + 1].clone(),
                    Some(_) => models[0].clone(),
                    None => models.first().cloned().unwrap_or_default(),
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
                    config.api_key = Some(item.secret.trim().to_string());
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
        let dir = deepx_types::platform::sessions_dir().join(&self.session_seed);
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
    // Avoid duplicate: if args_display equals or starts with the tool name, skip it
    let args = if args_display == name || args_display.starts_with(&format!("{} ", name)) {
        args_display.to_string()
    } else if args_display.is_empty() {
        name.to_string()
    } else {
        format!("{} {}", name, args_display)
    };
    match name {
        "exec" => format!("{} exec: {}", icon, args.trim_start_matches("exec ").trim_start_matches("exec: ")),
        _ => format!("{} {}", icon, args),
    }
}

fn format_tool_result_summary(output: &str, success: bool) -> String {
    if !success {
        let first_line = output.lines().next().unwrap_or("failed");
        let clean = first_line
            .strip_prefix("[ERROR] ").unwrap_or(first_line)
            .split(" | ").next().unwrap_or(first_line);
        return format!("✗ {}", clean.chars().take(80).collect::<String>());
    }
    // Structured output with [OK] prefix — strip prefix and tool suffix
    if output.starts_with("[OK] ") || output.starts_with("[DRY RUN] ") {
        let first_line = output.lines().next().unwrap_or("done");
        let clean = first_line
            .strip_prefix("[OK] ").unwrap_or_else(|| first_line.strip_prefix("[DRY RUN] ").unwrap_or(first_line))
            .split(" | ").next().unwrap_or(first_line);
        return clean.chars().take(80).collect();
    }
    // Unstructured output (read_file, search, etc.) — show line/byte count
    let lines = output.lines().count();
    let bytes = output.len();
    if lines > 0 {
        format!("{} lines, {} bytes", lines, bytes)
    } else {
        "empty".into()
    }
}

/// Extract a short label from tool output for the diff pane title.
fn format_tool_label_from_output(output: &str) -> String {
    // Try to find the file header in a unified diff
    for line in output.lines() {
        if line.starts_with("--- ") {
            return line[4..].to_string();
        }
        if line.starts_with("+++ ") {
            return line[4..].to_string();
        }
    }
    // Fallback: first non-empty line
    output.lines().find(|l| !l.trim().is_empty()).unwrap_or("diff").chars().take(60).collect()
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
            last_error: String::new(),
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
            line_count_version: 0,
            line_count_width: 0,
            message_version: 0,
            cached_text_lines: Vec::new(),
            cached_text_version: 0,
            cached_text_width: 0,
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
            streaming_tool_names: Vec::new(),
            debug: DebugState {
                hp_connected: false,
                session_seed: String::new(),
                context_tokens: 0,
                tool_calls_total: 0,
                tool_failures: 0,
                streaming: false,
                dsml_compat_count: 0,
                documents: Vec::new(),
                tasks: Vec::new(),
                recent_edits: Vec::new(),
            },
            ask: None,
            balance: String::new(),
            detail_pane: None,
            activity_log: Vec::new(),
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

    pub fn tasks(&self) -> &[TaskInfo] {
        &self.debug.tasks
    }

    pub fn push_msg(&mut self, role: ChatRole, content: &str) {
        // Flush previous renderer state before starting a new message
        self.finalize_last_message();
        self.message_version = self.message_version.wrapping_add(1);

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
        let lines = render_markdown(&content);
        self.streaming_rendered_len = content.len();
        self.last_msg_time = std::time::Instant::now();
        self.messages.push(ChatMessage { role, content, lines, tool_status: ToolStatus::None, tool_id: String::new(), tool_label: String::new() });
    }

    /// Push a message placeholder for streaming — rendering is deferred to append_last().
    fn push_streaming_msg(&mut self, role: ChatRole, content: &str) {
        self.streaming_rendered_len = 0;
        self.pending_tail_lines = 0;
        self.message_version = self.message_version.wrapping_add(1);
        self.messages.push(ChatMessage { role, content: content.to_string(), lines: Vec::new(), tool_status: ToolStatus::None, tool_id: String::new(), tool_label: String::new() });
    }

    fn append_last(&mut self, content: &str) {
        if let Some(last) = self.messages.last_mut() {
            self.message_version = self.message_version.wrapping_add(1);
            last.content.push_str(content);
            // Re-render full text with pulldown-cmark — fast enough for streaming
            last.lines = render_markdown(&last.content);
        }
    }

    /// Push or update a tool message, keyed by tool_id to avoid duplicates.
    fn push_tool_msg(&mut self, tool_id: &str, label: &str, status: ToolStatus) {
        self.finalize_last_message();
        self.upsert_tool_card(tool_id, label, status);
    }

    fn upsert_tool_card(&mut self, tool_id: &str, label: &str, status: ToolStatus) {
        self.message_version = self.message_version.wrapping_add(1);
        let content = label.to_string();
        let lines = render_markdown(&content);
        self.last_msg_time = std::time::Instant::now();
        // Upsert: update existing card if one exists with same tool_id
        if let Some(msg) = self.messages.iter_mut().rev()
            .find(|m| m.role == ChatRole::Tool && m.tool_id == tool_id)
        {
            msg.content = content;
            msg.lines = lines;
            msg.tool_status = status;
        } else {
            self.messages.push(ChatMessage {
                role: ChatRole::Tool,
                content,
                lines,
                tool_status: status,
                tool_id: tool_id.to_string(),
                tool_label: label.to_string(),
            });
        }
    }

    /// Re-render the last message with full markdown — called at end of streaming.
    fn finalize_last_message(&mut self) {
        if let Some(last) = self.messages.last_mut() {
            if !last.content.is_empty() {
                last.lines = render_markdown(&last.content);
            }
        }
        self.streaming_rendered_len = 0;
        self.pending_tail_lines = 0;
    }

    pub fn handle_frame(&mut self, frame: Agent2Ui) {
        match frame {
            Agent2Ui::TurnStart { turn_id: _, user_text } => {
                self.streaming = false;
                self.debug.streaming = false;
                self.tool_batch_start = None;
                self.tool_batch_total = 0;
                self.tool_batch_done = 0;
                self.last_error.clear();
                self.detail_pane = None;
                self.streaming_tool_names.clear();
                // Reset streaming draft state across turns — prevents stale
                // draft_round_msg_idx from a cancelled previous turn leaking
                // into the next RoundComplete and truncating wrong messages.
                self.draft_round_msg_idx = None;
                self.streaming_rendered_len = 0;
                self.pending_tail_lines = 0;
                self.push_msg(ChatRole::Divider, "");
                self.push_msg(ChatRole::User, &user_text);
                self.scroll_offset = 0;
            }
            Agent2Ui::TurnEnd { turn_id: _, stop_reason: _, usage } => {
                if let Some(u) = &usage {
                    self.context_tokens = u.prompt_tokens;
                    self.session_tokens += u.total_tokens as u64;
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
                            // Open PTY pane for exec commands
                            if card.name == "exec" {
                                self.detail_pane = Some(DetailPane::Pty(PtyPaneState::new(&card.args_display)));
                                self.message_version = self.message_version.wrapping_add(1);
                            }
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
                        msg.tool_status = if r.success { ToolStatus::Success } else { ToolStatus::Failed };

                        // Detect tool type from label, not output (all tools use [OK] now)
                        let label = if msg.tool_label.is_empty() { "tool".into() } else { msg.tool_label.clone() };
                        let is_exec = label.starts_with("⚡") || label.contains("exec");
                        let is_diff = render_diff(&r.output).is_some();

                        // One-liner card: just "label summary" (no redundant [OK] prefix)
                        msg.content = format!("{} {}", label, summary);
                        msg.lines = render_markdown(&msg.content);
                        self.message_version = self.message_version.wrapping_add(1);

                        // Populate detail pane
                        if is_exec {
                            if matches!(self.detail_pane, Some(DetailPane::Pty(_))) {
                                // already set up by RoundComplete + ExecProgress
                            }
                        } else if is_diff {
                            let diff_label = format_tool_label_from_output(&r.output);
                            self.detail_pane = Some(DetailPane::Diff(DiffPaneState {
                                label: diff_label,
                                rows: parse_diff_rows(&r.output),
                                scroll_offset: 0,
                            }));
                        } else {
                            // Generic tool output
                            self.detail_pane = Some(DetailPane::Output(OutputPaneState {
                                label,
                                output: r.output.clone(),
                                scroll_offset: 0,
                            }));
                        }
                    }
                    if r.success {
                        if let Some(json_str) = r.output.strip_prefix("[USER_QUERY] ") {
                            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(json_str) {
                                let question = payload.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let options: Vec<String> = payload.get("options").and_then(|v| v.as_array())
                                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                    .unwrap_or_default();
                                self.ask = Some(AskState {
                                    question,
                                    options,
                                    selected: 0,
                                    custom_input: String::new(),
                                });
                            }
                        }
                    }
                }
                // Advance tool batch progress
                self.tool_batch_done += results.len() as u32;
                if self.tool_batch_done >= self.tool_batch_total {
                    self.tool_batch_start = None;
                    self.tool_batch_total = 0;
                    self.tool_batch_done = 0;
                }
                // Finalize PTY pane if any exec completed
                if let Some(DetailPane::Pty(ref mut pane)) = self.detail_pane {
                    if pane.running {
                        for r in &results {
                            if r.success {
                                pane.running = false;
                                if let Some(code_start) = r.output.rfind("[EXIT:") {
                                    let rest = &r.output[code_start + 6..];
                                    if let Some(end) = rest.find(']') {
                                        pane.exit_code = rest[..end].parse().ok();
                                    }
                                }
                                if pane.exit_code.is_none() { pane.exit_code = Some(0); }
                                self.message_version = self.message_version.wrapping_add(1);
                                break;
                            }
                        }
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
                self.draft_round_msg_idx = None;
                let status_text = format!("{}: {}", self.setup.lang.t_chat_error(), message);
                self.push_msg(ChatRole::Status, &status_text);
                self.status = status_text.clone();
                self.last_error = status_text;
            }
            Agent2Ui::ToolNotice { ref message, ref level } => {
                let prefix = if level == "error" { "\u{26a0}" } else { "\u{2139}" };
                let text = format!("{prefix} {message}");
                self.push_msg(ChatRole::Status, &text);
            }
            Agent2Ui::Done => {
                self.draft_round_msg_idx = None;
                self.status = self.setup.lang.t_chat_ready().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.busy = false;
                self.scroll_offset = 0;
                self.finalize_last_message();
                // Terminal bell — flashes taskbar / sounds beep when answer completes
                use std::io::Write;
                let _ = std::io::stdout().write_all(b"\x07");
                let _ = std::io::stdout().flush();
            }
            Agent2Ui::Cancelled => {
                self.draft_round_msg_idx = None;
                self.status = self.setup.lang.t_chat_cancelled().to_string();
                self.streaming = false;
                self.debug.streaming = false;
                self.busy = false;
                self.scroll_offset = 0;
                self.finalize_last_message();
            }
            Agent2Ui::Dashboard { hp_connected, session_seed, usage, context_limit,
                tool_calls_total, tool_failures, current_phase: _, streaming, dsml_compat_count, documents, tasks, recent_edits, .. } => {
                self.debug.hp_connected = hp_connected;
                self.debug.session_seed = session_seed;
                self.debug.context_tokens = usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0);
                self.context_limit = context_limit;
                self.debug.tool_calls_total = tool_calls_total;
                self.debug.tool_failures = tool_failures;
                self.debug.streaming = streaming;
                self.debug.dsml_compat_count = dsml_compat_count;
                self.debug.documents = documents;
                self.debug.tasks = tasks;
                self.debug.recent_edits = recent_edits;
            }
            // AskUser variant removed — no longer in proto
            Agent2Ui::Balance { is_available, total_balance, currency } => {
                let status = if is_available { "✓" } else { "✗" };
                self.balance = format!("{} {}{} {}", status, if currency == "CNY" { "¥" } else { "$" }, total_balance, currency);
            }
            Agent2Ui::Ready => {} // Handshake frame from agent subprocess — ready to accept commands
            Agent2Ui::ShutdownAck => {
                self.streaming = false;
                self.debug.streaming = false;
                if self.setup.lang.as_str() == "zh" {
                    self.status = "Agent 已关闭".into();
                } else {
                    self.status = "Agent shut down".into();
                }
            }
            Agent2Ui::ToolCallPreview { turn_id: _, round_num: _, index: _, id: _, name, .. } => {
                // Tool calls during streaming — show compact summary in status bar
                // Deduplicate: only add name if not already tracked this round
                if !self.streaming_tool_names.contains(&name) {
                    self.streaming_tool_names.push(name);
                }
                let count = self.streaming_tool_names.len();
                let names: String = self.streaming_tool_names.iter()
                    .map(|n| tool_icon(n).to_string() + " " + n)
                    .collect::<Vec<_>>()
                    .join(", ");
                let label = if self.setup.lang.as_str() == "zh" {
                    format!("🔧 {} 个工具调用中：{}…", count, names)
                } else {
                    format!("🔧 {} tool(s): {}…", count, names)
                };
                self.status = label.chars().take(80).collect();
            }
            Agent2Ui::ExecProgress { tool_call_id, chunk } => {
                // Stream exec stdout/stderr into the tool card in real-time.
                if let Some(msg) = self.messages.iter_mut().rev()
                    .find(|m| m.role == ChatRole::Tool && m.tool_id == tool_call_id)
                {
                    msg.content.push_str(&chunk);
                    self.message_version = self.message_version.wrapping_add(1);
                    // PTY output carries ANSI escape codes — route to ANSI renderer
                    msg.lines = if crate::markdown::has_ansi(&msg.content) {
                        crate::markdown::render_ansi(&msg.content)
                    } else {
                        render_markdown(&msg.content)
                    };
                }
                // Also feed the live PTY pane
                if let Some(DetailPane::Pty(ref mut pane)) = self.detail_pane {
                    pane.output.push_str(&chunk);
                    self.message_version = self.message_version.wrapping_add(1);
                }
            }
            Agent2Ui::MoreTurns { turns, has_more: _ } => {
                // Prepend older turns loaded lazily (user scrolled up).
                self.prepend_turns(&turns);
            }
            Agent2Ui::CompactStart { turns_total: _, turns_keeping: _ } => {
                self.status = if self.setup.lang.as_str() == "zh" {
                    "正在压缩上下文...".into()
                } else {
                    "Compacting context...".into()
                };
            }
            Agent2Ui::CompactEnd { summary_chars: _, turns_compacted } => {
                self.status = if self.setup.lang.as_str() == "zh" {
                    format!("上下文压缩完成 ({} turn)", turns_compacted)
                } else {
                    format!("Context compacted ({} turns)", turns_compacted)
                };
            }
            Agent2Ui::AuditRecord { tool_name, result_summary, success } => {
                self.activity_log.push(ActivityEntry {
                    tool_name,
                    summary: result_summary,
                    success,
                    time: std::time::Instant::now(),
                });
                if self.activity_log.len() > 50 {
                    self.activity_log.remove(0);
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

    /// Prepend older turns at front of message list (lazy history loading).
    fn prepend_turns(&mut self, turns: &[TurnData]) {
        let mut new_msgs: Vec<ChatMessage> = Vec::new();
        for turn in turns {
            new_msgs.push(ChatMessage {
                role: ChatRole::Divider, content: String::new(), lines: Vec::new(),
                tool_status: ToolStatus::None, tool_id: String::new(), tool_label: String::new(),
            });
            let content = &turn.user_text;
            let md_lines = render_markdown(content);
            new_msgs.push(ChatMessage {
                role: ChatRole::User, content: content.clone(), lines: md_lines,
                tool_status: ToolStatus::None, tool_id: String::new(), tool_label: String::new(),
            });
            for round in &turn.rounds {
                if let Some(ref t) = round.thinking { if !t.is_empty() {
                    let md_lines = render_markdown(t);
                    new_msgs.push(ChatMessage {
                        role: ChatRole::Thinking, content: t.clone(), lines: md_lines,
                        tool_status: ToolStatus::None, tool_id: String::new(), tool_label: String::new(),
                    });
                }}
                if let Some(ref a) = round.answer { if !a.is_empty() {
                    let md_lines = render_markdown(a);
                    new_msgs.push(ChatMessage {
                        role: ChatRole::Assistant, content: a.clone(), lines: md_lines,
                        tool_status: ToolStatus::None, tool_id: String::new(), tool_label: String::new(),
                    });
                }}
                for tc in &round.tool_calls {
                    let label = format_tool_label(&tc.name, &tc.args_display);
                    let mut status = ToolStatus::Success;
                    if let Some(tr) = round.tool_results.iter().find(|r| r.tool_call_id == tc.id) {
                        if !tr.success { status = ToolStatus::Failed; }
                    }
                    let md_lines = render_markdown(&label);
                    new_msgs.push(ChatMessage {
                        role: ChatRole::Tool, content: label.clone(), lines: md_lines,
                        tool_status: status, tool_id: tc.id.clone(),
                        tool_label: label,
                    });
                }
            }
        }
        if !new_msgs.is_empty() { self.messages.splice(0..0, new_msgs); self.message_version = self.message_version.wrapping_add(1); }
    }
}