//! DSX TUI — single-process terminal UI for the coding agent.
//!
//! Threading: ratatui event loop on main thread, agent turn loop on background
//! thread. Communication via mpsc channels (zero serialization).

use std::io::BufReader;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use dsx_proto::{AgentToTui, TuiToAgent};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, LineGauge, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::{Frame, Terminal};
use tui_textarea::TextArea;

use crate::agent::AgentState;

// ── Semantic colors (catppuccin mocha) ──

mod theme {
    use ratatui::style::Color;
    pub const BG: Color = Color::from_u32(0x001e1e2e);
    pub const SURFACE: Color = Color::from_u32(0x00313244);
    pub const TEXT: Color = Color::from_u32(0x00cdd6f4);
    pub const SUBTEXT: Color = Color::from_u32(0x00a6adc8);
    pub const MUTED: Color = Color::from_u32(0x006c7086);
    pub const GREEN: Color = Color::from_u32(0x00a6e3a1);
    pub const RED: Color = Color::from_u32(0x00f38ba8);
    pub const YELLOW: Color = Color::from_u32(0x00f9e2af);
    pub const BLUE: Color = Color::from_u32(0x0089b4fa);
    pub const CYAN: Color = Color::from_u32(0x0089dceb);
    pub const MAUVE: Color = Color::from_u32(0x00cba6f7);
    #[allow(dead_code)]
    pub const PEACH: Color = Color::from_u32(0x00fab387);
}

// ── ToolEntry ──

struct ToolEntry {
    name: String,
    output: String,
    success: bool,
    running: bool,
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

    /// Model info (static, from config)
    model: String,
    context_limit: u32,

    /// New data — from previously ignored events
    explored: bool,
    files_read_this_turn: Vec<String>,
    files_written_this_turn: Vec<String>,
    ask_question: Option<String>,
    is_streaming: bool,

