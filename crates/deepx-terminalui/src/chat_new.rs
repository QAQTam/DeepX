//! New TUI chat loop using the Codex-inspired rendering pipeline.
//!
//! Uses: EventBroker, AppEvent dispatch, FlexRenderable layout,
//! ChatWidgetV2 with HistoryCell, BottomPane with tab bar.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use deepx_proto::{Agent2Ui, Ui2Agent};
use ratatui::DefaultTerminal;

use crate::app::App;
use crate::app::event_dispatch;
use crate::events::AppEvent;
use crate::render::renderable::Renderable;
use crate::terminal::{EventBroker, FrameRequester};
use crate::theme::Theme;
use crate::ui::layout::build_main_layout;

pub fn run_chat_new(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    tui_tx: &mut mpsc::Sender<Ui2Agent>,
    agent_rx: &mpsc::Receiver<Agent2Ui>,
    send: impl Fn(&mut mpsc::Sender<Ui2Agent>, &Ui2Agent),
) -> std::io::Result<()> {
    let mut agent_dead = false;
    let theme = Theme::dark();
    let frame_req = FrameRequester::new();
    let mut broker = EventBroker::new();

    loop {
        let poll_timeout = if app.streaming {
            Duration::from_millis(66)
        } else if agent_dead {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(100)
        };

        let mut had_input = false;

        if broker.poll(poll_timeout)? {
            if let Some(ev) = broker.read()? {
                match handle_crossterm_event(app, ev, tui_tx, &send, &mut agent_dead) {
                    Some(action) => {
                        had_input = true;
                        if action == EventAction::Quit {
                            return Ok(());
                        }
                    }
                    None => {}
                }
            }
        }

        // Drain agent frames
        loop {
            match agent_rx.try_recv() {
                Ok(frame) => {
                    event_dispatch::dispatch(app, AppEvent::AgentFrame(frame));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !agent_dead {
                        agent_dead = true;
                        event_dispatch::dispatch(app, AppEvent::AgentDisconnected);
                    }
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        if agent_dead && app.should_quit {
            return Ok(());
        }

        // Render
        let now = Instant::now();
        let render_interval = if app.streaming {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(100)
        };

        let should_render = had_input
            || frame_req.poll().is_some()
            || now.duration_since(app.last_render) >= render_interval;

        if should_render {
            terminal.draw(|frame| {
                let area = frame.area();
                let mut buf = frame.buffer_mut();

                let layout = build_main_layout(app, &theme);
                layout.render(area, buf);

                // Help overlay
                if app.visibility.show_help {
                    let help = crate::widgets::help_dialog::HelpDialog { theme: &theme };
                    ratatui::widgets::Widget::render(help, area, buf);
                }
                // Debug overlay
                if app.visibility.show_debug {
                    let recent: Vec<String> = app.debug.recent_edits.iter().cloned().collect();
                    let debug = crate::widgets::debug_panel::DebugPanel {
                        lines: &recent,
                        frame_count: app.frame_count,
                        context_tokens: app.context_tokens,
                        session_tokens: app.session_tokens,
                        cache_hit: app.cache_hit,
                        cache_miss: app.cache_miss,
                        theme: &theme,
                    };
                    ratatui::widgets::Widget::render(debug, area, buf);
                }
            })?;
            app.last_render = now;
        }
    }
}

// ── Event handling ──────────────────────────────────────────────────────

#[derive(PartialEq)]
enum EventAction {
    Quit,
    Continue,
}

fn handle_crossterm_event(
    app: &mut App,
    ev: Event,
    tui_tx: &mut mpsc::Sender<Ui2Agent>,
    send: impl Fn(&mut mpsc::Sender<Ui2Agent>, &Ui2Agent),
    agent_dead: &mut bool,
) -> Option<EventAction> {
    match ev {
        Event::Resize(w, h) => {
            event_dispatch::dispatch(app, AppEvent::Resize(w, h));
            Some(EventAction::Continue)
        }
        Event::Paste(data) => {
            event_dispatch::dispatch(app, AppEvent::Paste(data));
            Some(EventAction::Continue)
        }
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return None;
            }
            if app.ask.is_some() {
                return handle_ask_key(app, key, tui_tx, &send);
            }
            if app.visibility.show_help {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Char('?')) | (_, KeyCode::Esc) => {
                        event_dispatch::dispatch(app, AppEvent::DismissOverlay);
                    }
                    _ => {}
                }
                return Some(EventAction::Continue);
            }
            let app_event = key_to_event(key);
            match &app_event {
                AppEvent::Quit => {
                    app.should_quit = true;
                    return Some(EventAction::Quit);
                }
                AppEvent::CancelRequest => {
                    if !*agent_dead {
                        send(tui_tx, &Ui2Agent::Cancel);
                    }
                }
                AppEvent::SendMessage(_text) => {
                    if *agent_dead || app.busy {
                        return None;
                    }
                    let msg = app.input_state.input.drain(..).collect::<String>();
                    event_dispatch::dispatch(app, AppEvent::SendMessage(msg.clone()));
                    send(tui_tx, &Ui2Agent::UserInput { text: msg });
                    return Some(EventAction::Continue);
                }
                _ => {}
            }
            event_dispatch::dispatch(app, app_event);
            Some(EventAction::Continue)
        }
        _ => None,
    }
}

