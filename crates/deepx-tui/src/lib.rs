//! DeepX TUI — terminal frontend for the deepx agent.
//!
//! Runs the agent as a child process communicating over stdin/stdout.
//! Falls back to setup wizard if no config file exists.

mod app;
mod i18n;
mod markdown;
mod ui;

use app::{App, Screen};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::event::EnableBracketedPaste;
use deepx_proto::{Agent2Ui, Ui2Agent};
use deepx_types::{ConfigStore, SessionMeta};
use ratatui::DefaultTerminal;
use std::sync::mpsc;

/// Spawn the agent as a child process, communicating over stdin/stdout via bridge threads.
fn spawn_agent_subprocess(
    resume_seed: Option<&str>,
) -> Result<(mpsc::Sender<Ui2Agent>, mpsc::Receiver<Agent2Ui>, std::process::Child), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let exe = std::env::current_exe().map_err(|e| format!("exe path: {e}"))?;
    let mut cmd = Command::new(&exe);
    cmd.arg("agent");
    if let Some(seed) = resume_seed {
        cmd.arg("--resume-seed").arg(seed);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn agent subprocess: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;

    let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    // Writer thread: mpsc → child stdin (Ui2Agent → JSON lines)
    std::thread::spawn(move || {
        while let Ok(frame) = tui_rx.recv() {
            let json = match serde_json::to_string(&frame) {
                Ok(j) => j,
                Err(_) => break,
            };
            if writeln!(stdin, "{}", json).is_err() {
                break;
            }
            if stdin.flush().is_err() {
                break;
            }
        }
    });

    // Reader thread: child stdout → mpsc (JSON lines → Agent2Ui)
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Ok(event) = serde_json::from_str::<Agent2Ui>(&line) {
                if agent_tx.send(event).is_err() {
                    break;
                }
            }
        }
    });

    Ok((tui_tx, agent_rx, child))
}

/// Entry point for `deepx --tui`. Shows setup wizard if needed, then the session
/// selection screen, then spawns the agent subprocess and runs the chat loop.
pub fn run_tui() -> anyhow::Result<()> {
    deepx_session::SessionManager::init(deepx_types::platform::data_dir());
    let store = ConfigStore::default_location();
    let need_setup = !store.exists()
        || store.load_api_key().map_or(true, |k| k.is_empty());

    let mut app = App::new(need_setup);
    if !need_setup {
        app.setup.fill_from_store(&store);
    }

    let result = ratatui::run(|terminal| {
        crossterm::execute!(std::io::stdout(), EnableBracketedPaste).ok();
        if need_setup {
            run_setup(terminal, &mut app, &store)?;
        }
        load_sessions(&mut app);
        if !app.sessions.is_empty() {
            run_session_screen(terminal, &mut app)?;
        }
        if app.should_quit { return Ok(()); }
        app.scroll_offset = 0;
        app.status = app.setup.lang.t_chat_ready().to_string();
        match spawn_agent_subprocess(app.resume_seed.as_deref()) {
            Ok((mut tui_tx, agent_rx, mut child_handle)) => {
                if app.resume_seed.is_none() {
                    // Wait for Ready frame before sending CreateSession
                    loop {
                        match agent_rx.recv() {
                            Ok(Agent2Ui::Ready) => break,
                            Ok(_) => continue,
                            Err(_) => {
                                app.status = "Agent died before ready".into();
                                break;
                            }
                        }
                    }
                    let _ = tui_tx.send(Ui2Agent::CreateSession);
                }

                let send = |tx: &mut mpsc::Sender<Ui2Agent>, frame: &Ui2Agent| {
                    let _ = tx.send(frame.clone());
                };
                let result = run_chat(terminal, &mut app, &mut tui_tx, &agent_rx, send);
                drop(tui_tx);
                drop(agent_rx);
                let _ = child_handle.wait();
                result
            }
            Err(e) => {
                app.status = format!("Agent spawn failed: {e}");
                Ok(())
            }
        }
    });

    Ok(result?)
}