    /// tui-textarea replaces manual input + cursor
    textarea: TextArea<'static>,
    scroll: usize,
    scroll_state: ScrollbarState,

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
                                let lines: Vec<String> = self.textarea.lines().to_vec();
                                let msg = lines.join("\n").trim().to_string();
                                self.textarea = TextArea::default();
                                if !msg.is_empty() {
                                    self.messages.push(("user".into(), msg.clone()));
                                    let _ = self.agent_tx.send(TuiToAgent::UserInput { text: msg });
                                    self.status.clear();
                                    self.tools.clear();
                                    self.explored = false;
                                    self.files_read_this_turn.clear();
                                    self.files_written_this_turn.clear();
                                    self.ask_question = None;
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
                            KeyCode::PageUp => {
                                self.scroll = self.scroll.saturating_add(10);
                            }
                            KeyCode::PageDown => {
                                self.scroll = self.scroll.saturating_sub(10);
                            }
                            KeyCode::Esc => {
                                let _ = self.agent_tx.send(TuiToAgent::Cancel);
                            }
                            _ => {
                                // tui-textarea handles all other keys
                                self.textarea.input(ev);
                            }
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
                self.is_streaming = true;
                self.streaming_content.push_str(&delta);
                if let Some(r) = reasoning {
                    self.streaming_reasoning.push_str(&r);
                }
            }
            AgentToTui::ToolProgress { id: _, content, .. } => {
                if let Some(t) = self.tools.last_mut() {
                    t.output.push_str(&content);
                }
            }
            AgentToTui::ToolResult { id: _, name, content, success } => {
                let preview = content.lines().next().unwrap_or("")
                    .chars().take(80).collect::<String>();
                self.messages.push(("tool".into(),
                    format!("{} {}", if success { "OK" } else { "ER" }, preview)));
                // Mark the matching running tool as done
                for t in &mut self.tools {
                    if t.name == name && t.running {
                        t.output = content;
                        t.success = success;
                        t.running = false;
                        break;
                    }
                }
            }
            AgentToTui::ApiResponse { usage, stop_reason, .. } => {
                self.is_streaming = false;
                if let Some(u) = usage {
                    self.tokens_used = (u.prompt_tokens + u.completion_tokens) as u32;
                }
                if let Some(reason) = stop_reason {
                    if reason != "end_turn" && reason != "stop_sequence" {
                        self.status = reason;
                    }
                }
            }
            AgentToTui::PhaseChanged { phase } => {
                self.phase = phase;
            }
            AgentToTui::ToolState { explored, declared_files: _, read_files, written_this_turn } => {
                self.explored = explored;
                self.files_read_this_turn = read_files;
                self.files_written_this_turn = written_this_turn;
            }
            AgentToTui::Error { message } => {
                self.messages.push(("error".into(), message));
            }
            AgentToTui::CachePrediction { hit_rate } => {
                self.cache_hit_pct = hit_rate;
            }
            AgentToTui::SessionRestored { seed, message_count, tokens_used, cache_hit_pct, .. } => {
                self.session_seed = seed;
                self.tokens_used = tokens_used;
                self.cache_hit_pct = cache_hit_pct;
                self.status = format!("Resumed: {} messages", message_count);
            }
            AgentToTui::Done => {
                self.is_streaming = false;
                if !self.streaming_content.is_empty() {
                    self.messages.push(("assistant".into(),
                        std::mem::take(&mut self.streaming_content)));
                }
                self.streaming_reasoning.clear();
            }
            AgentToTui::AskUser { question, .. } => {
                self.ask_question = Some(question);
            }
            _ => {}
        }
    }

    fn ui(&mut self, f: &mut Frame) {
        let area = f.area();
        // Full-screen surface background
        f.render_widget(Paragraph::new("").bg(theme::BG), area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // status bar
                Constraint::Min(0),     // main area
                Constraint::Length(3),  // input
            ])
            .split(area);

        self.render_status(f, chunks[0]);
        self.render_main(f, chunks[1]);
        self.render_input(f, chunks[2]);
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let phase_display = match self.phase.as_str() {
            "plan" => "[Plan]", "debug" => "[Debug]", _ => "[Code]",
        };

        let ctx_pct = if self.context_limit > 0 {
            self.tokens_used as f64 / self.context_limit as f64
        } else { 0.0 };
        let ctx_color = if ctx_pct > 0.8 { theme::RED }
            else if ctx_pct > 0.3 { theme::YELLOW }
            else { theme::GREEN };

        let spinner = if self.is_streaming {
            const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let idx = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| (d.as_millis() / 80) as usize % FRAMES.len())
                .unwrap_or(0);
            FRAMES[idx]
        } else { "" };

        let status_text = if !self.status.is_empty() { self.status.as_str() }
            else if self.is_streaming { "Processing" }
            else { "Ready" };

        let left = Line::from(vec![
            Span::styled(format!(" {} ", self.model.chars().take(16).collect::<String>()),
                Style::default().fg(theme::BLUE)),
            Span::styled(format!("{phase_display} "), Style::default().fg(theme::MAUVE)),
            Span::styled(spinner, Style::default().fg(theme::CYAN)),
            Span::styled(format!("{status_text} "), Style::default().fg(theme::SUBTEXT)),
        ]);

        let right = Line::from(vec![
            Span::styled(format!("cache:{:.0}% ", self.cache_hit_pct * 100.0),
                Style::default().fg(theme::SUBTEXT)),
            Span::styled(format!("{}tk", self.tokens_used),
                Style::default().fg(theme::MUTED)),
        ]);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Percentage(20),
                Constraint::Percentage(35),
            ])
            .split(area);

        f.render_widget(Paragraph::new(left), chunks[0]);

        // Context pressure LineGauge
        let gauge = LineGauge::default()
            .ratio(ctx_pct.clamp(0.0, 1.0))
            .label(format!("ctx:{:.0}%", ctx_pct * 100.0))
            .filled_style(Style::default().fg(ctx_color))
            .style(Style::default().fg(theme::SURFACE));
        f.render_widget(gauge, chunks[1]);

        f.render_widget(Paragraph::new(right).alignment(ratatui::layout::Alignment::Right), chunks[2]);
    }

    fn render_main(&self, f: &mut Frame, area: Rect) {
        let has_side = !self.tools.is_empty() || !self.files_written_this_turn.is_empty()
            || self.explored;

        let chunks = if has_side {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100), Constraint::Percentage(0)])
                .split(area)
        };

        self.render_chat(f, chunks[0]);
        if has_side {
            self.render_side(f, chunks[1]);
        }
    }

    fn render_chat(&self, f: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        for (role, content) in &self.messages {
            let (prefix, style) = match role.as_str() {
                "user" => ("▸ ", Style::default().fg(theme::BLUE)),
                "error" => ("✗ ", Style::default().fg(theme::RED)),
                "tool" => ("⚙ ", Style::default().fg(theme::MUTED)),
                _ => ("", Style::default()),
            };
            let body_style = match role.as_str() {
                "assistant" => Style::default().fg(theme::GREEN),
                "tool" => Style::default().fg(theme::SUBTEXT),
                _ => Style::default().fg(theme::TEXT),
            };
            for line in content.lines() {
                if role == "assistant" || role == "tool" {
                    lines.push(Line::from(vec![Span::styled(line, body_style)]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(line, body_style),
                    ]));
                }
            }
            lines.push(Line::from(""));
        }

        if self.show_reasoning && !self.streaming_reasoning.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "── Thinking ──", Style::default().fg(theme::MAUVE),
            )]));
            for l in self.streaming_reasoning.lines() {
                lines.push(Line::from(vec![Span::styled(l, Style::default().fg(theme::SUBTEXT))]));
            }
            lines.push(Line::from(vec![Span::styled(
                "── Response ──", Style::default().fg(theme::MAUVE),
            )]));
        }

        if !self.streaming_content.is_empty() {
            for l in self.streaming_content.lines() {
                lines.push(Line::from(vec![Span::styled(l, Style::default().fg(theme::GREEN))]));
            }
            if !self.show_reasoning && !self.streaming_reasoning.is_empty() {
                let preview: String = self.streaming_reasoning.lines().next()
                    .unwrap_or("").chars().take(60).collect();
                lines.push(Line::from(vec![Span::styled(
                    format!("Ctrl+R: {preview}..."), Style::default().fg(theme::MUTED),
                )]));
            }
        }

        if self.is_streaming {
            lines.push(Line::from(vec![Span::styled("⏳", Style::default().fg(theme::CYAN))]));
        }

        let content_h = area.height.saturating_sub(2) as usize;
        let total = lines.len();
        let start = self.scroll.min(total.saturating_sub(content_h));
        let visible: Vec<Line> = lines.into_iter().skip(start).take(content_h).collect();

        f.render_widget(
            Paragraph::new(Text::from(visible))
                .block(Block::default().borders(Borders::ALL).title(" Chat ")
                    .border_style(Style::default().fg(theme::SURFACE))),
            area,
        );

        // Scrollbar
        if total > content_h {
            let mut s = self.scroll_state.clone();
            s = s.content_length(total);
            s = s.position(start);
            let scroll_area = Rect {
                x: area.right().saturating_sub(1),
                y: area.y + 1,
                width: 1,
                height: content_h as u16,
            };
            f.render_stateful_widget(
                Scrollbar::default()
                    .orientation(ScrollbarOrientation::VerticalRight)
                    .thumb_symbol("┃"),
                scroll_area,
                &mut s,
            );
        }
    }

    fn render_side(&self, f: &mut Frame, area: Rect) {
        let mut items: Vec<ListItem> = Vec::new();

        // Tool results
        for t in &self.tools {
            let style = if t.running {
                Style::default().fg(theme::YELLOW)
            } else if t.success {
                Style::default().fg(theme::GREEN)
            } else {
                Style::default().fg(theme::RED)
            };
            let marker = if t.running { "●" } else if t.success { "✓" } else { "✗" };
            items.push(ListItem::new(format!("{marker} {name}", name = t.name)).style(style));
        }

        // Files written this turn
        if !self.files_written_this_turn.is_empty() {
            items.push(ListItem::new("── Files ──").style(Style::default().fg(theme::MUTED)));
            for f in &self.files_written_this_turn {
                items.push(ListItem::new(format!("  {}", f)));
            }
        }

        if self.explored {
            items.push(ListItem::new("✓ explored").style(Style::default().fg(theme::GREEN)));
        }

        f.render_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Tools ")
                    .border_style(Style::default().fg(theme::SURFACE))),
            area,
        );
    }

    fn render_input(&self, f: &mut Frame, area: Rect) {
        let hint = if let Some(ref q) = self.ask_question {
            format!("🤖 {}  |  Enter to respond, Esc cancel", q)
        } else {
            "Ctrl+D exit | Ctrl+C cancel | Ctrl+R thinking | Esc cancel".into()
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        f.render_widget(&self.textarea, chunks[0]);
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(hint, Style::default().fg(theme::MUTED))])),
            chunks[1],
        );
    }
}