fn handle_ask_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    tui_tx: &mut mpsc::Sender<Ui2Agent>,
    send: impl Fn(&mut mpsc::Sender<Ui2Agent>, &Ui2Agent),
) -> Option<EventAction> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => {
            event_dispatch::dispatch(app, AppEvent::AskCancel);
            Some(EventAction::Continue)
        }
        (_, KeyCode::Up) => {
            event_dispatch::dispatch(app, AppEvent::AskUp);
            Some(EventAction::Continue)
        }
        (_, KeyCode::Down) => {
            event_dispatch::dispatch(app, AppEvent::AskDown);
            Some(EventAction::Continue)
        }
        (_, KeyCode::Enter) => {
            let reply = if let Some(ref ask) = app.ask {
                if ask.selected < ask.options.len() {
                    let opt = &ask.options[ask.selected];
                    if opt.is_empty() {
                        if ask.custom_input.is_empty() { None }
                        else { Some(ask.custom_input.clone()) }
                    } else {
                        Some(opt.clone())
                    }
                } else { None }
            } else { None };
            if let Some(reply) = reply {
                if !reply.is_empty() {
                    send(tui_tx, &Ui2Agent::UserInput { text: reply });
                }
                app.ask = None;
            }
            event_dispatch::dispatch(app, AppEvent::AskConfirm);
            Some(EventAction::Continue)
        }
        (_, KeyCode::Char(c)) => {
            if let Some(ref mut ask) = app.ask {
                if ask.allow_custom { ask.custom_input.push(c); }
            }
            Some(EventAction::Continue)
        }
        (_, KeyCode::Backspace) => {
            if let Some(ref mut ask) = app.ask {
                ask.custom_input.pop();
            }
            Some(EventAction::Continue)
        }
        _ => None,
    }
}

fn key_to_event(key: crossterm::event::KeyEvent) -> AppEvent {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::F(3)) => AppEvent::Quit,

        (KeyModifiers::CONTROL, KeyCode::Enter) => AppEvent::InsertNewline,
        (_, KeyCode::Enter) => AppEvent::SendMessage(String::new()),
        (_, KeyCode::Esc) => AppEvent::CancelRequest,
        (_, KeyCode::Char('?')) => AppEvent::ToggleHelp,

        (_, KeyCode::F(6)) => AppEvent::ToggleThinking,
        (_, KeyCode::F(8)) => AppEvent::ToggleContext,
        (_, KeyCode::F(9)) => AppEvent::ToggleTasks,
        (_, KeyCode::F(10)) => AppEvent::OpenMenu,
        (_, KeyCode::F(11)) => AppEvent::ToggleDetailPane,
        (_, KeyCode::F(12)) => AppEvent::ToggleDebug,

        (KeyModifiers::CONTROL, KeyCode::Left) => AppEvent::CursorWordLeft,
        (KeyModifiers::CONTROL, KeyCode::Right) => AppEvent::CursorWordRight,
        (_, KeyCode::Left) => AppEvent::CursorLeft,
        (_, KeyCode::Right) => AppEvent::CursorRight,
        (_, KeyCode::Home) => AppEvent::MoveToLineStart,
        (_, KeyCode::End) => AppEvent::MoveToLineEnd,

        (KeyModifiers::CONTROL, KeyCode::Backspace) => AppEvent::DeleteWordBack,
        (_, KeyCode::Backspace) => AppEvent::Backspace,
        (_, KeyCode::Delete) => AppEvent::Delete,

        (_, KeyCode::Up) => AppEvent::HistoryPrevious,
        (_, KeyCode::Down) => AppEvent::HistoryNext,
        (_, KeyCode::PageUp) => AppEvent::ScrollPageUp,
        (_, KeyCode::PageDown) => AppEvent::ScrollPageDown,

        (KeyModifiers::CONTROL, KeyCode::Char('l')) => AppEvent::ClearScreen,

        (KeyModifiers::NONE, KeyCode::Char(c)) | (KeyModifiers::SHIFT, KeyCode::Char(c)) => AppEvent::TypeChar(c),

        _ => AppEvent::Noop,
    }
}
