//! DSX TUI — single-process terminal UI for the coding agent.
//!
//! Runs the ratatui event loop on the main thread and the agent turn loop
//! on a background thread. Communication via mpsc channels carrying
//! TuiToAgent / AgentToTui enums (zero serialization).

use std::io::BufReader;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use dsx_proto::{AgentToTui, TuiToAgent};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};

use crate::agent::AgentState;

// ── ToolEntry ──

struct ToolEntry {
    id: String,
    name: String,
    output: String,
    success: bool,
}

// ── App ──

struct App {
    messages: Vec<(String, String)>,
    streaming_content: String,
    streaming_reasoning: String,
    show_reasoning: bool,
    tools: Vec<ToolEntry>,
    phase: String,
    cache_hit_pct: f64,
    tokens_used: u32,
    session_seed: String,
    status: String,

    input: String,
    cursor: usize,
    scroll: usize,

    agent_tx: mpsc::Sender<TuiToAgent>,
    agent_rx: mpsc::Receiver<AgentToTui>,
    exit: bool,
}

// ── CJK-safe cursor helpers ──
// `cursor` is a byte offset into `input: String`.
// All operations must preserve the invariant that `cursor` is always
// at a UTF-8 character boundary (or == input.len()).

/// Move the byte cursor to the start of the previous character.
fn cursor_prev(s: &str, pos: usize) -> usize {
    if pos == 0 { return 0; }
    let mut p = pos - 1;
    while p > 0 && !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Move the byte cursor to the start of the next character.
fn cursor_next(s: &str, pos: usize) -> usize {
    if pos >= s.len() { return s.len(); }
    let mut p = pos + 1;
    while p < s.len() && !s.is_char_boundary(p) {
        p += 1;
    }
    p
}

/// Remove the character before the cursor (Backspace), returning true if anything was removed.
fn backspace_at(s: &mut String, cursor: &mut usize) -> bool {
    if *cursor == 0 { return false; }
    let prev = cursor_prev(s, *cursor);
    s.remove(prev);
    *cursor = prev;
    true
}

/// Remove the character after the cursor (Delete).
fn delete_at(s: &mut String, cursor: usize) -> bool {
    if cursor >= s.len() { return false; }
    // Remove one char — cursor is at a char boundary, s.remove(cursor) is safe
    s.remove(cursor);
    true
}

impl App {
    fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;

        loop {
            terminal.draw(|f| self.ui(f))?;

            if self.exit {
                break;
            }

            if crossterm::event::poll(Duration::from_millis(30))? {
                let ev = crossterm::event::read()?;
                match ev {
                    Event::Key(key) => {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }
                        match key.code {
                            KeyCode::Enter => {
                                let msg = std::mem::take(&mut self.input);
                                self.cursor = 0;
                                if !msg.is_empty() {
                                    self.messages.push(("user".into(), msg.clone()));
                                    let _ = self.agent_tx.send(TuiToAgent::UserInput { text: msg });
                                    self.status.clear();
                                    self.tools.clear();
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
                            KeyCode::Char(c) => {
                                self.input.insert(self.cursor, c);
                                self.cursor += c.len_utf8();
                            }
                            KeyCode::Backspace => {
                                backspace_at(&mut self.input, &mut self.cursor);
                            }
                            KeyCode::Delete => {
                                delete_at(&mut self.input, self.cursor);
                            }
                            KeyCode::Left => {
                                self.cursor = cursor_prev(&self.input, self.cursor);
                            }
                            KeyCode::Right => {
                                self.cursor = cursor_next(&self.input, self.cursor);
                            }
                            KeyCode::Home => {
                                self.cursor = 0;
                            }
                            KeyCode::End => {
                                self.cursor = self.input.len();
                            }
                            KeyCode::PageUp => {
                                self.scroll = self.scroll.saturating_add(10);
                            }
                            KeyCode::PageDown => {
                                self.scroll = self.scroll.saturating_sub(10);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            while let Ok(frame) = self.agent_rx.try_recv() {
                self.handle_frame(frame);
            }
        }

        terminal.clear()?;
        Ok(())
    }

    fn handle_frame(&mut self, frame: AgentToTui) {
        match frame {
            AgentToTui::ContentDelta { delta, reasoning } => {
                self.streaming_content.push_str(&delta);
                if let Some(r) = reasoning {
                    self.streaming_reasoning.push_str(&r);
                }
            }
            AgentToTui::ToolProgress { id, content, .. } => {
                if let Some(t) = self.tools.iter_mut().find(|t| t.id == id) {
                    t.output.push_str(&content);
                }
            }
            AgentToTui::ToolResult {
                id,
                name,
                content,
                success,
            } => {
                let icon = if success { "OK" } else { "ER" };
                let preview = content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>();
                self.messages.push((
                    "tool".into(),
                    format!("{} {} → {}", icon, name, preview),
                ));
                self.tools.push(ToolEntry {
                    id,
                    name,
                    output: content,
                    success,
                });
            }
            AgentToTui::ApiResponse { usage, .. } => {
                if let Some(u) = usage {
                    self.tokens_used = (u.prompt_tokens + u.completion_tokens) as u32;
                }
            }
            AgentToTui::PhaseChanged { phase } => {
                self.phase = phase;
            }
            AgentToTui::ToolState { .. } => {}
            AgentToTui::Error { message } => {
                self.messages.push(("error".into(), message));
            }
            AgentToTui::CachePrediction { hit_rate } => {
                self.cache_hit_pct = hit_rate;
            }
            AgentToTui::SessionRestored {
                seed,
                message_count,
                tokens_used,
                cache_hit_pct,
                ..
            } => {
                self.session_seed = seed;
                self.tokens_used = tokens_used;
                self.cache_hit_pct = cache_hit_pct;
                self.status = format!("Resumed: {} messages", message_count);
            }
            AgentToTui::Done => {
                if !self.streaming_content.is_empty() {
                    self.messages.push((
                        "assistant".into(),
                        std::mem::take(&mut self.streaming_content),
                    ));
                }
                self.streaming_reasoning.clear();
            }
            _ => {}
        }
    }

    fn ui(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(f.area());

        self.render_status(f, chunks[0]);
        self.render_main(f, chunks[1]);
        self.render_input(f, chunks[3]);
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let phase_display = match self.phase.as_str() {
            "plan" => "[Plan]",
            "debug" => "[Debug]",
            _ => "[Code]",
        };
        let text = format!(
            " DSX {} {} | Cache {:.0}% | {} tokens | {}",
            &self.session_seed.chars().take(8).collect::<String>(),
            phase_display,
            self.cache_hit_pct * 100.0,
            self.tokens_used,
            if self.status.is_empty() {
                "Ready"
            } else {
                &self.status
            },
        );
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::Gray)),
            area,
        );
    }

    fn render_main(&self, f: &mut Frame, area: Rect) {
        let has_tools = !self.tools.is_empty();
        let chunks = if has_tools {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100), Constraint::Percentage(0)])
                .split(area)
        };

        self.render_chat(f, chunks[0]);
        if has_tools {
            self.render_tools(f, chunks[1]);
        }
    }

    fn render_chat(&self, f: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        for (role, content) in &self.messages {
            let (prefix, style) = match role.as_str() {
                "user" => ("▸", Style::default().fg(Color::Cyan)),
                "error" => ("✗", Style::default().fg(Color::Red)),
                "tool" => ("⚙", Style::default().fg(Color::Yellow)),
                _ => ("", Style::default().fg(Color::White)),
            };
            for line in content.lines() {
                if role == "assistant" {
                    lines.push(Line::from(vec![Span::styled(
                        line,
                        Style::default().fg(Color::Green),
                    )]));
                } else if role == "tool" {
                    lines.push(Line::from(vec![Span::styled(
                        line,
                        Style::default().fg(Color::Yellow),
                    )]));
                } else {
                    lines.push(Line::from(vec![Span::styled(prefix, style), Span::raw(line)]));
                }
            }
            lines.push(Line::from(""));
        }

        let is_streaming =
            !self.streaming_content.is_empty() || !self.streaming_reasoning.is_empty();

        if self.show_reasoning && !self.streaming_reasoning.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "-- Reasoning --",
                Style::default().fg(Color::DarkGray),
            )]));
            for line in self.streaming_reasoning.lines() {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Gray),
                )]));
            }
            lines.push(Line::from(vec![Span::styled(
                "-- Response --",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        if !self.streaming_content.is_empty() {
            for line in self.streaming_content.lines() {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Green),
                )]));
            }
            if !self.show_reasoning && !self.streaming_reasoning.is_empty() {
                let preview: String = self
                    .streaming_reasoning
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(60)
                    .collect();
                lines.push(Line::from(vec![Span::styled(
                    format!("[Ctrl+R to see thinking: {}...]", preview),
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }

        if is_streaming {
            lines.push(Line::from(""));
        }

        let content_height = area.height.saturating_sub(2) as usize;
        let total_lines = lines.len();
        let start = self
            .scroll
            .min(total_lines.saturating_sub(content_height));
        let visible: Vec<Line> = lines.into_iter().skip(start).take(content_height).collect();

        f.render_widget(
            Paragraph::new(Text::from(visible))
                .block(Block::default().borders(Borders::ALL).title(" Chat ")),
            area,
        );
    }

    fn render_tools(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .tools
            .iter()
            .map(|t| {
                let icon = if t.success { "OK" } else { "ER" };
                let style = if t.success {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };
                ListItem::new(format!("[{}] {}", icon, t.name)).style(style)
            })
            .collect();

        f.render_widget(
            List::new(items).block(Block::default().borders(Borders::ALL).title(" Tools ")),
            area,
        );
    }

    fn render_input(&self, f: &mut Frame, area: Rect) {
        let display = if self.input.is_empty() {
            Span::styled(
                "Type a message (Ctrl+D exit, Ctrl+C cancel, Ctrl+R toggle thinking)...",
                Style::default().fg(Color::DarkGray),
            )
        } else {
            Span::raw(&self.input)
        };
        f.render_widget(
            Paragraph::new(Line::from(display))
                .block(Block::default().borders(Borders::ALL).title(" Input ")),
            area,
        );
    }
}

// ── HP daemon lifecycle ──

fn ensure_hp(exe: &std::path::Path) -> anyhow::Result<()> {
    use dsx_types::platform::hp_port_path;
    let port_path = hp_port_path();
    if let Ok(port_str) = std::fs::read_to_string(&port_path) {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                return Ok(());
            }
        }
    }
    let _ = std::fs::write(&port_path, "");
    let mut hp = Command::new(exe)
        .arg("hp")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(s) = std::fs::read_to_string(&port_path) {
            if let Ok(p) = s.trim().parse::<u16>() {
                if TcpStream::connect(format!("127.0.0.1:{p}")).is_ok() {
                    return Ok(());
                }
            }
        }
        if hp.try_wait()?.is_some() {
            break;
        }
    }
    anyhow::bail!("HP startup timeout. Run 'dsx config' to set API key.")
}

