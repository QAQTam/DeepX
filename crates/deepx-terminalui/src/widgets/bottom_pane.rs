//! BottomPane — combined status bar, tab bar, view content, and composer.
//!
//! Mirrors Codex's BottomPane structure:
//! ```
//! ┌─────────────────────────┐
//! │ StatusLine              │  1 row
//! │ Chat | Files            │  TabBar 1 row
//! │ View Content            │  flex
//! │ Composer                │  dynamic rows
//! └─────────────────────────┘
//! ```

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::render::renderable::Renderable;
use crate::theme::Theme;

/// Tabs in the bottom pane.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Chat,
    Files,
}

/// Combined bottom pane widget.
pub struct BottomPane<'a> {
    /// Active tab.
    pub active_tab: BottomTab,
    /// Available tabs.
    pub tabs: &'a [BottomTab],
    /// Status text (e.g. "Thinking...", "cancelled").
    pub status: &'a str,
    /// Whether the agent is streaming.
    pub is_streaming: bool,
    /// Composer input text.
    pub composer_input: &'a str,
    /// Composer cursor position.
    pub composer_cursor: usize,
    /// Theme.
    pub theme: &'a Theme,
}

impl Renderable for BottomPane<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }
        let mut y = area.y;

        // Status line
        if area.height >= 1 {
            let status_text = if self.is_streaming {
                format!("\u{2022} {}", self.status)
            } else {
                self.status.to_string()
            };
            let status_style = if self.is_streaming {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                self.theme.status_bar_fg
            };
            Paragraph::new(Line::from(Span::styled(status_text, status_style)))
                .render(Rect { x: area.x, y, width: area.width, height: 1 }, buf);
            y += 1;
        }

        // Tab bar
        if area.height >= 2 {
            let tab_line = build_tab_bar(self.tabs, self.active_tab, self.theme, area.width);
            Paragraph::new(tab_line)
                .render(Rect { x: area.x, y, width: area.width, height: 1 }, buf);
            y += 1;
        }

        // Composer
        let remaining = area.height.saturating_sub(y.saturating_sub(area.y));
        if remaining >= 1 {
            let composer_area = Rect { x: area.x, y, width: area.width, height: remaining };
            render_composer(
                self.composer_input,
                self.composer_cursor,
                self.theme,
                composer_area,
                buf,
            );
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let composer_lines = self.composer_input.lines().count().max(1);
        (2 + composer_lines) as u16 // status + tabbar + composer
    }
}

/// Build the tab bar line.
fn build_tab_bar(tabs: &[BottomTab], active: BottomTab, theme: &Theme, width: u16) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    for tab in tabs {
        let name = match tab {
            BottomTab::Chat => " Chat ",
            BottomTab::Files => " Files ",
        };
        let style = if *tab == active {
            Style::new()
                .fg(Color::Black)
                .bg(Color::Rgb(100, 200, 255))
                .add_modifier(Modifier::BOLD)
        } else {
            theme.status_bar_fg
        };
        spans.push(Span::styled(name, style));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

/// Render the composer input area.
fn render_composer(
    input: &str,
    cursor_pos: usize,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    // Border line
    let border = "\u{2500}".repeat(area.width as usize);
    Paragraph::new(Line::from(Span::styled(border, theme.input_border)))
        .render(Rect { x: area.x, y: area.y, width: area.width, height: 1 }, buf);

    if area.height < 2 {
        return;
    }
    let input_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    };

    if input.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            "Type a message...",
            theme.input_placeholder,
        )))
        .render(input_area, buf);
    } else {
        let display = if cursor_pos < input.len() {
            let end = input.floor_char_boundary(
                std::cmp::min(cursor_pos + 1, input.len())
            );
            format!("{}|{}", &input[..cursor_pos], &input[end..])
        } else {
            format!("{}|", input)
        };
        Paragraph::new(Line::from(Span::styled(display, theme.input_text)))
            .render(input_area, buf);
    }
}
