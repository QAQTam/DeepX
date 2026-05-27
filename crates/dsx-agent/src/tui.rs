//! DSX TUI — single-process terminal UI for the coding agent.
//!
//! Layout (split-footer with optional side panel):
//!   ┌──────────────────────────┬───────────┐
//!   │      Chat (scrollback)   │   Side    │
//!   │  messages, markdown,     │  tools,   │
//!   │  tool output             │  files    │
//!   ├──────────────────────────┴───────────┤
//!   │        Footer (fixed)                │
//!   │  ┃ input area                        │
//!   │  ┃ model · phase · ctx · tokens      │
//!   └──────────────────────────────────────┘

use std::io::BufReader;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use dsx_proto::{AgentToTui, TuiToAgent};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};

use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, LineGauge, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use ratatui::Terminal;
use tui_textarea::TextArea;

use crate::agent::AgentState;
use crate::config::Config;

// ── Semantic colors ──

mod theme {
    use ratatui::style::Color;
    pub const BG:        Color = Color::from_u32(0x001e1e2e);
    pub const SURFACE:   Color = Color::from_u32(0x00313244);
    pub const OVERLAY:   Color = Color::from_u32(0x0045465a);
    pub const TEXT:      Color = Color::from_u32(0x00cdd6f4);
    pub const SUBTEXT:   Color = Color::from_u32(0x00a6adc8);
    pub const MUTED:     Color = Color::from_u32(0x006c7086);
    pub const GREEN:     Color = Color::from_u32(0x00a6e3a1);
    pub const RED:       Color = Color::from_u32(0x00f38ba8);
    pub const YELLOW:    Color = Color::from_u32(0x00f9e2af);
    pub const BLUE:      Color = Color::from_u32(0x0089b4fa);
    pub const CYAN:      Color = Color::from_u32(0x0089dceb);
    pub const MAUVE:     Color = Color::from_u32(0x00cba6f7);
    pub const PEACH:     Color = Color::from_u32(0x00fab387);

    pub struct Resolved {
        pub bg: Color, pub surface: Color, pub overlay: Color,
        pub text: Color, pub subtext: Color, pub muted: Color,
        pub green: Color, pub red: Color, pub yellow: Color,
        pub blue: Color, pub cyan: Color, pub mauve: Color,
        #[allow(dead_code)]
        pub peach: Color,
    }
    pub const DEFAULT: Resolved = Resolved {
        bg: BG, surface: SURFACE, overlay: OVERLAY,
        text: TEXT, subtext: SUBTEXT, muted: MUTED,
        green: GREEN, red: RED, yellow: YELLOW,
        blue: BLUE, cyan: CYAN, mauve: MAUVE, peach: PEACH,
    };
}
use theme::Resolved;

const SPINNER: [&str; 10] = ["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"];

// ── Markdown rendering ──

fn markdown_to_text(md: &str, width: u16, _t: &Resolved) -> Text<'static> {
    use limner::render_markdown;
    use limner::style::MarkdownStyle;
    let style = MarkdownStyle::default();
    let result = render_markdown(md, &style, width);
    Text::from(result.lines)
}

// ── Syntax highlighting ──

fn highlight_code(code: &str, lang: &str, _t: &Resolved) -> Text<'static> {
    use syntect::easy::HighlightLines;
    use syntect::highlighting::ThemeSet;
    use syntect::parsing::SyntaxSet;
    use syntect::util::LinesWithEndings;

    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ps.find_syntax_by_token(lang)
        .unwrap_or_else(|| ps.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, &ts.themes["base16-ocean.dark"]);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges: Vec<(syntect::highlighting::Style, &str)> = match h.highlight_line(line, &ps) {
            Ok(r) => r,
            Err(_) => { lines.push(Line::from(line.to_string())); continue; }
        };
        let spans: Vec<Span> = ranges.into_iter().map(|(style, text)| {
            let fg = ratatui::style::Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
            Span::styled(text.to_string(), Style::default().fg(fg))
        }).collect();
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

fn render_message_body(content: &str, role: &str, t: &Resolved, width: u16) -> Text<'static> {
    if role == "assistant" {
        return markdown_to_text(content, width, t);
    }

    let owned = content.to_string();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let body_color = match role {
        "error" => t.red,
        "tool" => t.subtext,
        _ => t.text,
    };
    let wrap_w = width.saturating_sub(2) as usize;

    for line in owned.lines() {
        if line.starts_with("```") && !in_code {
            in_code = true;
            code_lang = line[3..].trim().to_string();
            code_buf.clear();
            continue;
        }
        if line.starts_with("```") && in_code {
            in_code = false;
            let highlighted = highlight_code(&code_buf, &code_lang, t);
            for hl_line in highlighted.lines {
                for wrapped in wrap_line(&hl_line_to_str(&hl_line), wrap_w) {
                    lines.push(Line::from(wrapped));
                }
            }
            continue;
        }
        if in_code {
            if !code_buf.is_empty() { code_buf.push('\n'); }
            code_buf.push_str(line);
            continue;
        }

        let styled = if role == "tool" {
            if line.starts_with('+') {
                Line::from(vec![Span::styled(line.to_string(), Style::default().fg(t.green))])
            } else if line.starts_with('-') {
                Line::from(vec![Span::styled(line.to_string(), Style::default().fg(t.red))])
            } else {
                Line::from(vec![Span::styled(line.to_string(), Style::default().fg(body_color))])
            }
        } else {
            Line::from(vec![Span::styled(line.to_string(), Style::default().fg(body_color))])
        };
        lines.push(styled);
    }
    if in_code && !code_buf.is_empty() {
        let highlighted = highlight_code(&code_buf, &code_lang, t);
        for hl_line in highlighted.lines {
            for wrapped in wrap_line(&hl_line_to_str(&hl_line), wrap_w) {
                lines.push(Line::from(wrapped));
            }
        }
    }
    Text::from(lines)
}

