//! Help dialog widget.
//!
//! Displays key bindings and usage help overlay.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use crate::theme::Theme;

/// Help overlay showing key bindings.
pub struct HelpDialog<'a> {
    pub theme: &'a Theme,
}

impl Widget for HelpDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let block = Block::new()
            .title(" Help — Key Bindings ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.theme.popup_border)
            .style(self.theme.help_bg);

        let inner = block.inner(area);
        block.render(area, buf);

        let help_text = vec![
            Line::from(""),
            Line::from("  Enter         Send message"),
            Line::from("  Ctrl+Enter    Insert newline"),
            Line::from("  Esc           Cancel request / Dismiss overlay"),
            Line::from("  Ctrl+C / F3   Quit"),
            Line::from("  ?             Toggle this help"),
            Line::from("  F6            Toggle thinking"),
            Line::from("  F8            Toggle context"),
            Line::from("  F9            Toggle tasks"),
            Line::from("  F10           Settings menu"),
            Line::from("  F11           Toggle detail pane"),
            Line::from("  F12           Toggle debug"),
            Line::from("  Up/Down       Scroll or history"),
            Line::from("  PageUp/Down   Scroll by page"),
            Line::from("  Ctrl+L        Clear screen"),
            Line::from("  Ctrl+Left/Right  Word skip"),
            Line::from("  Ctrl+Backspace   Delete word"),
            Line::from(""),
        ];

        Paragraph::new(help_text)
            .style(self.theme.help_fg)
            .render(inner, buf);
    }
}
