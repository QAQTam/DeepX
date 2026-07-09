//! Status bar widget.
//!
//! Single-line status display at the bottom of the chat area.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::theme::Theme;

/// Status bar showing current state (thinking, tool execution, etc.).
pub struct StatusBar<'a> {
    pub text: &'a str,
    pub streaming: bool,
    pub theme: &'a Theme,
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let display = if self.streaming {
            format!("... {}", self.text)
        } else {
            self.text.to_string()
        };

        Paragraph::new(Line::from(display))
            .style(self.theme.status_bar_fg)
            .render(area, buf);
    }
}