// ── Command menu (with nucleo fuzzy matching) ──

struct CommandMenu {
    items: Vec<(&'static str, &'static str)>,
    filtered: Vec<usize>,
    cursor: usize,
    sub: Option<(String, Vec<&'static str>)>,
    matcher: nucleo::Matcher,
}

fn sub_options(cmd: &str) -> Vec<&'static str> {
    match cmd {
        "/lang" => vec!["zh", "en"],
        "/effort" => vec!["high", "max", "off"],
        "/profile" => vec!["list", "save"],
        _ => vec![],
    }
}

impl CommandMenu {
    fn new(lang: &str) -> Self {
        let items = crate::config::all_commands(lang);
        let n = items.len();
        Self {
            items, filtered: (0..n).collect(), cursor: 0, sub: None,
            matcher: nucleo::Matcher::new(nucleo::Config::DEFAULT),
        }
    }

    fn update_filter(&mut self, prefix: &str) {
        let search = prefix.trim_start_matches('/').trim().to_lowercase();
        if let Some((cmd, rest)) = search.split_once(' ') {
            self.sub = Some((cmd.to_string(), sub_options(cmd)));
            let subs = sub_options(cmd);
            let mut scored: Vec<(i32, usize)> = subs.iter().enumerate()
                .filter_map(|(i, opt)| {
                    let haystack = nucleo::Utf32Str::Ascii(opt.as_bytes());
                    let needle = nucleo::Utf32Str::Ascii(rest.as_bytes());
                    let score = self.matcher.fuzzy_match(haystack, needle)?;
                    Some((score as i32, i))
                })
                .collect();
            scored.sort_by_key(|(s, _)| -s);
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
            self.cursor = 0;
        } else {
            self.sub = None;
            let needle = nucleo::Utf32Str::Ascii(search.as_bytes());
            let mut scored: Vec<(i32, usize)> = self.items.iter().enumerate()
                .filter_map(|(i, (cmd, desc))| {
                    let text = format!("{} {}", cmd.trim_start_matches('/'), desc);
                    let haystack = nucleo::Utf32Str::Ascii(text.as_bytes());
                    let score = self.matcher.fuzzy_match(haystack, needle)?;
                    Some((score as i32, i))
                })
                .collect();
            scored.sort_by_key(|(s, _)| -s);
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
            self.cursor = 0;
        }
    }

    fn select(&mut self) -> Option<String> {
        if let Some((ref parent, ref subs)) = self.sub {
            if !self.filtered.is_empty() {
                let idx = self.filtered[self.cursor.min(self.filtered.len().saturating_sub(1))];
                let opt = subs.get(idx).unwrap_or(&"?");
                return Some(format!("{parent} {opt}"));
            }
        } else if !self.filtered.is_empty() {
            let idx = self.filtered[self.cursor.min(self.filtered.len().saturating_sub(1))];
            let (cmd, _) = self.items[idx];
            let subs = sub_options(cmd);
            if !subs.is_empty() {
                self.sub = Some((cmd.to_string(), subs.clone()));
                self.filtered = (0..subs.len()).collect();
                self.cursor = 0;
                return None;
            }
            return Some(cmd.to_string());
        }
        None
    }

    fn back(&mut self) -> bool {
        if self.sub.is_some() {
            self.sub = None;
            self.filtered = (0..self.items.len()).collect();
            self.cursor = 0;
            true
        } else {
            false
        }
    }
}

// ── ToolEntry ──

struct ToolEntry {
    name: String,
    output: String,
    success: bool,
    running: bool,
}

// ── Screen modes ──

enum Screen {
    Chat,
    Setup(SetupState),
    Resume(ResumeState),
}

struct ResumeState {
    sessions: Vec<dsx_types::SessionMeta>,
    cursor: usize,
}

struct SetupState {
    api_key: String,
    model: String,
    base_url: String,
    effort_idx: usize,
    lang_idx: usize,
    focus: usize,
}

// ── App ──

const FOOTER_HEIGHT: u16 = 5;

struct App {
    screen: Screen,
    config: Config,
    theme: Resolved,

    messages: Vec<(String, String, String)>,
    streaming_content: String,
    streaming_reasoning: String,
    saved_reasoning: Option<String>,
    show_reasoning: bool,
    tools: Vec<ToolEntry>,
    phase: String,
    cache_hit_pct: f64,
    tokens_used: u32,
    session_seed: String,
    status: String,
    context_limit: u32,

    explored: bool,
    files_read_this_turn: Vec<String>,
    files_written_this_turn: Vec<String>,
    ask_question: Option<String>,
    is_streaming: bool,

    textarea: TextArea<'static>,
    auto_scroll: bool,
    scroll_offset: usize,
    scroll_state: ScrollbarState,
    command_menu: Option<CommandMenu>,

    agent_tx: mpsc::Sender<TuiToAgent>,
    agent_rx: mpsc::Receiver<AgentToTui>,
    exit: bool,
}

impl App {
    fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;

        loop {
            terminal.draw(|f| self.ui(f))?;
            if self.exit { break; }

            if crossterm::event::poll(Duration::from_millis(30))? {
                let ev = crossterm::event::read()?;
                let prev = std::mem::replace(&mut self.screen, Screen::Chat);
                self.screen = match prev {
                    Screen::Setup(s) => self.run_setup_input(ev, s),
                    Screen::Resume(s) => self.run_resume_input(ev, s),
                    Screen::Chat => { self.handle_chat_input(ev); Screen::Chat },
                };
            }

            if matches!(self.screen, Screen::Chat) {
                while let Ok(frame) = self.agent_rx.try_recv() {
                    self.handle_frame(frame);
                }
            }
        }

