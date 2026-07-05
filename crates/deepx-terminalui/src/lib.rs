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
use deepx_types::ConfigStore;
use ratatui::DefaultTerminal;
use std::sync::mpsc;

/// Spawn the agent by connecting to the daemon over Unix socket.
/// Falls back to direct child process spawn when daemon is unavailable.
fn spawn_agent_subprocess(
    resume_seed: Option<&str>,
    _last_seq: u64,
) -> Result<(mpsc::Sender<Ui2Agent>, mpsc::Receiver<Agent2Ui>, std::process::Child, bool), String> {
    // ── Determine session seed ──
    let seed: String = match resume_seed {
        Some(s) => s.to_string(),
        None => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            format!("s{:016x}", nanos)
        }
    };

    // ── Attempt daemon TCP connection ──
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        use std::time::Duration;
        use deepx_proto::{FrontendToDaemon, DaemonToFrontend};

        let exe = std::env::current_exe().map_err(|e| format!("exe path: {e}"))?;

        // Read port from file with retry
        let mut stream = {
            let mut result: Option<std::net::TcpStream> = None;
            let mut attempt = 0;
            while result.is_none() && attempt < 3 {
                if let Some(port) = deepx_daemon::read_port() {
                    result = deepx_daemon::transport::connect(port).ok();
                }
                if attempt == 0 {
                    // Auto-spawn daemon on first failure
                    let _ = Command::new(&exe)
                        .arg("daemon")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
                attempt += 1;
                if result.is_none() && attempt < 3 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            match result {
                Some(s) => s,
                None => {
                    return spawn_agent_child(&seed, &exe, resume_seed)
                        .map(|(tx, rx, child)| (tx, rx, child, false));
                }
            }
        };

        let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
        let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

        // ── Daemon reconnect handshake: send Subscribe + Reconnect before spawning threads ──
        {
            let sub = FrontendToDaemon {
                seed: seed.clone(),
                frame: Ui2Agent::Subscribe { seed: seed.clone() },
            };
            deepx_daemon::transport::write_frame(&mut stream, &sub)
                .map_err(|e| format!("daemon subscribe: {e}"))?;

            let recon = FrontendToDaemon {
                seed: seed.clone(),
                frame: Ui2Agent::Reconnect { seed: seed.clone(), last_seq: _last_seq },
            };
            deepx_daemon::transport::write_frame(&mut stream, &recon)
                .map_err(|e| format!("daemon reconnect: {e}"))?;
            // Responses (Snapshot) are read by the reader thread below.
        }

        let seed_w = seed.clone();
        let mut stream_w = stream.try_clone()
            .map_err(|e| format!("clone stream: {e}"))?;
        let mut stream_r = stream;

        // Writer thread: mpsc → socket (Ui2Agent → FrontendToDaemon)
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                while let Ok(frame) = tui_rx.recv() {
                    let daemon_frame = FrontendToDaemon {
                        seed: seed_w.clone(),
                        frame,
                    };
                    if deepx_daemon::transport::write_frame(&mut stream_w, &daemon_frame).is_err() {
                        break;
                    }
                }
            }));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() { s.to_string() }
                    else if let Some(s) = e.downcast_ref::<String>() { s.clone() }
                    else { "unknown panic".into() };
                eprintln!("[TUI] writer thread panicked: {}", msg);
            }
        });

        // Reader thread: socket → mpsc (DaemonToFrontend → Agent2Ui)
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                loop {
                    match deepx_daemon::transport::read_frame(&mut stream_r) {
                        Ok(Some(frame)) => {
                            if agent_tx.send(frame.event).is_err() {
                                break;
                            }
                        }
                        Ok(None) => break, // clean EOF
                        Err(_) => break,
                    }
                }
            }));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() { s.to_string() }
                    else if let Some(s) = e.downcast_ref::<String>() { s.clone() }
                    else { "unknown panic".into() };
                eprintln!("[TUI] reader thread panicked: {}", msg);
            }
        });

        // No actual child process — return a sentinel so callers can still call kill/wait.
        // The dummy exits instantly, so kill/wait are no-ops (errors ignored).
        let dummy = Command::new(&exe)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("dummy child: {e}"))?;

        return Ok((tui_tx, agent_rx, dummy, true));
    }

    // ── Deprecated fallback / Windows: spawn agent as direct child process ──
    // Will be removed once Windows named pipe support is implemented.
    eprintln!("[deepx] daemon unavailable, falling back to deprecated direct child spawn");
    spawn_agent_child(&seed, &std::env::current_exe().map_err(|e| format!("exe path: {e}"))?, resume_seed)
        .map(|(tx, rx, child)| (tx, rx, child, false))
}