fn load_sessions(app: &mut App) {
    use std::fs;
    let dir = deepx_types::platform::sessions_dir();
    let index_path = dir.join("index.toml");
    if let Ok(data) = fs::read_to_string(&index_path) {
        let metas: Option<Vec<SessionMeta>> = toml::from_str(&data).ok()
            .or_else(|| serde_json::from_str(&data).ok());
        if let Some(mut metas) = metas {
            metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
            app.sessions = metas;
        }
    }
}

// ── Setup wizard loop ──

fn run_setup(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    store: &ConfigStore,
) -> std::io::Result<()> {
    app.setup.fill_from_store(store);

    loop {
        terminal.draw(|frame| ui::render_setup(frame, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press { continue; }

            match (key.modifiers, key.code) {
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),

                // Step 0: language selection via arrow keys
                (_, KeyCode::Up) | (_, KeyCode::Down) if app.setup.step == 0 => {
                    app.setup.toggle_lang();
                }

                // Step 2: model selection via arrow keys
                (_, KeyCode::Up) if app.setup.step == 2 && app.setup.models_loaded => {
                    let len = app.setup.model_list.len();
                    if len > 0 {
                        app.setup.model_index = app.setup.model_index.checked_sub(1).unwrap_or(len - 1);
                        app.setup.model = app.setup.model_list[app.setup.model_index].clone();
                    }
                }
                (_, KeyCode::Down) if app.setup.step == 2 && app.setup.models_loaded => {
                    let len = app.setup.model_list.len();
                    if len > 0 {
                        app.setup.model_index = (app.setup.model_index + 1) % len;
                        app.setup.model = app.setup.model_list[app.setup.model_index].clone();
                    }
                }

                (_, KeyCode::Enter) => {
                    // Step 1: validate API key before advancing
                    if app.setup.step == 1 && !app.setup.api_key.trim().is_empty() {
                        let l = app.setup.lang;
                        app.setup.status = l.t_validating().to_string();
                        app.validating = true;
                        terminal.draw(|frame| ui::render_setup(frame, app))?;

                        let (pid, _) = deepx_config::registry::first_provider_endpoint();
                        let ok = app.setup.fetch_models(&pid);
                        app.validating = false;
                        if ok {
                            app.setup.status = l.t_key_valid().to_string();
                            app.setup.error.clear();
                        } else {
                            app.setup.status.clear();
                            app.setup.error = l.t_key_invalid().to_string();
                            continue;
                        }
                    }

                    if app.setup.next() {
                        let pc = app.setup.to_persistent_config();
                        store.save(&pc);
                        app.screen = Screen::Chat;
                        app.status = format!(
                            "{} {}",
                            app.setup.lang.t_config_saved(),
                            deepx_types::platform::config_path().display()
                        );
                        return Ok(());
                    }
                }

                (_, KeyCode::Backspace) => app.setup.backspace(),
                (_, KeyCode::Esc) => app.setup.clear_field(),
                (_, KeyCode::Char(c)) => app.setup.type_char(c),
                _ => {}
            }
        }
    }
}

// ── Session selection screen ──

