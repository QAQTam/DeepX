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
                app.status = format!("Failed to start agent: {e}");
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
                            "Config saved to {}",
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
    loop {
        terminal.draw(|frame| ui::render_chat(frame, app))?;
        app.tick();

        if event::poll(std::time::Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match (key.modifiers, key.code) {
                    (KeyModifiers::NONE, KeyCode::F(12)) => {
                        app.show_debug = !app.show_debug;
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('c'))
                    | (KeyModifiers::NONE, KeyCode::Char('q'))
                    | (_, KeyCode::F(3)) => return Ok(()),
                    (_, KeyCode::Esc) => {
                        send(stdin, &dsx_proto::Ui2Agent::Cancel);
                        app.status = "Cancelled".into();
                    }
                    (_, KeyCode::Enter) => {
                        let text = app.input.drain(..).collect::<String>();
                        if !text.trim().is_empty() {
                            app.messages.push(app::ChatMessage {
                                role: app::ChatRole::User,
                                content: text.clone(),
                                lines: vec![ratatui::text::Line::from(text.clone())],
                            });
                            app.input.clear();
                            app.status = "Thinking...".into();
                            send(stdin, &dsx_proto::Ui2Agent::UserInput { text });
                        }
                    }
                    (_, KeyCode::Backspace) => { app.input.pop(); }
                    (_, KeyCode::Char(c)) => { app.input.push(c); }
                    (_, KeyCode::Up) | (_, KeyCode::PageUp) => {
                        let n = if key.code == KeyCode::PageUp { 12 } else { 1 };
                        app.scroll_offset = app.scroll_offset.saturating_add(n);
                    }
                    (_, KeyCode::Down) | (_, KeyCode::PageDown) => {
                        let n = if key.code == KeyCode::PageDown { 12 } else { 1 };
                        app.scroll_offset = app.scroll_offset.saturating_sub(n);
                    }
                    _ => {}
                }
            }
        }

        if let Ok(frame) = agent_rx.try_recv() {
            app.handle_frame(frame);
        } else if !app.streaming {
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        if app.should_quit { return Ok(()); }
    }
}
