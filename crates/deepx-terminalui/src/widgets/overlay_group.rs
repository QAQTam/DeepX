//! OverlayGroup — layer system for popups that stack on top of the main UI.
//!
//! Each overlay dims the background and renders centered content.
//! Layers are rendered in order (last = topmost).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

use crate::render::renderable::Renderable;
use crate::theme::Theme;

/// A single overlay layer.
pub struct OverlayLayer {
    pub title: String,
    pub content: Vec<String>,
}

/// Renders overlays stacked on top of the main UI.
pub struct OverlayGroup<'a> {
    pub layers: &'a [OverlayLayer],
    pub theme: &'a Theme,
}

impl Renderable for OverlayGroup<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.layers.is_empty() {
            return;
        }

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
        let layer = &self.layers[self.layers.len() - 1];
        let popup_w = (area.width as f64 * 0.7) as u16;
        let popup_h = (area.height as f64 * 0.6) as u16;
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_h)) / 2;
        let popup_area = Rect::new(popup_x, popup_y, popup_w.max(20), popup_h.max(3));

        Clear.render(popup_area, buf);

        let block = Block::new()
            .title(layer.title.as_str())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.theme.popup_border)
            .style(self.theme.popup_bg);

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let text: Vec<ratatui::text::Line> = layer.content.iter()
            .map(|l| ratatui::text::Line::from(l.as_str()))
            .collect();
        Paragraph::new(text).render(inner, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        0 // overlays don't participate in layout
    }
}