        terminal.clear()?;
        Ok(())
    }

    // ── Chat input ──

    fn handle_chat_input(&mut self, ev: Event) {
        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => {
                    if let Some(ref mut menu) = self.command_menu {
                        if let Some(cmd) = menu.select() {
                            self.textarea = TextArea::default();
                            self.command_menu = None;
                            let resp = self.handle_slash_command(&cmd);
                            self.messages.push(("assistant".into(), resp, now_ts()));
                            return;
                        }
                        return;
                    }
                    let lines: Vec<String> = self.textarea.lines().to_vec();
                    let msg = lines.join("\n").trim().to_string();
                    self.textarea = TextArea::default();
                    self.command_menu = None;
                    if msg.is_empty() { return; }

                    if msg.starts_with('/') && crate::config::is_command(&msg) {
                        let resp = self.handle_slash_command(&msg);
                        self.messages.push(("assistant".into(), resp, now_ts()));
                        return;
                    }

                    self.messages.push(("user".into(), msg.clone(), now_ts()));
                    self.scroll_offset = 0;
                    self.auto_scroll = true;
                    let _ = self.agent_tx.send(TuiToAgent::UserInput { text: msg });
                    self.status.clear();
                    self.tools.clear();
                    self.explored = false;
                    self.files_read_this_turn.clear();
                    self.files_written_this_turn.clear();
                    self.ask_question = None;
                    self.saved_reasoning = None;
                }
                KeyCode::Up | KeyCode::Down if self.command_menu.is_some() => {
                    if let Some(ref mut menu) = self.command_menu {
                        if key.code == KeyCode::Up {
                            menu.cursor = menu.cursor.saturating_sub(1);
                        } else {
                            let max = menu.filtered.len().saturating_sub(1);
                            menu.cursor = (menu.cursor + 1).min(max);
                        }
                    }
                }
                KeyCode::Esc => {
                    if let Some(ref mut menu) = self.command_menu {
                        if !menu.back() { self.command_menu = None; }
                    } else {
                        let _ = self.agent_tx.send(TuiToAgent::Cancel);
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let _ = self.agent_tx.send(TuiToAgent::Cancel);
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let _ = self.agent_tx.send(TuiToAgent::Shutdown);
                    self.exit = true;
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.show_reasoning = !self.show_reasoning;
                }
                KeyCode::PageUp => { self.auto_scroll = false; self.scroll_offset = self.scroll_offset.saturating_add(10); }
                KeyCode::PageDown => {
                    self.scroll_offset = self.scroll_offset.saturating_sub(10);
                    if self.scroll_offset == 0 { self.auto_scroll = true; }
                }
                _ => {
                    self.textarea.input(ev);
                    self.update_command_menu();
                }
            },
            _ => {}
        }
    }

    fn update_command_menu(&mut self) {
        let line = self.textarea.lines().first().map(|s| s.as_str()).unwrap_or("");
        if line.starts_with('/') {
            if self.command_menu.is_none() {
                self.command_menu = Some(CommandMenu::new(&self.config.prompt_lang));
            }
            if let Some(ref mut menu) = self.command_menu {
                menu.update_filter(line);
            }
        } else {
            self.command_menu = None;
        }
    }

    fn handle_slash_command(&mut self, msg: &str) -> String {
        let lang = self.config.prompt_lang.clone();
        if let Some(r) = crate::config::handle_reset_command(msg, &mut self.config) { return r; }
        if let Some(r) = crate::config::handle_reconfig_command(msg, &mut self.config) { return r; }
        if let Some(r) = crate::config::handle_effort_command(msg, &mut self.config) { return r; }
        if let Some(r) = crate::config::handle_lang_command(msg, &mut self.config) { return r; }
        if let Some(r) = crate::config::handle_model_command(msg, &mut self.config) { return r; }
        if let Some(r) = crate::config::handle_profile_command(msg, &mut self.config) { return r; }
        if msg.trim() == "/auto" {
            self.config.auto_mode = !self.config.auto_mode;
            crate::tools::AUTO_MODE.store(self.config.auto_mode, std::sync::atomic::Ordering::Relaxed);
            dsx_tools::AUTO_MODE.store(self.config.auto_mode, std::sync::atomic::Ordering::Relaxed);
            self.config.save();
            return format!("Auto mode: {}", if self.config.auto_mode { "ON" } else { "OFF" });
        }
        format!("Unknown command. Type /menu for all commands.\n\n{}",
            crate::config::all_commands(&lang).iter()
                .map(|(cmd, desc)| format!("  {cmd:12} {desc}"))
                .collect::<Vec<_>>().join("\n"))
    }

    // ── Setup input ──

    fn run_setup_input(&mut self, ev: Event, mut s: SetupState) -> Screen {
        const SAVE_FOCUS: usize = 5;
        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => {
                    if s.api_key.is_empty() { return Screen::Setup(s); }
                    self.apply_setup(&s);
                    return Screen::Chat;
                }
                KeyCode::Enter if s.focus == SAVE_FOCUS => {
                    if s.api_key.is_empty() { return Screen::Setup(s); }
                    self.apply_setup(&s);
                    return Screen::Chat;
                }
                KeyCode::Up => s.focus = s.focus.saturating_sub(1),
                KeyCode::Down => s.focus = (s.focus + 1).min(SAVE_FOCUS),
                KeyCode::Backspace => match s.focus {
                    0 => { s.api_key.pop(); }
                    1 => { s.model.pop(); }
                    2 => { s.base_url.pop(); }
                    _ => {}
                },
                KeyCode::Char(c) => match s.focus {
                    0 => s.api_key.push(c),
                    1 => s.model.push(c),
                    2 => s.base_url.push(c),
                    3 if c == ' ' || c == '\t' => s.effort_idx = (s.effort_idx + 1) % 3,
                    4 if c == ' ' || c == '\t' => s.lang_idx = (s.lang_idx + 1) % 2,
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
        Screen::Setup(s)
    }

    fn apply_setup(&mut self, s: &SetupState) {
        self.config.api_key = s.api_key.clone();
        self.config.model = s.model.clone();
        self.config.base_url = s.base_url.clone();
        self.config.effort = match s.effort_idx {
            0 => None, 1 => Some("high".into()), 2 => Some("max".into()), _ => None,
        };
        self.config.prompt_lang = match s.lang_idx {
            0 => "en".into(), 1 => "zh".into(), _ => "en".into(),
        };
        self.config.save();
    }

    fn run_resume_input(&mut self, ev: Event, mut s: ResumeState) -> Screen {
        // cursor 0 = new session, 1.. = sessions
        let max_cursor = s.sessions.len();
        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Screen::Chat,
                KeyCode::Enter => {
                    if s.cursor == 0 {
                        // New session — proceed to chat
                        return Screen::Chat;
                    }
                    // Resume existing session
                    let idx = s.cursor.saturating_sub(1);
                    if idx < s.sessions.len() {
                        let seed = s.sessions[idx].seed.clone();
                        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("dsx"));
                        let _ = std::process::Command::new(exe).arg("tui").arg("--session").arg(&seed).spawn();
                        self.exit = true;
                    }
                    return Screen::Chat;
                }
                KeyCode::Up => s.cursor = s.cursor.saturating_sub(1),
                KeyCode::Down => s.cursor = (s.cursor + 1).min(max_cursor),
                _ => {}
            },
            _ => {}
        }
        Screen::Resume(s)
    }

    // ── Frame handler ──

    fn handle_frame(&mut self, frame: AgentToTui) {
        match frame {
            AgentToTui::ContentDelta { delta, reasoning } => {
                self.is_streaming = true;
                self.streaming_content.push_str(&delta);
                if let Some(r) = reasoning { self.streaming_reasoning.push_str(&r); }
            }
            AgentToTui::ToolProgress { id, content, .. } => {
                // Ensure a ToolEntry exists; use the most recent running one
                if !self.tools.iter().any(|t| t.running) {
                    self.tools.push(ToolEntry { name: id, output: String::new(), success: false, running: true });
                }
                if let Some(t) = self.tools.iter_mut().rev().find(|t| t.running) {
                    t.output.push_str(&content);
                }
            }
            AgentToTui::ToolResult { id, name, content, success } => {
                let preview = content.lines().next().unwrap_or("").chars().take(80).collect::<String>();
                self.messages.push(("tool".into(), format!("{} {} → {}", if success { "OK" } else { "ER" }, name, preview), now_ts()));
                // Find the running tool: prefer match by progress id, fallback to any running tool
                let idx = self.tools.iter().rposition(|t| t.running && t.name == id)
                    .or_else(|| self.tools.iter().rposition(|t| t.running));
                if let Some(i) = idx {
                    self.tools[i].name = name;
                    self.tools[i].output = content;
                    self.tools[i].success = success;
                    self.tools[i].running = false;
                } else {
                    self.tools.push(ToolEntry { name, output: content, success, running: false });
                }
            }
            AgentToTui::ApiResponse { usage, stop_reason, context_tokens, .. } => {
                self.is_streaming = false;
                if context_tokens > 0 { self.tokens_used = context_tokens; }
                else if let Some(u) = usage { self.tokens_used = (u.prompt_tokens + u.completion_tokens) as u32; }
                if let Some(reason) = stop_reason {
                    if reason != "end_turn" && reason != "stop_sequence" { self.status = reason; }
                }
            }
            AgentToTui::PhaseChanged { phase } => self.phase = phase,
            AgentToTui::ToolState { explored, declared_files: _, read_files, written_this_turn } => {
                self.explored = explored;
                self.files_read_this_turn = read_files;
                self.files_written_this_turn = written_this_turn;
            }
            AgentToTui::Error { message } => { self.messages.push(("error".into(), message, now_ts())); }
            AgentToTui::CachePrediction { hit_rate } => self.cache_hit_pct = hit_rate,
            AgentToTui::SessionRestored { seed, message_count, tokens_used, cache_hit_pct, .. } => {
                self.session_seed = seed; self.tokens_used = tokens_used; self.cache_hit_pct = cache_hit_pct;
                self.status = format!("Resumed: {} messages", message_count);
            }
            AgentToTui::Done => {
                self.is_streaming = false;
                let reasoning = std::mem::take(&mut self.streaming_reasoning);
                let content = std::mem::take(&mut self.streaming_content);
                if !reasoning.is_empty() { self.saved_reasoning = Some(reasoning.clone()); }
                if !reasoning.is_empty() {
                    self.messages.push(("thinking".into(), reasoning, now_ts()));
                }
                if !content.is_empty() {
                    self.messages.push(("assistant".into(), content, now_ts()));
                }
            }
            AgentToTui::AskUser { question, .. } => { self.ask_question = Some(question); }
            _ => {}
        }
    }

    // ── UI dispatcher ──

    fn ui(&mut self, f: &mut Frame) {
        let area = f.area();
        f.render_widget(Paragraph::new("").bg(self.theme.bg), area);

        match &self.screen {
            Screen::Setup(_) => self.render_setup(f, area),
            Screen::Resume(_) => self.render_resume(f, area),
            Screen::Chat => self.render_chat_ui(f, area),
        }
    }

    // ── Setup screen ──

    fn render_setup(&self, f: &mut Frame, area: Rect) {
        let s = match &self.screen { Screen::Setup(s) => s, _ => return };
        let popup = centered_rect(60, 14, area);
        f.render_widget(Clear, popup);
        let t = &self.theme;
        let efforts = ["default", "high", "max"];
        let langs = ["en", "zh"];
        let focus_style = Style::default().fg(t.cyan).add_modifier(Modifier::BOLD);
        let normal = Style::default().fg(t.text);
        let muted = Style::default().fg(t.muted);
        let warning = Style::default().fg(t.red);

        let api_key_empty = s.api_key.is_empty();
        let hint_line = if api_key_empty {
            Line::from(vec![Span::styled("  ⚠ API Key is required  |  ↑↓: navigate  |  Enter: select", warning)])
        } else {
            Line::from(vec![Span::styled("  Esc: save & start  |  ↑↓: navigate  |  Enter: select", muted)])
        };

        let lines = vec![
            Line::from(vec![Span::styled(" DSX — First Launch Setup", focus_style)]),
            Line::from(""),
            Line::from(vec![
                Span::styled(if s.focus == 0 { "▶ " } else { "  " }, focus_style),
                Span::styled("API Key: ", normal),
                Span::styled(mask_key(&s.api_key), if s.focus == 0 { focus_style } else { muted }),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(if s.focus == 1 { "▶ " } else { "  " }, focus_style),
                Span::styled("Model:  ", normal),
                Span::styled(&s.model, if s.focus == 1 { focus_style } else { muted }),
            ]),
            Line::from(vec![
                Span::styled(if s.focus == 2 { "▶ " } else { "  " }, focus_style),
                Span::styled("BaseURL:", normal),
                Span::styled(&s.base_url, if s.focus == 2 { focus_style } else { muted }),
            ]),
            Line::from(vec![
                Span::styled(if s.focus == 3 { "▶ " } else { "  " }, focus_style),
                Span::styled("Effort: ", normal),
                Span::styled(format!("{} ◄►", efforts[s.effort_idx]), if s.focus == 3 { focus_style } else { muted }),
            ]),
            Line::from(vec![
                Span::styled(if s.focus == 4 { "▶ " } else { "  " }, focus_style),
                Span::styled("Lang:   ", normal),
                Span::styled(format!("{} ◄►", langs[s.lang_idx]), if s.focus == 4 { focus_style } else { muted }),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(if s.focus == 5 { "▶ " } else { "  " }, focus_style),
                Span::styled("[ Save & Start ]", if s.focus == 5 { focus_style.add_modifier(Modifier::BOLD) } else { muted }),
            ]),
            Line::from(""),
            hint_line,
        ];

        f.render_widget(
            Paragraph::new(Text::from(lines))
                .block(Block::default().borders(Borders::ALL).title(" Setup ").border_style(Style::default().fg(t.cyan))),
            popup,
        );
    }

    fn render_resume(&self, f: &mut Frame, area: Rect) {
        let s = match &self.screen { Screen::Resume(s) => s, _ => return };
        let t = &self.theme;
        let h = (s.sessions.len() + 6).min(16) as u16;
        let popup = centered_rect(70, h, area);
        f.render_widget(Clear, popup);

        let mut items: Vec<ListItem> = Vec::new();
        items.push(ListItem::new(Line::from(vec![
            Span::styled(" Sessions found — select or start new", Style::default().fg(t.cyan)),
        ])));

        // NEW SESSION always visible at top
        let new_sel = s.cursor == 0;
        let new_style = if new_sel {
            Style::default().fg(t.green).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(t.green) };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(if new_sel { "▶" } else { " " }, new_style),
            Span::styled(" +  New session  ", new_style),
            Span::styled("(start fresh)", Style::default().fg(t.muted)),
        ])));
        items.push(ListItem::new(""));

        for (i, meta) in s.sessions.iter().enumerate() {
            let ci = i + 1; // cursor offset by 1 for "new session" row
            let style = if ci == s.cursor {
                Style::default().fg(t.cyan).add_modifier(Modifier::BOLD)
            } else { Style::default().fg(t.text) };
            let prefix = if ci == s.cursor { "▶" } else { " " };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(format!("{prefix} "), style),
                Span::styled(format!("{} ", meta.seed), Style::default().fg(t.blue)),
                Span::styled(format!("{}msgs ", meta.message_count), Style::default().fg(t.subtext)),
                Span::styled(meta.last_summary.chars().take(50).collect::<String>(), Style::default().fg(t.muted)),
            ])));
        }

        f.render_widget(List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Resume ").border_style(Style::default().fg(t.cyan))),
            popup,
        );
    }

    // ── Chat UI (split-footer layout) ──

    fn render_chat_ui(&self, f: &mut Frame, area: Rect) {
        let t = &self.theme;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(FOOTER_HEIGHT)])
            .split(area);

        self.render_scrollback(f, chunks[0], t);
        self.render_footer(f, chunks[1], t);
    }

    fn render_scrollback(&self, f: &mut Frame, area: Rect, t: &Resolved) {
        let has_side = !self.tools.is_empty() || !self.files_written_this_turn.is_empty() || self.explored;
        let chunks = if has_side {
            Layout::default().direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(62), Constraint::Percentage(38)]).split(area)
        } else {
            Layout::default().direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100), Constraint::Percentage(0)]).split(area)
        };
        self.render_chat(f, chunks[0], t);
        if has_side { self.render_side(f, chunks[1], t); }
    }

    fn render_chat(&self, f: &mut Frame, area: Rect, t: &Resolved) {
        let mut lines: Vec<Line> = Vec::new();
        let inner_w = area.width.saturating_sub(2) as usize;
        let sep_w = inner_w.saturating_sub(20);
        let sep = "─".repeat(sep_w);
        let md_width = area.width.saturating_sub(2);

        for (role, content, ts) in &self.messages {
            let header_line = match role.as_str() {
                "user" => Line::from(vec![
                    Span::styled(sep.clone(), Style::default().fg(t.surface)),
                    Span::styled(" You ", Style::default().fg(t.blue).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {ts} "), Style::default().fg(t.muted)),
                ]),
                "thinking" => Line::from(vec![
                    Span::styled(sep.clone(), Style::default().fg(t.surface)),
                    Span::styled(" · ", Style::default().fg(t.muted)),
                    Span::styled(format!(" {ts} "), Style::default().fg(t.muted)),
                ]),
                "assistant" => Line::from(vec![
                    Span::styled(sep.clone(), Style::default().fg(t.surface)),
                    Span::styled(" DSX ", Style::default().fg(t.green).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {ts} "), Style::default().fg(t.muted)),
                ]),
                "error" => Line::from(vec![
                    Span::styled(sep.clone(), Style::default().fg(t.surface)),
                    Span::styled(" Error ", Style::default().fg(t.red).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {ts} "), Style::default().fg(t.muted)),
                ]),
                "tool" => Line::from(vec![
                    Span::styled(sep.clone(), Style::default().fg(t.surface)),
                    Span::styled(" Tool ", Style::default().fg(t.yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {ts} "), Style::default().fg(t.muted)),
                ]),
                _ => continue,
            };
            lines.push(Line::from(""));
            lines.push(header_line);

            if role.as_str() == "thinking" {
                for l in content.lines() {
                    for wrapped in wrap_line(l, inner_w) {
                        lines.push(Line::from(vec![
                            Span::styled(wrapped, Style::default().fg(t.subtext).add_modifier(Modifier::DIM)),
                        ]));
                    }
                }
            } else {
                let rendered = render_message_body(content, role, t, md_width);
                for line in rendered.lines {
                    lines.push(line.clone());
                }
            }
        }

        // Streaming content
        if self.is_streaming {
            if self.show_reasoning && !self.streaming_reasoning.is_empty() {
                lines.push(Line::from(""));
                for l in self.streaming_reasoning.lines() {
                    for wrapped in wrap_line(l, inner_w) {
                        lines.push(Line::from(vec![
                            Span::styled(wrapped, Style::default().fg(t.subtext).add_modifier(Modifier::DIM)),
                        ]));
                    }
                }
            }
            if !self.streaming_content.is_empty() {
                if self.show_reasoning && !self.streaming_reasoning.is_empty() {
                    lines.push(Line::from(""));
                }
                for l in self.streaming_content.lines() {
                    for wrapped in wrap_line(l, inner_w) {
                        lines.push(Line::from(vec![Span::styled(wrapped, Style::default().fg(t.green))]));
                    }
                }
            }
            // Animated spinner
            let idx = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| (d.as_millis() / 80) as usize % SPINNER.len()).unwrap_or(0);
            lines.push(Line::from(vec![Span::styled(SPINNER[idx], Style::default().fg(t.cyan))]));
        }

        let content_h = area.height.saturating_sub(2) as usize;
        let total = lines.len();
        let bottom_start = total.saturating_sub(content_h);
        let start = if self.auto_scroll {
            bottom_start
        } else {
            bottom_start.saturating_sub(self.scroll_offset)
        };
        let visible: Vec<Line> = lines.into_iter().skip(start).take(content_h).collect();

        f.render_widget(
            Paragraph::new(Text::from(visible))
                .block(Block::default().borders(Borders::ALL).title(" Chat ")
                    .border_style(Style::default().fg(t.blue))),
            area,
        );

        if total > content_h {
            let mut s = self.scroll_state.clone();
            s = s.content_length(total);
            s = s.position(start);
            f.render_stateful_widget(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight).thumb_symbol("┃"),
                Rect { x: area.right().saturating_sub(2), y: area.y + 1, width: 1, height: content_h as u16 },
                &mut s,
            );
        }
    }

    fn render_side(&self, f: &mut Frame, area: Rect, t: &Resolved) {
        let mut items: Vec<ListItem> = Vec::new();
        let bold = Style::default().add_modifier(Modifier::BOLD);

        let running: Vec<&ToolEntry> = self.tools.iter().filter(|e| e.running).collect();
        if !running.is_empty() {
            items.push(ListItem::new(Line::from(vec![Span::styled("● Running ", bold.fg(t.yellow))])));
            for e in &running {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("  ⠋ ", Style::default().fg(t.yellow)),
                    Span::styled(&e.name, Style::default().fg(t.text)),
                ])));
            }
            items.push(ListItem::new(""));
        }

        let done: Vec<&ToolEntry> = self.tools.iter().filter(|e| !e.running).collect();
        if !done.is_empty() {
            items.push(ListItem::new(Line::from(vec![Span::styled("✓ Completed ", bold.fg(t.green))])));
            for e in &done {
                let (icon, color) = if e.success { ("✓", t.green) } else { ("✗", t.red) };
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(format!("  {icon} "), Style::default().fg(color)),
                    Span::styled(&e.name, Style::default().fg(t.subtext)),
                ])));
            }
            items.push(ListItem::new(""));
        }

        if !self.files_written_this_turn.is_empty() {
            items.push(ListItem::new(Line::from(vec![
                Span::styled(format!("📄 Files ({})", self.files_written_this_turn.len()), bold.fg(t.blue)),
            ])));
            for f in &self.files_written_this_turn {
                let short = if f.len() > 35 { format!("...{}", &f[f.len().saturating_sub(32)..]) } else { f.clone() };
                items.push(ListItem::new(Line::from(vec![Span::styled(format!("  {short}"), Style::default().fg(t.subtext))])));
            }
        }

        let side_title = match (!self.tools.is_empty(), !self.files_written_this_turn.is_empty()) {
            (true, true) => " Tools & Files ",
            (true, false) => " Tools ",
            (false, true) => " Files ",
            _ => " Explorer ",
        };
        f.render_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(side_title)
                    .border_style(Style::default().fg(t.yellow))),
            area,
        );
    }

    // ── Footer (unified status + input) ──

    fn render_footer(&self, f: &mut Frame, area: Rect, t: &Resolved) {
        // Command menu popup (floating above footer)
        if let Some(ref menu) = self.command_menu {
            if !menu.filtered.is_empty() {
                let popup_h = (menu.filtered.len() as u16 + 2).min(12);
                let popup_y = if area.y > popup_h { area.y - popup_h } else { 2 };
                let popup = Rect {
                    x: area.x + 1,
                    y: popup_y,
                    width: (area.width - 2).min(55),
                    height: popup_h,
                };
                f.render_widget(Clear, popup);
                let title = if let Some((ref parent, _)) = menu.sub {
                    format!(" {} ▸ ", parent)
                } else {
                    " Commands ".into()
                };
                if menu.sub.is_some() {
                    let all_subs = match menu.sub {
                        Some((_, ref s)) => s.clone(),
                        _ => vec![],
                    };
                    let items: Vec<ListItem> = menu.filtered.iter().enumerate().map(|(i, &idx)| {
                        let opt = all_subs.get(idx).unwrap_or(&"?");
                        let style = if i == menu.cursor {
                            Style::default().fg(t.cyan).add_modifier(Modifier::BOLD)
                        } else { Style::default().fg(t.text) };
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {} ", if i == menu.cursor { "▶" } else { " " }), style),
                            Span::styled(*opt, style),
                        ]))
                    }).collect();
                    f.render_widget(
                        List::new(items).block(Block::default().borders(Borders::ALL).title(title)
                            .border_style(Style::default().fg(t.cyan))),
                        popup,
                    );
                } else {
                    let items: Vec<ListItem> = menu.filtered.iter().enumerate().map(|(i, &idx)| {
                        let (cmd, desc) = menu.items[idx];
                        let has_sub = !sub_options(cmd).is_empty();
                        let style = if i == menu.cursor {
                            Style::default().fg(t.cyan).add_modifier(Modifier::BOLD)
                        } else { Style::default().fg(t.text) };
                        let arrow = if has_sub { " ▸" } else { "" };
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {} ", if i == menu.cursor { "▶" } else { " " }), style),
                            Span::styled(format!("{cmd}{arrow}"), style),
                            Span::styled(format!("  {desc}"), Style::default().fg(t.subtext)),
                        ]))
                    }).collect();
                    f.render_widget(
                        List::new(items).block(Block::default().borders(Borders::ALL).title(title)
                            .border_style(Style::default().fg(t.cyan))),
                        popup,
                    );
                }
            }
        }

        // Footer layout: accent line + input + status row
        let chunks = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        // Accent line
        let accent = "─".repeat(area.width as usize);
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(accent, Style::default().fg(t.overlay))])),
            chunks[0],
        );

        // Input area with left border accent
        let input_chunks = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(chunks[1]);

        let border_color = if self.is_streaming { t.yellow } else { t.cyan };
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled("┃", Style::default().fg(border_color))])),
            input_chunks[0],
        );
        f.render_widget(&self.textarea, input_chunks[1]);

        // Status row
        let phase_display = match self.phase.as_str() {
            "plan" => "[Plan]", "debug" => "[Debug]", _ => "[Code]",
        };
        let ctx_pct = if self.context_limit > 0 {
            self.tokens_used as f64 / self.context_limit as f64
        } else { 0.0 };

        let spinner = if self.is_streaming {
            let idx = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| (d.as_millis() / 80) as usize % SPINNER.len()).unwrap_or(0);
            SPINNER[idx]
        } else { "" };

        let status_text = if !self.status.is_empty() { self.status.as_str() }
            else if self.is_streaming { "Processing" } else { "Ready" };

        let status_chunks = Layout::default().direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ])
            .split(chunks[2]);

        // Left: model + phase + spinner + status
        let left = Line::from(vec![
            Span::styled(format!("{} ", self.config.model.chars().take(14).collect::<String>()), Style::default().fg(t.blue)),
            Span::styled(phase_display, Style::default().fg(t.mauve)),
            Span::styled(" ", Style::default().fg(t.muted)),
            Span::styled(spinner, Style::default().fg(t.cyan)),
            Span::styled(format!(" {status_text}"), Style::default().fg(t.subtext)),
        ]);
        f.render_widget(Paragraph::new(left), status_chunks[0]);

        // Center-left: context gauge
        let ratio = ctx_pct.clamp(0.0, 1.0);
        let ctx_color = if ratio > 0.8 { t.red } else if ratio > 0.3 { t.yellow } else { t.green };
        let gauge = LineGauge::default()
            .ratio(ratio)
            .label(format!("ctx:{:.0}%", ratio * 100.0))
            .filled_style(Style::default().fg(ctx_color))
            .style(Style::default().fg(t.surface));
        f.render_widget(gauge, status_chunks[1]);

        // Center-right: cache + tokens
        let mid = Line::from(vec![
            Span::styled(format!("cache:{:.0}% ", self.cache_hit_pct * 100.0), Style::default().fg(t.subtext)),
            Span::styled(format!("{}tk", self.tokens_used), Style::default().fg(t.muted)),
        ]);
        f.render_widget(Paragraph::new(mid), status_chunks[2]);

        // Right: hint
        let hint = if let Some(ref q) = self.ask_question {
            format!("🤖 {q}")
        } else {
            "Ctrl+D exit".into()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(hint, Style::default().fg(t.muted))])).alignment(Alignment::Right),
            status_chunks[3],
        );
    }
}

