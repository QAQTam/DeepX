//! Session selector widget.
//!
//! Renders a scrollable list of sessions for selection.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Widget};

/// Session selection list.
pub struct SessionSelector<'a> {
    /// Session names.
    pub sessions: &'a [String],
    /// Currently selected index.
    pub selected: usize,
    /// Style for selected item.
    pub selected_style: Style,
    /// Style for normal items.
    pub normal_style: Style,
    /// Block title.
    pub title: &'a str,
}

impl Widget for SessionSelector<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let block = Block::new()
            .title(self.title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);

        let inner = block.inner(area);
        block.render(area, buf);

        let items: Vec<ListItem> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let style = if i == self.selected {
                    self.selected_style
                } else {
                    self.normal_style
                };
                ListItem::new(Line::from(Span::styled(name.clone(), style)))
            })
            .collect();

        List::new(items).render(inner, buf);
    }
}
