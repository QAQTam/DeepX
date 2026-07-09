//! Composer widget — the input area at the bottom of the TUI.
//!
//! Handles the text input buffer, cursor position, placeholder text,
//! and visual feedback during editing.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

/// The composer / input area widget.
pub struct ComposerWidget<'a> {
    /// Current input text.
    pub input: &'a str,
    /// Cursor position (byte index).
    pub cursor_pos: usize,
    /// Prompt text shown when input is empty.
    pub placeholder: &'a str,
    /// Style for cursor.
    pub cursor_style: Style,
    /// Style for input text.
    pub text_style: Style,
    /// Style for placeholder text.
    pub placeholder_style: Style,
    /// Style for the border line above the composer.
    pub border_style: Style,
}

impl Widget for ComposerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        if area.height < 2 {
            // Not enough space — just render one line of input
            render_input_line(
                self.input,
                self.cursor_pos,
                self.placeholder,
                self.text_style,
                self.placeholder_style,
                self.cursor_style,
                area,
                buf,
            );
            return;
        }

        // Border line at top
        let border = "\u{2500}".repeat(area.width as usize);
        Paragraph::new(Line::from(Span::styled(border, self.border_style)))
            .render(
                Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );

        // Input area below border
        let input_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };

        render_input_line(
            self.input,
            self.cursor_pos,
            self.placeholder,
            self.text_style,
            self.placeholder_style,
            self.cursor_style,
            input_area,
            buf,
        );
    }
}

fn render_input_line(
    input: &str,
    cursor_pos: usize,
    placeholder: &str,
    text_style: Style,
    placeholder_style: Style,
    cursor_style: Style,
    area: Rect,
    buf: &mut Buffer,
) {
    if input.is_empty() {
        Paragraph::new(Line::from(Span::styled(placeholder, placeholder_style)))
            .render(area, buf);
    } else {
        // Show input with cursor indicator
        let display = if cursor_pos < input.len() {
            // Insert cursor character at position
            let mut s = String::with_capacity(input.len() + 1);
            s.push_str(&input[..cursor_pos]);
            s.push('|'); // cursor indicator
            s.push_str(&input[cursor_pos..]);
            s
        } else {
            format!("{}|", input)
        };

        Paragraph::new(Line::from(Span::styled(display, text_style)))
            .render(area, buf);
    }
}
