//! DeepX TUI — terminal frontend for the dsx agent.
//!
//! Spawns `dsx agent` as a child process, communicates via stdin/stdout
//! JSON-LP protocol (Ui2Agent / Agent2Ui), renders a chat-like interface.
//! Falls back to setup wizard if no config file exists.

mod app;
mod i18n;
mod ui;

use app::{App, Screen};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use dsx_proto::Agent2Ui;
use dsx_types::ConfigStore;
use ratatui::DefaultTerminal;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;

fn spawn_agent() -> anyhow::Result<(Child, ChildStdin, mpsc::Receiver<Agent2Ui>, thread::JoinHandle<()>)> {
    let mut child = Command::new("dsx")
        .arg("agent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
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
            run_setup(terminal, &mut app, &store)
        } else {
            match spawn_agent() {
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
        }
    });

    Ok(result?)
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

        while event::poll(std::time::Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),
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
                            });
                            app.input.clear();
                            app.status = "Thinking...".into();
                            send(stdin, &dsx_proto::Ui2Agent::UserInput { text });
                        }
                    }
                    (_, KeyCode::Backspace) => { app.input.pop(); }
                    (_, KeyCode::Char(c)) => { app.input.push(c); }
                    _ => {}
                }
            }
        }

        while let Ok(frame) = agent_rx.try_recv() {
            app.handle_frame(frame);
        }

        if app.should_quit { return Ok(()); }
    }
}
