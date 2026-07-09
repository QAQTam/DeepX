//! Popup overlay stack.
//!
//! Manages a stack of overlay widgets rendered on top of the main UI.
//! Each overlay occupies a portion of the screen and dims the background.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

/// A single overlay layer.
pub struct PopupLayer<'a> {
    pub title: &'a str,
    pub content: &'a str,
    /// Fraction of screen width (0.0-1.0).
    pub width_ratio: f64,
    /// Fraction of screen height (0.0-1.0).
    pub height_ratio: f64,
}

/// Renders a popup stack — topmost layer only (for now).
pub struct PopupStack<'a> {
    pub layers: &'a [PopupLayer<'a>],
    pub border_style: Style,
    pub bg_style: Style,
}

impl Widget for PopupStack<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        // Dim background
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    if cell.style().bg == Some(Color::Reset) || cell.style().bg.is_none() {
                        cell.set_style(Style::new().bg(Color::Rgb(0, 0, 0)));
                    }
                }
            }
        }

        // Render topmost layer
        if let Some(layer) = self.layers.last() {
            let popup_w = (area.width as f64 * layer.width_ratio) as u16;
            let popup_h = (area.height as f64 * layer.height_ratio) as u16;
            let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
            let popup_y = area.y + (area.height.saturating_sub(popup_h)) / 2;

            let popup_area = Rect::new(popup_x, popup_y, popup_w.max(20), popup_h.max(3));

            // Clear the popup area
            Clear.render(popup_area, buf);

            // Render border + content
            let block = Block::new()
                .title(layer.title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(self.border_style)
                .style(self.bg_style);

            let inner = block.inner(popup_area);
            block.render(popup_area, buf);

            Paragraph::new(layer.content).render(inner, buf);
        }
    }
}
