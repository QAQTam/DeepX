//! Debug panel widget.
//!
//! Displays debug information: frame count, token usage, cache stats,
//! agent protocol frames log, and error details.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use crate::theme::Theme;

/// Debug information panel.
pub struct DebugPanel<'a> {
    /// Debug text lines.
    pub lines: &'a [String],
    /// Frame count.
    pub frame_count: u64,
    /// Context tokens used.
    pub context_tokens: u32,
    /// Session total tokens.
    pub session_tokens: u64,
    /// Cache hit tokens.
    pub cache_hit: u32,
    /// Cache miss tokens.
    pub cache_miss: u32,
    /// Theme.
    pub theme: &'a Theme,
}

impl Widget for DebugPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let block = Block::new()
            .title("Debug")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.theme.popup_border)
            .style(self.theme.debug_bg);

        let inner = block.inner(area);
        block.render(area, buf);

        // Build debug info
        let mut text = Vec::new();
        text.push(Line::from(Span::styled(
            format!("Frame: {} | Tokens: {} (ctx) / {} (session) | Cache: hit={} miss={}",
                self.frame_count, self.context_tokens, self.session_tokens,
                self.cache_hit, self.cache_miss),
            self.theme.debug_fg,
        )));
        text.push(Line::from(""));
        for line in self.lines.iter().take(inner.height.saturating_sub(2) as usize) {
            text.push(Line::from(Span::styled(line.clone(), self.theme.debug_fg)));
        }

        Paragraph::new(text).render(inner, buf);
    }
}