/// Spawn the agent as a child process communicating over stdin/stdout.
/// Used as fallback when the daemon is not available (Windows / Unix without daemon).
fn spawn_agent_child(
    _seed: &str,
    exe: &std::path::Path,
    resume_seed: Option<&str>,
) -> Result<(mpsc::Sender<Ui2Agent>, mpsc::Receiver<Agent2Ui>, std::process::Child), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(exe);
    cmd.arg("agent");
    if let Some(seed) = resume_seed {
        cmd.arg("--resume-seed").arg(seed);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped()) // capture crash logs — was null()
        .spawn()
        .map_err(|e| format!("spawn agent subprocess: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    // Pipe agent stderr to log file for crash diagnosis
    let debug_seed = if let Some(s) = resume_seed { s.to_string() } else { _seed.to_string() };
    std::thread::spawn(move || {
        let log_path = deepx_types::platform::data_dir()
            .join(format!("agent_{}_debug.log", &debug_seed[..debug_seed.floor_char_boundary(debug_seed.len().min(8))]));
        let mut writer = std::fs::File::create(&log_path)
            .unwrap_or_else(|_| std::fs::File::create(deepx_types::platform::data_dir().join("agent_debug.log")).unwrap());
        use std::io::BufRead;
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => { use std::io::Write; let _ = write!(writer, "{}", line); }
            }
        }
    });

    let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    // Writer thread: mpsc → child stdin (Ui2Agent → JSON lines)
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            while let Ok(frame) = tui_rx.recv() {
                let json = match serde_json::to_string(&frame) {
                    Ok(j) => j,
                    Err(e) => {
                        eprintln!("[TUI] writer: JSON serialize error: {e}");
                        break;
                    }
                };
                if writeln!(stdin, "{}", json).is_err() {
                    eprintln!("[TUI] writer: stdin write error — pipe broken");
                    break;
                }
                if stdin.flush().is_err() {
                    eprintln!("[TUI] writer: stdin flush error — pipe broken");
                    break;
                }
            }
            eprintln!("[TUI] writer: tui_rx disconnected — exiting");
        }));
        if let Err(e) = result {
            let msg = if let Some(s) = e.downcast_ref::<&str>() { s.to_string() }
                else if let Some(s) = e.downcast_ref::<String>() { s.clone() }
                else { "unknown panic".into() };
            eprintln!("[TUI] writer thread panicked: {}", msg);
        }
        eprintln!("[TUI] writer thread exited");
    });

    // Reader thread: child stdout → mpsc (JSON lines → Agent2Ui)
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("[TUI] reader thread: stdout read error: {e}");
                        break;
                    }
                };
                if line.trim().is_empty() { continue; }
                if let Ok(event) = serde_json::from_str::<Agent2Ui>(&line) {
                    if agent_tx.send(event).is_err() {
                        break;
                    }
                }
            }
            // Agent stdout pipe broke — emit error so TUI can react
            let _ = agent_tx.send(Agent2Ui::Error {
                message: "Agent process stdout pipe closed — agent may have exited".into(),
            });
        }));
        if let Err(e) = result {
            let msg = if let Some(s) = e.downcast_ref::<&str>() { s.to_string() }
                else if let Some(s) = e.downcast_ref::<String>() { s.clone() }
                else { "unknown panic".into() };
            eprintln!("[TUI] reader thread panicked: {}", msg);
            let _ = agent_tx.send(Agent2Ui::Error { message: format!("Reader thread panicked: {msg}") });
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
        run_session_screen(terminal, &mut app)?;
        if app.should_quit { return Ok(()); }
        app.scroll_offset = 0;
        app.status = app.setup.lang.t_chat_ready().to_string();
        match spawn_agent_subprocess(app.resume_seed.as_deref(), app.last_seq) {
            Ok((mut tui_tx, agent_rx, mut child_handle, is_daemon)) => {
                if is_daemon {
                    // Daemon path: drain handshake events (Snapshot) before chat.
                    let mut saw_snapshot = false;
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    loop {
                        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            if saw_snapshot { break; }
                            app.status = "Daemon handshake timed out (10s)".into();
                            return Ok(());
                        }
                        match agent_rx.recv_timeout(remaining) {
                            Ok(frame) => {
                                if matches!(&frame, Agent2Ui::Snapshot { .. }) {
                                    saw_snapshot = true;
                                }
                                app.handle_frame(frame);
                                if saw_snapshot {
                                    // Non-blocking drain of any remaining handshake events
                                    while let Ok(f) = agent_rx.try_recv() {
                                        app.handle_frame(f);
                                    }
                                    break;
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                app.status = "Daemon disconnected during handshake".into();
                                return Ok(());
                            }
                        }
                    }
                } else if app.resume_seed.is_none() {
                    // Child path: new session — wait for Ready frame before sending CreateSession
                    match agent_rx.recv_timeout(std::time::Duration::from_secs(10)) {
                        Ok(Agent2Ui::Ready) => {},
                        Ok(_) => {
                            // Drain remaining and check for Ready
                            let mut ready = false;
                            while let Ok(frame) = agent_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                                if matches!(frame, Agent2Ui::Ready) { ready = true; break; }
                            }
                            if !ready {
                                app.status = "Agent did not send Ready within 10s".into();
                                let _ = child_handle.kill();
                                return Ok(());
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            app.status = "Agent startup timed out (10s)".into();
                            let _ = child_handle.kill();
                            return Ok(());
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            app.status = "Agent died before ready".into();
                            return Ok(());
                        }
                    }
                    let _ = tui_tx.send(Ui2Agent::CreateSession);
                } else {
                    // Child path: resume session — wait for Ready frame before sending ResumeSession
                    match agent_rx.recv_timeout(std::time::Duration::from_secs(10)) {
                        Ok(Agent2Ui::Ready) => {},
                        Ok(_) => {
                            let mut ready = false;
                            while let Ok(frame) = agent_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                                if matches!(frame, Agent2Ui::Ready) { ready = true; break; }
                            }
                            if !ready {
                                app.status = "Agent did not send Ready within 10s".into();
                                let _ = child_handle.kill();
                                return Ok(());
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            app.status = "Agent startup timed out (10s)".into();
                            let _ = child_handle.kill();
                            return Ok(());
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            app.status = "Agent died before ready".into();
                            return Ok(());
                        }
                    }
                    let seed = app.resume_seed.clone().expect("resume_seed is Some");
                    let _ = tui_tx.send(Ui2Agent::ResumeSession { seed });
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
    app.sessions = deepx_session::SessionManager::global().list();
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
                (KeyModifiers::SHIFT, KeyCode::Up) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_up(1);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::Down) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_down(1);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::PageUp) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_up(6);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::PageDown) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_down(6);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
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

        let mut had_input = false;

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Resize(_, _) => {
                    // ratatui Terminal auto-resizes on next draw — nothing to do here
                    had_input = true;
                }
                Event::Paste(data) => {
                    // Replace all newlines with space so multi-line paste becomes single line.
                    let mut text = data.replace("\r\n", " ").replace('\n', " ").replace('\r', " ");
                    // Collapse consecutive spaces
                    while text.contains("  ") { text = text.replace("  ", " "); }
                    let text = text.trim();
                    const MAX_INPUT: usize = 10_000;
                    let available = MAX_INPUT.saturating_sub(app.input_state.input.len());
                    if available == 0 { continue; }
                    let text = if text.len() > available { &text[..available] } else { text };
                    app.input_state.input.insert_str(app.input_state.cursor, text);
                    app.input_state.cursor += text.len();
                    had_input = true;
                }
                Event::Key(key) => {
            if key.kind != KeyEventKind::Press { continue; }
            had_input = true;

            // Ask popup: intercept keys
            if let Some(ask) = app.ask.as_mut() {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc) => { app.ask = None; }
                    (_, KeyCode::Up) => { if ask.selected > 0 { ask.selected -= 1; } }
                    (_, KeyCode::Down) => { if ask.selected + 1 < ask.options.len() { ask.selected += 1; } }
                    (_, KeyCode::Enter) => {
                        let reply = if ask.selected < ask.options.len() {
                            let opt = &ask.options[ask.selected];
                            if opt.is_empty() {
                                if ask.custom_input.is_empty() { None }
                                else { Some(ask.custom_input.clone()) }
                            } else {
                                Some(opt.clone())
                            }
                        } else { None };
                        if let Some(reply) = reply {
                            if !reply.is_empty() {
                                send(tui_tx, &deepx_proto::Ui2Agent::UserInput { text: reply });
                            }
                            app.ask = None;
                        }
                    }
                    (_, KeyCode::Backspace) => { ask.custom_input.pop(); }
                    (_, KeyCode::Char(c)) => { ask.custom_input.push(c); }
                    _ => {}
                }
            } else if app.visibility.show_help {
                // Help overlay
                match (key.modifiers, key.code) {
                    (_, KeyCode::Char('?')) | (_, KeyCode::Esc) => { app.visibility.show_help = false; }
                    _ => {}
                }
            } else {
                // Normal chat keys
                match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('?')) => {
                    app.visibility.show_help = !app.visibility.show_help;
                    app.scroll_offset = 0;
                }
                (KeyModifiers::NONE, KeyCode::F(6)) => {
                    app.visibility.show_thinking = !app.visibility.show_thinking;
                    app.scroll_offset = 0;
                }
                (_, KeyCode::F(10)) => {
                    if agent_dead { continue; }
                    let menu = crate::app::MenuState::new(app);
                    run_menu(terminal, app, menu)?;
                    send(tui_tx, &deepx_proto::Ui2Agent::ReloadConfig);
                }
                (KeyModifiers::NONE, KeyCode::F(12)) => {
                    app.visibility.show_debug = !app.visibility.show_debug;
                }
                (KeyModifiers::NONE, KeyCode::F(9)) => {
                    app.visibility.show_tasks = !app.visibility.show_tasks;
                }
                (KeyModifiers::NONE, KeyCode::F(11)) => {
                    // Toggle detail pane
                    app.detail_pane = None;
                    app.message_version = app.message_version.wrapping_add(1);
                }
                (KeyModifiers::NONE, KeyCode::F(8)) => {
                    app.visibility.show_context = !app.visibility.show_context;
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
                    app.input_state.input.insert(app.input_state.cursor, '\n');
                    app.input_state.cursor += 1;
                }
                (_, KeyCode::Enter) => {
                    if agent_dead || app.busy { continue; }
                    let text = app.input_state.input.drain(..).collect::<String>();
                    app.input_state.cursor = 0;
                    app.input_state.history_idx = None;
                    app.input_state.draft_input.clear();
                    if !text.trim().is_empty() {
                        // Dedup: don't push identical consecutive entries
                        if app.input_state.input_history.last().map_or(true, |last| last != &text) {
                            app.input_state.input_history.push(text.clone());
                        }
                        // Cap history to 200 entries
                        if app.input_state.input_history.len() > 200 {
                            app.input_state.input_history.remove(0);
                        }
                        app.status = app.setup.lang.t_chat_thinking().to_string();
                        app.busy = true;
                        send(tui_tx, &deepx_proto::Ui2Agent::UserInput { text });
                    }
                }
                (_, KeyCode::Backspace) => {
                    if app.input_state.cursor > 0 {
                        if let Some((idx, _)) = app.input_state.input[..app.input_state.cursor].char_indices().rev().next() {
                            app.input_state.input.remove(idx);
                            app.input_state.cursor = idx;
                        }
                    }
                }
                (_, KeyCode::Delete) => {
                    if app.input_state.cursor < app.input_state.input.len() {
                        if let Some((_, _)) = app.input_state.input[app.input_state.cursor..].char_indices().next() {
                            app.input_state.input.remove(app.input_state.cursor);
                        }
                    }
                }
                (_, KeyCode::Left) => {
                    if app.input_state.cursor > 0 {
                        if let Some((idx, _)) = app.input_state.input[..app.input_state.cursor].char_indices().rev().next() {
                            app.input_state.cursor = idx;
                        } else {
                            app.input_state.cursor = 0;
                        }
                    }
                }
                (_, KeyCode::Right) => {
                    if app.input_state.cursor < app.input_state.input.len() {
                        if let Some((idx, _)) = app.input_state.input[app.input_state.cursor..].char_indices().nth(1) {
                            app.input_state.cursor = app.input_state.cursor + idx;
                        } else {
                            app.input_state.cursor = app.input_state.input.len();
                        }
                    }
                }
                (_, KeyCode::Home) => { app.input_state.cursor = 0; }
                (_, KeyCode::End) => { app.input_state.cursor = app.input_state.input.len(); }
                (_, KeyCode::Char(c)) => {
                    const MAX_INPUT: usize = 10_000;
                    if app.input_state.input.len() >= MAX_INPUT { continue; }
                    // Typing exits history browse mode
                    if app.input_state.history_idx.is_some() {
                        app.input_state.history_idx = None;
                        app.input_state.draft_input.clear();
                    }
                    app.input_state.input.insert(app.input_state.cursor, c);
                    app.input_state.cursor += c.len_utf8();
                }
                (KeyModifiers::SHIFT, KeyCode::Up) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_up(1);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::Down) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_down(1);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::PageUp) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_up(6);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::PageDown) => {
                    if let Some(ref mut pane) = app.detail_pane {
                        pane.scroll_down(6);
                        app.message_version = app.message_version.wrapping_add(1);
                    }
                }
                (_, KeyCode::Up) => {
                    // If cursor is on the first line of input, browse history
                    let cursor_line = app.input_state.input[..app.input_state.cursor].chars().filter(|&c| c == '\n').count();
                    if cursor_line == 0 && !app.input_state.input_history.is_empty() {
                        if app.input_state.history_idx.is_none() {
                            app.input_state.draft_input = app.input_state.input.clone();
                            app.input_state.history_idx = Some(app.input_state.input_history.len() - 1);
                        } else if let Some(idx) = app.input_state.history_idx {
                            if idx > 0 { app.input_state.history_idx = Some(idx - 1); }
                        }
                        if let Some(idx) = app.input_state.history_idx {
                            app.input_state.input = app.input_state.input_history[idx].clone();
                            app.input_state.cursor = app.input_state.input.len();
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
                    if app.input_state.history_idx.is_some() {
                        if let Some(idx) = app.input_state.history_idx {
                            if idx + 1 < app.input_state.input_history.len() {
                                app.input_state.history_idx = Some(idx + 1);
                                app.input_state.input = app.input_state.input_history[idx + 1].clone();
                            } else {
                                // Past the last history entry — restore draft
                                app.input_state.history_idx = None;
                                app.input_state.input = app.input_state.draft_input.clone();
                                app.input_state.draft_input.clear();
                            }
                            app.input_state.cursor = app.input_state.input.len();
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
            } // else
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

        // 3. Render — throttle when idle, but render immediately on user input
        // for instant visual feedback (typing, scrolling, toggles).
        let now = Instant::now();
        let render_interval = if app.streaming {
            Duration::from_millis(33) // ~30 FPS streaming
        } else {
            Duration::from_millis(100) // ~10 FPS idle
        };
        if had_input || now.duration_since(app.last_render) >= render_interval {
            terminal.draw(|frame| {
                ui::render_chat(frame, app);
                if app.ask.is_some() { ui::render_ask(frame, app); }
                if app.visibility.show_help { ui::render_help(frame, app); }
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
                        let item = &mut menu.items[menu.selected];
                        if !menu.edit_buf.is_empty() || item.key == "api_key" {
                            item.value = menu.edit_buf.clone();
                            if item.key == "api_key" {
                                item.secret = menu.edit_buf.clone();
                                // Update display mask for the cleared/updated key
                                let l = menu.lang;
                                item.value = if menu.edit_buf.is_empty() {
                                    if l.as_str() == "zh" { "(未设置)" } else { "(not set)" }.into()
                                } else if menu.edit_buf.len() > 3 {
                                    format!("sk-{}", "●".repeat(menu.edit_buf.len().saturating_sub(3).min(20)))
                                } else {
                                    menu.edit_buf.clone()
                                };
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
                    } else if menu.items.get(menu.selected).map_or(false, |i| i.editable) {
                        // Start editing on any editable item when user types
                        menu.editing = true;
                        menu.edit_buf.clear();
                        menu.edit_buf.push(c);
                    }
                }
                _ => {}
            }
        }
    }
}