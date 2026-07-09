//! Event dispatcher — routes AppEvent to App state mutations.
//! All cursor/index operations are byte-safe for CJK text.

use crate::app::App;
use crate::events::AppEvent;

/// Dispatch a semantic event to App. Returns true if a redraw is needed.
pub fn dispatch(app: &mut App, event: AppEvent) -> bool {
    match event {
        // Input editing
        AppEvent::TypeChar(ch) => {
            if app.input_state.input.len() < 10_000 {
                app.input_state.input.insert(app.input_state.cursor, ch);
                app.input_state.cursor += ch.len_utf8();
            }
            true
        }
        AppEvent::Backspace => {
            if app.input_state.cursor > 0 {
                let prev = prev_char_boundary(&app.input_state.input, app.input_state.cursor);
                app.input_state.input.drain(prev..app.input_state.cursor);
                app.input_state.cursor = prev;
            }
            true
        }
        AppEvent::DeleteWordBack => {
            if app.input_state.cursor == 0 {
                return false;
            }
            let before = &app.input_state.input[..app.input_state.cursor];
            let word_start = before
                .rfind(|c: char| c.is_whitespace())
                .map_or(0, |i| i + 1);
            app.input_state.input.drain(word_start..app.input_state.cursor);
            app.input_state.cursor = word_start;
            true
        }
        AppEvent::Delete => {
            if app.input_state.cursor < app.input_state.input.len() {
                let next = next_char_boundary(&app.input_state.input, app.input_state.cursor);
                app.input_state.input.drain(app.input_state.cursor..next);
            }
            true
        }
        AppEvent::MoveToLineStart => {
            app.input_state.cursor = 0;
            true
        }
        AppEvent::MoveToLineEnd => {
            app.input_state.cursor = app.input_state.input.len();
            true
        }
        AppEvent::CursorLeft => {
            if app.input_state.cursor > 0 {
                app.input_state.cursor = prev_char_boundary(&app.input_state.input, app.input_state.cursor);
            }
            true
        }
        AppEvent::CursorRight => {
            if app.input_state.cursor < app.input_state.input.len() {
                app.input_state.cursor = next_char_boundary(&app.input_state.input, app.input_state.cursor);
            }
            true
        }
        AppEvent::CursorWordLeft => {
            if app.input_state.cursor == 0 {
                return false;
            }
            let before = &app.input_state.input[..app.input_state.cursor];
            let pos = before
                .trim_end()
                .rfind(|c: char| c.is_whitespace())
                .map_or(0, |i| i + 1);
            app.input_state.cursor = pos;
            true
        }
        AppEvent::CursorWordRight => {
            let after = &app.input_state.input[app.input_state.cursor..];
            let offset = after
                .find(|c: char| c.is_whitespace())
                .map_or(after.len(), |i| i + 1);
            app.input_state.cursor += offset;
            true
        }

        // History
        AppEvent::HistoryPrevious => {
            let hist = &app.input_state.input_history;
            if hist.is_empty() {
                return false;
            }
            if let Some(idx) = app.input_state.history_idx {
                if idx > 0 {
                    app.input_state.history_idx = Some(idx - 1);
                    app.input_state.input = hist[idx - 1].clone();
                    app.input_state.cursor = app.input_state.input.len();
                }
            } else {
                app.input_state.draft_input = app.input_state.input.clone();
                let last = hist.len() - 1;
                app.input_state.history_idx = Some(last);
                app.input_state.input = hist[last].clone();
                app.input_state.cursor = app.input_state.input.len();
            }
            true
        }
        AppEvent::HistoryNext => {
            if let Some(idx) = app.input_state.history_idx {
                if idx + 1 < app.input_state.input_history.len() {
                    app.input_state.history_idx = Some(idx + 1);
                    app.input_state.input = app.input_state.input_history[idx + 1].clone();
                } else {
                    app.input_state.history_idx = None;
                    app.input_state.input = app.input_state.draft_input.clone();
                    app.input_state.draft_input.clear();
                }
                app.input_state.cursor = app.input_state.input.len();
            }
            true
        }

        // Scroll
        AppEvent::ScrollUp => {
            app.scroll_offset = app.scroll_offset.saturating_add(1);
            true
        }
        AppEvent::ScrollDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(1);
            true
        }
        AppEvent::ScrollPageUp => {
            app.scroll_offset = app.scroll_offset.saturating_add(10);
            true
        }
        AppEvent::ScrollPageDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(10);
            true
        }

        // Commands
        AppEvent::ClearScreen => {
            app.scroll_offset = 0;
            true
        }
        AppEvent::CancelRequest | AppEvent::Quit | AppEvent::OpenMenu => {
            true
        }
        AppEvent::InsertNewline => {
            true
        }
        AppEvent::SendMessage(text) => {
            if text.trim().is_empty() {
                return false;
            }
            if app.input_state.input_history.last().map_or(true, |last| last != &text) {
                app.input_state.input_history.push(text.clone());
            }
            while app.input_state.input_history.len() > 200 {
                app.input_state.input_history.remove(0);
            }
            app.status = app.setup.lang.t_chat_thinking().to_string();
            app.busy = true;
            app.input_state.input.clear();
            app.input_state.cursor = 0;
            app.input_state.history_idx = None;
            app.input_state.draft_input.clear();
            true
        }
        AppEvent::SendCommand(_) => true,

        // Panel toggles
        AppEvent::ToggleDebug => {
            app.visibility.show_debug = !app.visibility.show_debug;
            true
        }
        AppEvent::ToggleTasks => {
            app.visibility.show_tasks = !app.visibility.show_tasks;
            true
        }
        AppEvent::ToggleContext => {
            app.visibility.show_context = !app.visibility.show_context;
            true
        }
        AppEvent::ToggleHelp => {
            app.visibility.show_help = !app.visibility.show_help;
            app.scroll_offset = 0;
            true
        }
        AppEvent::ToggleThinking => {
            app.visibility.show_thinking = !app.visibility.show_thinking;
            app.scroll_offset = 0;
            true
        }
        AppEvent::ToggleDetailPane => {
            app.detail_pane = None;
            app.message_version = app.message_version.wrapping_add(1);
            true
        }
        AppEvent::DismissOverlay => {
            app.visibility.show_help = false;
            app.ask = None;
            true
        }

        // Session
        AppEvent::NewSession | AppEvent::SwitchSession => true,

        // Popup
        AppEvent::AskUp => {
            if let Some(ref mut ask) = app.ask {
                if ask.selected > 0 {
                    ask.selected -= 1;
                }
            }
            true
        }
        AppEvent::AskDown => {
            if let Some(ref mut ask) = app.ask {
                if ask.selected + 1 < ask.options.len() {
                    ask.selected += 1;
                }
            }
            true
        }
        AppEvent::AskConfirm | AppEvent::AskCancel => true,

        // Paste
        AppEvent::Paste(data) => {
            let text = data
                .replace("\r\n", " ")
                .replace('\n', " ")
                .replace('\r', " ");
            let collapsed = collapse_spaces(&text);
            let trimmed = collapsed.trim();
            let available = 10_000usize.saturating_sub(app.input_state.input.len());
            if available == 0 {
                return false;
            }
            let text = if trimmed.len() > available {
                &trimmed[..trimmed.floor_char_boundary(available)]
            } else {
                trimmed
            };
            app.input_state.input.insert_str(app.input_state.cursor, text);
            app.input_state.cursor += text.len();
            true
        }

        // Terminal
        AppEvent::Resize(_, _) => true,
        AppEvent::Tick => false,

        // Agent
        AppEvent::AgentFrame(frame) => {
            app.handle_frame(frame);
            true
        }
        AppEvent::AgentDisconnected => {
            let l = app.setup.lang;
            app.push_msg(
                crate::app::ChatRole::Status,
                if l.as_str() == "zh" {
                    "Agent disconnected - press F3 to quit"
                } else {
                    "Agent disconnected - press F3 to quit"
                },
            );
            app.status = "Agent disconnected".into();
            app.streaming = false;
            true
        }

        AppEvent::Noop => false,
    }
}

/// Find the previous char boundary before `pos` (byte-safe).
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut p = pos - 1;
    while p > 0 && !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Find the next char boundary after `pos` (byte-safe).
fn next_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let mut p = pos + 1;
    while p < s.len() && !s.is_char_boundary(p) {
        p += 1;
    }
    p
}

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    for c in s.chars() {
        if c == ' ' {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out
}