// ── Public entry point ──

pub fn run_tui(seed: Option<String>) -> anyhow::Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("dsx"));

    // 1. Ensure HP daemon running
    ensure_hp(&exe)?;

    // 2. Load config, create AgentState
    let config = crate::config::Config::load().unwrap_or_default();
    eprintln!(
        "dsx: model={} effort={:?} context_limit={}",
        config.model, config.effort, config.context_limit
    );

    let mut agent = AgentState::new(config);
    agent.resume_seed = seed.clone();
    agent.health.context_limit = agent.config.context_limit;

    // 3. Connect HP
    let hp_conn = crate::hp::connect().map(BufReader::new);

    // 4. Spawn tools subprocess and complete Init/Ready handshake
    let tools_child = init_tools(&exe, &mut agent);

    // 5. Create channels
    let (tui_tx, tui_rx) = mpsc::channel::<TuiToAgent>();
    let (agent_tx, agent_rx) = mpsc::channel::<AgentToTui>();

    // 6. Spawn agent background thread
    let _agent_handle = std::thread::spawn(move || {
        crate::runner::run_agent_loop(agent, hp_conn, tui_rx, agent_tx);
    });

    // 7. Create App and run TUI
    let app = App {
        messages: Vec::new(),
        streaming_content: String::new(),
        streaming_reasoning: String::new(),
        show_reasoning: true,
        tools: Vec::new(),
        phase: String::new(),
        cache_hit_pct: 0.0,
        tokens_used: 0,
        session_seed: seed.unwrap_or_default(),
        status: String::new(),
        input: String::new(),
        cursor: 0,
        scroll: 0,
        agent_tx: tui_tx,
        agent_rx,
        exit: false,
    };

    let _ = crossterm::terminal::enable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
    let result = app.run();
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);

    // Cleanup
    if let Some(mut c) = tools_child {
        let _ = c.kill();
        let _ = c.wait();
    }

    result
}

fn init_tools(exe: &std::path::Path, agent: &mut AgentState) -> Option<Child> {
    use dsx_proto::{AgentToTools, ToolsToAgent};

    let (child, mut reader, mut writer) = crate::tools_spawn::spawn_process(exe);

    let init = AgentToTools::Init {
        allowed_tools: vec![],
        session_seed: "pipe".into(),
        auto_mode: agent.auto_mode,
    };
    let _ = dsx_proto::write_frame(&mut writer, &init);

    let ready: Option<ToolsToAgent> = dsx_proto::read_frame(&mut reader).ok().flatten();
    if let Some(ToolsToAgent::Ready { tools }) = &ready {
        agent.tool_defs = tools.clone();
        eprintln!(
            "dsx: tools → {}",
            agent.tool_defs.len(),
        );
    }

    crate::tools::init_tools_ipc(reader, writer, agent.tool_defs.clone());
    Some(child)
}
