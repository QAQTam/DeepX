//! DeepX TUI — terminal frontend for the dsx agent.
//!
//! Spawns `dsx agent` as a child process, communicates via stdin/stdout
//! JSON-LP protocol (Ui2Agent / Agent2Ui), renders a chat-like interface.
//! Falls back to setup wizard if no config file exists.

mod app;
mod i18n;
mod markdown;
mod ui;

use app::{App, Screen};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use dsx_proto::Agent2Ui;
use dsx_types::{ConfigStore, SessionMeta};
use ratatui::DefaultTerminal;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;

fn find_dsx_binary() -> String {
    // 1. Same directory as dsx-tui (cargo build workspace)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(if cfg!(windows) { "dsx.exe" } else { "dsx" });
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    // 2. Fallback: assume on PATH
    "dsx".to_string()
}

fn spawn_agent(resume_seed: Option<&str>) -> anyhow::Result<(Child, ChildStdin, mpsc::Receiver<Agent2Ui>, thread::JoinHandle<()>)> {
    let dsx = find_dsx_binary();
    let mut cmd = Command::new(&dsx);
    cmd.arg("agent");
    if let Some(seed) = resume_seed {
        cmd.arg("--session").arg(seed);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Ok(frame) = serde_json::from_str::<Agent2Ui>(&line) {
                if agent_tx.send(frame).is_err() { break; }
            }
        }
    });

    Ok((child, stdin, agent_rx, stdout_handle))
}

fn send_to_agent(stdin: &mut ChildStdin, frame: &dsx_proto::Ui2Agent) {
    let json = serde_json::to_string(frame).unwrap_or_default();
    let _ = writeln!(stdin, "{}", json);
    let _ = stdin.flush();
}

fn main() -> anyhow::Result<()> {
    let store = ConfigStore::default_location();
    let need_setup = !store.exists()
        || store.load_api_key().map_or(true, |k| k.is_empty());

    let mut app = App::new(need_setup);
    if !need_setup {
        app.setup.fill_from_store(&store);
    }

    let result = ratatui::run(|terminal| {
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
        match spawn_agent(app.resume_seed.as_deref()) {
            Ok((mut child, mut stdin, agent_rx, stdout_handle)) => {
                let result = run_chat(terminal, &mut app, &mut stdin, &agent_rx,
                    |stdin, frame| send_to_agent(stdin, frame));
                send_to_agent(&mut stdin, &dsx_proto::Ui2Agent::Shutdown);
                let _ = child.wait();
                stdout_handle.join().ok();
                result
            }
            Err(e) => {
                let msg = format!("{}: {}", app.setup.lang.t_failed_agent(), e);
                app.status = app.setup.lang.t_failed_agent().to_string();
                app.push_msg(app::ChatRole::Status, &msg);
                Ok(())
            }
        }
    });

    Ok(result?)
}

fn load_sessions(app: &mut App) {
    use std::fs;
    let dir = dsx_types::platform::sessions_dir();
    let index_path = dir.join("index.json");
    if let Ok(data) = fs::read_to_string(&index_path) {
        if let Ok(mut metas) = serde_json::from_str::<Vec<SessionMeta>>(&data) {
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

                        let ok = app.setup.fetch_models();
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
                            dsx_types::platform::config_path().display()
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
    stdin: &mut ChildStdin,
    agent_rx: &mpsc::Receiver<Agent2Ui>,
    send: impl Fn(&mut ChildStdin, &dsx_proto::Ui2Agent),
) -> std::io::Result<()> {
    let mut agent_dead = false;
    loop {
        // 1. Drain all pending agent frames — then draw immediately
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

        // 2. Render (ask overlay included if active)
        terminal.draw(|frame| {
            ui::render_chat(frame, app);
            if app.ask.is_some() { ui::render_ask(frame, app); }
        })?;
        app.tick();

        if app.should_quit { return Ok(()); }

        // 3. Handle keyboard (non-blocking)
        if event::poll(std::time::Duration::ZERO)? {
            let key = if let Event::Key(k) = event::read()? { k } else { continue };
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
                            if opt.is_empty() && !ask.custom_input.is_empty() { ask.custom_input.clone() } else { opt.clone() }
                        } else { String::new() };
                        if !reply.is_empty() {
                            send(stdin, &dsx_proto::Ui2Agent::UserInput { text: reply });
                        }
                        app.ask = None;
                    }
                    (_, KeyCode::Backspace) => { ask.custom_input.pop(); }
                    (_, KeyCode::Char(c)) => { ask.custom_input.push(c); }
                    _ => {}
                }
                continue;
            }

            // Normal chat keys
            match (key.modifiers, key.code) {
                (_, KeyCode::F(10)) => {
                    if agent_dead { continue; }
                    let menu = crate::app::MenuState::new(app);
                    run_menu(terminal, app, menu)?;
                    send(stdin, &dsx_proto::Ui2Agent::ReloadConfig);
                }
                (KeyModifiers::NONE, KeyCode::F(12)) => {
                    app.show_debug = !app.show_debug;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c'))
                | (KeyModifiers::NONE, KeyCode::Char('q'))
                | (_, KeyCode::F(3)) => return Ok(()),
                (_, KeyCode::Esc) => {
                    if !agent_dead {
                        send(stdin, &dsx_proto::Ui2Agent::Cancel);
                    }
                    app.status = app.setup.lang.t_chat_cancelled().to_string();
                }
                (KeyModifiers::CONTROL, KeyCode::Enter) => {
                    app.input.push('\n');
                }
                (_, KeyCode::Enter) => {
                    if agent_dead || app.busy { continue; }
                    let text = app.input.drain(..).collect::<String>();
                    if !text.trim().is_empty() {
                        app.messages.push(app::ChatMessage {
                            role: app::ChatRole::User,
                            content: text.clone(),
                            lines: text.lines().map(|l| ratatui::text::Line::from(l.to_string())).collect(),
                        });
                        app.status = app.setup.lang.t_chat_thinking().to_string();
                        app.busy = true;
                        send(stdin, &dsx_proto::Ui2Agent::UserInput { text });
                    }
                }
                (_, KeyCode::Backspace) => {
                    // Remove last grapheme cluster (supports emoji, CJK)
                    if let Some((idx, _)) = app.input.char_indices().rev().next() {
                        app.input.truncate(idx);
                    }
                }
                (_, KeyCode::Char(c)) => { app.input.push(c); }
                (_, KeyCode::Up) | (_, KeyCode::PageUp) => {
                    let n = if key.code == KeyCode::PageUp { 10 } else { 1 };
                    app.scroll_offset = app.scroll_offset.saturating_add(n);
                }
                (_, KeyCode::Down) | (_, KeyCode::PageDown) => {
                    let n = if key.code == KeyCode::PageDown { 10 } else { 1 };
                    app.scroll_offset = app.scroll_offset.saturating_sub(n);
                }
                _ => {}
            }
        } else if !app.streaming && !agent_dead {
            std::thread::sleep(std::time::Duration::from_millis(16));
        } else {
            // Micro-sleep during streaming to avoid 100% CPU
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
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