fn run_session_screen(
    terminal: &mut DefaultTerminal,
    app: &mut App,
) -> std::io::Result<()> {
    app.screen = Screen::Session;
    loop {
        terminal.draw(|frame| ui::render_sessions(frame, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press { continue; }

            match (key.modifiers, key.code) {
                (KeyModifiers::CONTROL, KeyCode::Char('c'))
                | (KeyModifiers::NONE, KeyCode::Char('q'))
                | (_, KeyCode::F(3)) => {
                    app.should_quit = true;
                    return Ok(());
                }
                (_, KeyCode::Up) => {
                    if app.session_index > 0 {
                        app.session_index -= 1;
                    }
                }
                (_, KeyCode::Down) => {
                    if app.session_index < app.sessions.len() {
                        app.session_index += 1;
                    }
                }
                (_, KeyCode::Enter) => {
                    let total = app.sessions.len();
                    if app.session_index == total {
                        // "New Session" selected
                        break;
                    } else if app.session_index < total {
                        app.resume_seed = Some(app.sessions[app.session_index].seed.clone());
                        break;
                    }
                }
                (_, KeyCode::Delete) => {
                    let total = app.sessions.len();
                    if app.session_index < total {
                        let seed = app.sessions[app.session_index].seed.clone();
                        let _ = deepx_session::SessionManager::global().delete(&seed);
                        load_sessions(app);
                        if app.resume_seed.as_deref() == Some(&seed) {
                            app.resume_seed = None;
                        }
                        if app.session_index >= app.sessions.len() {
                            app.session_index = app.sessions.len().saturating_sub(1);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    app.screen = Screen::Chat;
    app.scroll_offset = 0;
    Ok(())
}

// ── Chat loop ──

fn run_chat(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    tui_tx: &mut mpsc::Sender<Ui2Agent>,
    agent_rx: &mpsc::Receiver<Agent2Ui>,
    send: impl Fn(&mut mpsc::Sender<Ui2Agent>, &Ui2Agent),
) -> std::io::Result<()> {
    use std::time::{Duration, Instant};
    let mut agent_dead = false;

    // ── Event loop: poll keyboard with timeout, drain agent frames, render ──
    // Avoids spin-wait: poll timeout scales with streaming urgency.
    // Keyboard events are checked at ~30Hz idle, ~15Hz while streaming.
    // Agent frames arrive via mpsc and are drained non-blocking each iteration.
    loop {
        // 1. Keyboard: poll with timeout (streaming → faster poll for snappy cancel)
        let poll_timeout = if app.streaming {
            Duration::from_millis(66) // ~15 Hz — enough for cancel responsiveness
        } else if agent_dead {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(100) // ~10 Hz idle — terminals don't need more
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Resize(_, _) => {
                    // ratatui Terminal auto-resizes on next draw — nothing to do here
                }
                Event::Paste(data) => {
                    let text = data.trim_end_matches(|c: char| c == '\n' || c == '\r');
                    app.input.insert_str(app.cursor, text);
                    app.cursor += text.len();
                }
                Event::Key(key) => {
            if key.kind != KeyEventKind::Press { continue; }

            // Ask popup: intercept keys
            if app.ask.is_some() {
                let ask = app.ask.as_mut().unwrap();
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc) => { app.ask = None; }
                    (_, KeyCode::Up) => { if ask.selected > 0 { ask.selected -= 1; } }
                    (_, KeyCode::Down) => { if ask.selected + 1 < ask.options.len() { ask.selected += 1; } }
                    (_, KeyCode::Enter) => {
                        let reply = if ask.selected < ask.options.len() {
                            let opt = &ask.options[ask.selected];
                            if opt.is_empty() {
                                if ask.custom_input.is_empty() { continue; }
                                ask.custom_input.clone()
                            } else {
                                opt.clone()
                            }
                        } else { continue };
                        if !reply.is_empty() {
                            send(tui_tx, &deepx_proto::Ui2Agent::UserInput { text: reply });
                        }
                        app.ask = None;
                    }
                    (_, KeyCode::Backspace) => { ask.custom_input.pop(); }
                    (_, KeyCode::Char(c)) => { ask.custom_input.push(c); }
                    _ => {}
                }
                continue;
            }

            // Help overlay
            if app.show_help {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Char('?')) | (_, KeyCode::Esc) => { app.show_help = false; }
                    _ => {}
                }
                continue;
            }

            // Normal chat keys
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('?')) => {
                    app.show_help = !app.show_help;
                    app.scroll_offset = 0;
                }
                (KeyModifiers::NONE, KeyCode::F(6)) => {
                    app.show_thinking = !app.show_thinking;
                    app.scroll_offset = 0;
                }
                (_, KeyCode::F(10)) => {
                    if agent_dead { continue; }
                    let menu = crate::app::MenuState::new(app);
                    run_menu(terminal, app, menu)?;
                    send(tui_tx, &deepx_proto::Ui2Agent::ReloadConfig);
                }
                (KeyModifiers::NONE, KeyCode::F(12)) => {
                    app.show_debug = !app.show_debug;
                }
                (KeyModifiers::NONE, KeyCode::F(9)) => {
                    app.show_tasks = !app.show_tasks;
                }
                (KeyModifiers::NONE, KeyCode::F(8)) => {
                    app.show_context = !app.show_context;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c'))
                | (_, KeyCode::F(3)) => return Ok(()),
                (_, KeyCode::Esc) => {
                    if !agent_dead {
                        send(tui_tx, &deepx_proto::Ui2Agent::Cancel);
                    }
                    app.status = app.setup.lang.t_chat_cancelled().to_string();
                }
                (KeyModifiers::CONTROL, KeyCode::Enter) => {
                    app.input.insert(app.cursor, '\n');
                    app.cursor += 1;
                }
                (_, KeyCode::Enter) => {
                    if agent_dead || app.busy { continue; }
                    let text = app.input.drain(..).collect::<String>();
                    app.cursor = 0;
                    app.history_idx = None;
                    app.draft_input.clear();
                    if !text.trim().is_empty() {
                        // Dedup: don't push identical consecutive entries
                        if app.input_history.last().map_or(true, |last| last != &text) {
                            app.input_history.push(text.clone());
                        }
                        // Cap history to 200 entries
                        if app.input_history.len() > 200 {
                            app.input_history.remove(0);
                        }
                        app.status = app.setup.lang.t_chat_thinking().to_string();
                        app.busy = true;
                        send(tui_tx, &deepx_proto::Ui2Agent::UserInput { text });
                    }
                }
                (_, KeyCode::Backspace) => {
                    if app.cursor > 0 {
                        if let Some((idx, _)) = app.input[..app.cursor].char_indices().rev().next() {
                            app.input.remove(idx);
                            app.cursor = idx;
                        }
                    }
                }
                (_, KeyCode::Delete) => {
                    if app.cursor < app.input.len() {
                        if let Some((_, _)) = app.input[app.cursor..].char_indices().next() {
                            app.input.remove(app.cursor);
                        }
                    }
                }
                (_, KeyCode::Left) => {
                    if app.cursor > 0 {
                        if let Some((idx, _)) = app.input[..app.cursor].char_indices().rev().next() {
                            app.cursor = idx;
                        } else {
                            app.cursor = 0;
                        }
                    }
                }
                (_, KeyCode::Right) => {
                    if app.cursor < app.input.len() {
                        if let Some((idx, _)) = app.input[app.cursor..].char_indices().nth(1) {
                            app.cursor = app.cursor + idx;
                        } else {
                            app.cursor = app.input.len();
                        }
                    }
                }
                (_, KeyCode::Home) => { app.cursor = 0; }
                (_, KeyCode::End) => { app.cursor = app.input.len(); }
                (_, KeyCode::Char(c)) => {
                    // Typing exits history browse mode
                    if app.history_idx.is_some() {
                        app.history_idx = None;
                        app.draft_input.clear();
                    }
                    app.input.insert(app.cursor, c);
                    app.cursor += c.len_utf8();
                }
                (_, KeyCode::Up) => {
                    // If cursor is on the first line of input, browse history
                    let cursor_line = app.input[..app.cursor].chars().filter(|&c| c == '\n').count();
                    if cursor_line == 0 && !app.input_history.is_empty() {
                        if app.history_idx.is_none() {
                            app.draft_input = app.input.clone();
                            app.history_idx = Some(app.input_history.len() - 1);
                        } else if let Some(idx) = app.history_idx {
                            if idx > 0 { app.history_idx = Some(idx - 1); }
                        }
                        if let Some(idx) = app.history_idx {
                            app.input = app.input_history[idx].clone();
                            app.cursor = app.input.len();
                            app.cached_input_len = 0; // invalidate cache
                        }
                    } else {
                        app.scroll_offset = app.scroll_offset.saturating_add(1);
                    }
                }
                (_, KeyCode::PageUp) => {
                    app.scroll_offset = app.scroll_offset.saturating_add(10);
                }
                (_, KeyCode::Down) => {
                    if app.history_idx.is_some() {
                        if let Some(idx) = app.history_idx {
                            if idx + 1 < app.input_history.len() {
                                app.history_idx = Some(idx + 1);
                                app.input = app.input_history[idx + 1].clone();
                            } else {
                                // Past the last history entry — restore draft
                                app.history_idx = None;
                                app.input = app.draft_input.clone();
                                app.draft_input.clear();
                            }
                            app.cursor = app.input.len();
                            app.cached_input_len = 0;
                        }
                    } else {
                        app.scroll_offset = app.scroll_offset.saturating_sub(1);
                    }
                }
                (_, KeyCode::PageDown) => {
                    app.scroll_offset = app.scroll_offset.saturating_sub(10);
                }
                _ => {}
            }
            } // Event::Key
            _ => {}
            } // match event
        }

        // 2. Drain agent frames (non-blocking)
        loop {
            match agent_rx.try_recv() {
                Ok(frame) => app.handle_frame(frame),
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !agent_dead {
                        agent_dead = true;
                        let l = app.setup.lang;
                        app.push_msg(app::ChatRole::Status,
                            if l.as_str() == "zh" { "Agent 进程已断开，请按 F3 退出" }
                            else { "Agent disconnected — press F3 to quit" });
                        app.status = if l.as_str() == "zh" { "Agent 已断开" } else { "Agent disconnected" }.into();
                        app.streaming = false;
                    }
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        if agent_dead && app.should_quit { return Ok(()); }

        // 3. Render (ratatui internally diffs — cheap when nothing changed)
        let now = Instant::now();
        let render_interval = if app.streaming {
            Duration::from_millis(33) // ~30 FPS streaming
        } else {
            Duration::from_millis(100) // ~10 FPS idle — terminals are low-bandwidth
        };
        if now.duration_since(app.last_render) >= render_interval {
            terminal.draw(|frame| {
                ui::render_chat(frame, app);
                if app.ask.is_some() { ui::render_ask(frame, app); }
                if app.show_help { ui::render_help(frame, app); }
            })?;
            app.last_render = now;
        }
        app.tick();

        if app.should_quit { return Ok(()); }
    }
}

// ── Menu screen ──

fn run_menu(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mut menu: crate::app::MenuState,
) -> std::io::Result<()> {
    app.screen = app::Screen::Menu;
    loop {
        terminal.draw(|frame| ui::render_menu(frame, &menu))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press { continue; }

            match (key.modifiers, key.code) {
                (_, KeyCode::F(10))
                | (_, KeyCode::Esc) => {
                    menu.go_back(app);
                    return Ok(());
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                    app.should_quit = true;
                    return Ok(());
                }
                (_, KeyCode::Up) => {
                    if menu.selected > 0 {
                        menu.selected -= 1;
                        menu.editing = false;
                        menu.edit_buf.clear();
                        menu.status.clear();
                    }
                }
                (_, KeyCode::Down) => {
                    if menu.selected + 1 < menu.items.len() {
                        menu.selected += 1;
                        menu.editing = false;
                        menu.edit_buf.clear();
                        menu.status.clear();
                    }
                }
                (_, KeyCode::Enter) => {
                    let item = match menu.items.get(menu.selected) {
                        Some(i) => i,
                        None => continue,
                    };
                    if menu.editing {
                        if !menu.edit_buf.is_empty() {
                            let item = &mut menu.items[menu.selected];
                            item.value = menu.edit_buf.clone();
                            if item.key == "api_key" {
                                item.secret = menu.edit_buf.clone();
                            }
                        }
                        menu.editing = false;
                        menu.edit_buf.clear();
                        menu.save_all();
                    } else if item.editable && item.kind == crate::app::MenuItemKind::Toggle {
                        menu.toggle(app);
                        menu.save_all();
                    } else if item.editable && item.kind == crate::app::MenuItemKind::Value {
                        menu.editing = true;
                        menu.edit_buf.clear();
                    } else if item.kind == crate::app::MenuItemKind::Action {
                        menu.toggle(app);
                        menu.save_all();
                    }
                }
                (_, KeyCode::Backspace) => {
                    if menu.editing {
                        menu.edit_buf.pop();
                    }
                }
                (_, KeyCode::Char(c)) => {
                    if menu.editing {
                        menu.edit_buf.push(c);
                    }
                }
                _ => {}
            }
        }
    }
}