// ── Helpers ──

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_w = (r.width * percent_x / 100).min(r.width.saturating_sub(4));
    let popup_x = r.x + (r.width.saturating_sub(popup_w)) / 2;
    let safe_h = height.min(r.height.saturating_sub(2));
    let popup_y = r.y + (r.height.saturating_sub(safe_h)) / 2;
    Rect { x: popup_x, y: popup_y, width: popup_w, height: safe_h }
}

fn now_ts() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let total_secs = secs + 8 * 3600; // UTC+8
    let h = (total_secs / 3600) % 24;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn hl_line_to_str(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Wrap a single line of text to `max_width` columns. Lines exceeding the
/// width are split into multiple strings; shorter lines are returned as-is.
fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 || line.is_empty() {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    for chunk in line.chars().collect::<Vec<char>>().chunks(max_width) {
        out.push(chunk.iter().collect::<String>());
    }
    out
}

fn mask_key(key: &str) -> String {
    if key.is_empty() { return "(empty)".into(); }
    if key.len() <= 8 { return "***".into(); }
    format!("{}...{}", &key[..4], &key[key.len()-4..])
}

fn detect_theme() -> Resolved {
    // Use catppuccin mocha by default.
    theme::DEFAULT
}

// ── HP daemon lifecycle ──

fn ensure_hp(exe: &std::path::Path) -> anyhow::Result<Option<Child>> {
    use dsx_types::platform::hp_port_path;
    let port_path = hp_port_path();
    if let Ok(port_str) = std::fs::read_to_string(&port_path) {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() { return Ok(None); }
        }
    }
    let _ = std::fs::write(&port_path, "");
    let mut hp = Command::new(exe).arg("hp").stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(s) = std::fs::read_to_string(&port_path) {
            if let Ok(p) = s.trim().parse::<u16>() {
                if TcpStream::connect(format!("127.0.0.1:{p}")).is_ok() { return Ok(Some(hp)); }
            }
        }
        if hp.try_wait()?.is_some() { break; }
    }
    let _ = hp.kill();
    anyhow::bail!("HP startup timeout. Run 'dsx config' to set API key.")
}

