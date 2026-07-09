//! Chat widget using HistoryCell trait — each message is a separate Renderable.
//!
//! Inspired by Codex's ChatWidget:
//! - Past messages render as a column of `HistoryCell` implementations
//! - The active (streaming) message renders via `ActiveCell`
//! - Scroll is managed internally

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::{ChatMessage, ChatRole, ToolStatus};
use crate::render::renderable::{to_item, ColumnRenderable, FlexRenderable, Renderable};
use crate::theme::Theme;

/// Trait for rendering a single chat message cell.
pub trait HistoryCell {
    fn render_cell(&self, area: Rect, buf: &mut Buffer, theme: &Theme, width: u16);
    fn cell_height(&self, width: u16) -> u16;
}

// ── HistoryCell implementations ─────────────────────────────────────────

impl HistoryCell for ChatMessage {
    fn render_cell(&self, area: Rect, buf: &mut Buffer, theme: &Theme, _width: u16) {
        match self.role {
            ChatRole::User => render_user_cell(self, area, buf, theme),
            ChatRole::Assistant => render_assistant_cell(self, area, buf, theme),
            ChatRole::Thinking => render_thinking_cell(self, area, buf, theme),
            ChatRole::Tool => render_tool_cell(self, area, buf, theme),
            ChatRole::Status => render_status_cell(self, area, buf, theme),
            ChatRole::Divider => render_divider_cell(self, area, buf, theme),
        }
    }

    fn cell_height(&self, width: u16) -> u16 {
        let w = width.saturating_sub(2).max(20) as usize;
        match self.role {
            ChatRole::Divider => 1,
            ChatRole::Status => 1,
            ChatRole::Tool => (self.content.lines().count() + 1) as u16,
            ChatRole::User | ChatRole::Assistant => {
                crate::widgets::chat_widget::ChatWidget::wrap_text_owned(
                    &self.content, w
                ).len() as u16
            }
            ChatRole::Thinking => self.content.lines().count() as u16,
        }
    }
}

// ── Cell renderers ──────────────────────────────────────────────────────

fn render_user_cell(msg: &ChatMessage, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let bg = Style::new().bg(Color::Rgb(55, 55, 65));
    let indent = Span::styled("  ", theme.user_message);
    let text = Span::styled(&msg.content, theme.user_message.bg(bg.bg.unwrap_or(Color::Reset)));
    Paragraph::new(Line::from(vec![indent, text])).render(area, buf);
}

fn render_assistant_cell(msg: &ChatMessage, area: Rect, buf: &mut Buffer, theme: &Theme) {
    // Word-wrap and render
    let wrapped = crate::widgets::chat_widget::ChatWidget::wrap_text_owned(
        &msg.content,
        area.width.saturating_sub(2).max(20) as usize,
    );
    let lines: Vec<Line> = wrapped
        .into_iter()
        .map(|s| Line::from(Span::styled(s, theme.assistant_message)))
        .collect();
    Paragraph::new(Text::from(lines)).render(area, buf);
}

fn render_thinking_cell(msg: &ChatMessage, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let lines: Vec<Line> = msg
        .content
        .lines()
        .map(|l| Line::from(Span::styled(format!("  {}", l), theme.thinking_text)))
        .collect();
    Paragraph::new(Text::from(lines)).render(area, buf);
}

fn render_tool_cell(msg: &ChatMessage, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let (badge, badge_style) = match msg.tool_status {
        ToolStatus::Pending => ("...", Style::new().fg(Color::Yellow)),
        ToolStatus::Success => ("OK", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ToolStatus::Failed => ("ERR", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ToolStatus::None => ("", theme.tool_message),
    };

    let label = if msg.tool_label.is_empty() {
        &msg.content
    } else {
        &msg.tool_label
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("  [{}]", badge), badge_style),
        Span::styled(format!(" {}", label), theme.tool_message),
    ]));

    if !msg.content.is_empty() && msg.content != msg.tool_label {
        for tline in msg.content.lines().take(5) {
            let truncated = if tline.len() > 200 {
                tline[..tline.floor_char_boundary(200)].to_string()
            } else {
                tline.to_string()
            };
            lines.push(Line::from(Span::styled(format!("    {}", truncated), Style::new().fg(Color::Rgb(140, 140, 150)))));
        }
    }

    Paragraph::new(Text::from(lines)).render(area, buf);
}

fn render_status_cell(msg: &ChatMessage, area: Rect, buf: &mut Buffer, theme: &Theme) {
    Paragraph::new(Line::from(Span::styled(&msg.content, theme.error_message)))
        .render(area, buf);
}

fn render_divider_cell(_msg: &ChatMessage, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let div = "\u{2500}".repeat(area.width.min(60) as usize);
    Paragraph::new(Line::from(Span::styled(div, theme.divider)))
        .render(area, buf);
}

// ── New ChatWidget ──────────────────────────────────────────────────────

/// Codex-style chat widget with HistoryCell architecture.
pub struct ChatWidgetV2<'a> {
    pub messages: &'a [ChatMessage],
    pub scroll_offset: usize,
    pub theme: &'a Theme,
    pub show_thinking: bool,
}

impl Renderable for ChatWidgetV2<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Build a ColumnRenderable of all messages
        let mut col = ColumnRenderable::new();
        let width = area.width;

        for msg in self.messages {
            if msg.role == ChatRole::Thinking && !self.show_thinking {
                continue;
            }
            let h = msg.cell_height(width);
            if h == 0 {
                continue;
            }
            col.push(to_item(CellWidget { msg, theme: self.theme }));
        }

        let total_h = col.desired_height(width);
        let viewport = area.height;

        // Apply scroll offset
        let skip = self.scroll_offset.min(total_h.saturating_sub(viewport) as usize);

        // Render only visible portion manually
        let mut y = area.y;
        let mut rendered = 0usize;
        for msg in self.messages {
            if msg.role == ChatRole::Thinking && !self.show_thinking {
                continue;
            }
            let h = msg.cell_height(width) as usize;
            if h == 0 { continue; }
            if rendered < skip {
                rendered += h;
                continue;
            }
            if y >= area.y + area.height {
                break;
            }
            let cell_h = h.min((area.y + area.height - y) as usize) as u16;
            let cell_area = Rect { x: area.x, y, width: area.width, height: cell_h };
            msg.render_cell(cell_area, buf, self.theme, width);
            rendered += h;
            y += cell_h;
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        let mut total = 0u16;
        for msg in self.messages {
            if msg.role == ChatRole::Thinking && !self.show_thinking {
                continue;
            }
            total += msg.cell_height(width);
        }
        total
    }
}

/// Adapter: wraps ChatMessage into a Renderable via HistoryCell.
struct CellWidget<'a> {
    msg: &'a ChatMessage,
    theme: &'a Theme,
}

impl Renderable for CellWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.msg.render_cell(area, buf, self.theme, area.width);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.msg.cell_height(width)
    }
}