// ── HP daemon lifecycle ──

fn ensure_hp(exe: &std::path::Path) -> anyhow::Result<Option<Child>> {
    use dsx_types::platform::hp_port_path;
    let port_path = hp_port_path();
    if let Ok(port_str) = std::fs::read_to_string(&port_path) {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                return Ok(None); // already running, not our child
            }
        }
    }
    let _ = std::fs::write(&port_path, "");
    let mut hp = Command::new(exe).arg("hp")
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(s) = std::fs::read_to_string(&port_path) {
            if let Ok(p) = s.trim().parse::<u16>() {
                if TcpStream::connect(format!("127.0.0.1:{p}")).is_ok() {
                    return Ok(Some(hp));
                }
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

    let config = crate::config::Config::load().unwrap_or_default();
    eprintln!("dsx: model={} effort={:?} context_limit={}",
        config.model, config.effort, config.context_limit);

    let mut agent = AgentState::new(config.clone());
    agent.resume_seed = seed.clone();
    agent.health.context_limit = agent.config.context_limit;

    let hp_conn = crate::hp::connect().map(BufReader::new);
    let tools_child = init_tools(&exe, &mut agent);

    let (tui_tx, tui_rx) = mpsc::channel::<TuiToAgent>();
    let (agent_tx, agent_rx) = mpsc::channel::<AgentToTui>();

    let _agent_handle = std::thread::spawn(move || {
        crate::runner::run_agent_loop(agent, hp_conn, tui_rx, agent_tx);
    });

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
        model: config.model,
        context_limit: config.context_limit,
        explored: false,
        files_read_this_turn: Vec::new(),
        files_written_this_turn: Vec::new(),
        ask_question: None,
        is_streaming: false,
        textarea: TextArea::default(),
        scroll: 0,
        scroll_state: ScrollbarState::default(),
        agent_tx: tui_tx,
        agent_rx,
        exit: false,
    };

    let _ = crossterm::terminal::enable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
    let result = app.run();
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);

    // Cleanup: kill all spawned subprocesses
    if let Some(mut c) = tools_child {
        let _ = c.kill();
        let _ = c.wait();
    }
    if let Some(mut c) = hp_child {
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
        eprintln!("dsx: tools → {}", agent.tool_defs.len());
    }

    crate::tools::init_tools_ipc(reader, writer, agent.tool_defs.clone());
    Some(child)
}