// ── Public entry point ──

pub fn run_tui(seed: Option<String>) -> anyhow::Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("dsx"));
    let hp_child = ensure_hp(&exe)?;
    let config = Config::load().unwrap_or_default();

    crate::dsx_log::init();
    crate::dsx_log::set_session(seed.as_deref().unwrap_or("tui"));

    let theme = detect_theme();

    let mut agent = AgentState::new(config.clone());
    agent.resume_seed = seed.clone();
    agent.health.context_limit = config.context_limit;

    // Init in-process tool manager
    crate::tools::init_tools(seed.as_deref().unwrap_or("tui"), agent.auto_mode);
    agent.tool_defs = crate::tools::all_tools();

    let hp_conn = crate::hp::connect().map(BufReader::new);

    let (tui_tx, tui_rx) = mpsc::channel::<TuiToAgent>();
    let (agent_tx, agent_rx) = mpsc::channel::<AgentToTui>();

    let agent_handle = std::thread::spawn(move || {
        crate::runner::run_agent_loop(agent, hp_conn, tui_rx, agent_tx);
    });

    let live_sessions = crate::session::find_live_sessions();
    let screen = if !config.is_ready() {
        Screen::Setup(SetupState {
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            effort_idx: match config.effort.as_deref() {
                Some("high") => 1, Some("max") => 2, _ => 0,
            },
            lang_idx: if config.prompt_lang == "zh" { 1 } else { 0 },
            focus: 0,
        })
    } else if !live_sessions.is_empty() && seed.is_none() {
        Screen::Resume(ResumeState { sessions: live_sessions, cursor: 0 })
    } else {
        Screen::Chat
    };

    let context_limit = config.context_limit;

    let app = App {
        screen,
        config,
        theme,
        messages: Vec::new(),
        streaming_content: String::new(),
        streaming_reasoning: String::new(),
        saved_reasoning: None,
        show_reasoning: true,
        tools: Vec::new(),
        phase: String::new(),
        cache_hit_pct: 0.0,
        tokens_used: 0,
        session_seed: seed.unwrap_or_default(),
        status: String::new(),
        context_limit,
        explored: false,
        files_read_this_turn: Vec::new(),
        files_written_this_turn: Vec::new(),
        ask_question: None,
        is_streaming: false,
        textarea: TextArea::default(),
        auto_scroll: true,
        scroll_offset: 0,
        scroll_state: ScrollbarState::default(),
        command_menu: None,
        agent_tx: tui_tx,
        agent_rx,
        exit: false,
    };

    let _ = crossterm::terminal::enable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
    let result = app.run();
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);

    agent_handle.join().ok();
    crate::tools::shutdown_tools();
    crate::hp::kill_hp_daemon();
    if let Some(mut c) = hp_child { let _ = c.kill(); let _ = c.wait(); }
    result
}
