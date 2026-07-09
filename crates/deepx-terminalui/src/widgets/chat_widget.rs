//! Chat widget for DeepX TUI.
//!
//! Renders the conversation transcript as a scrollable list of messages.
//! Designed as a standalone ratatui [`Widget`] that consumes [`ChatMessage`] data.
//!
//! ## Architecture
//!
//! Inspired by Codex's ChatWidget but adapted for DeepX:
//! - Self-contained scroll state (offset, follow-bottom mode)
//! - Streaming support — re-renders incrementally as new tokens arrive
//! - Role-based coloring via [`Theme`]
//! - Consecutive messages of same role are visually grouped

use std::cmp;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::{ChatMessage, ChatRole, ToolStatus};
use crate::theme::Theme;

// ── ScrollState ─────────────────────────────────────────────────────────

/// Manages scroll position within the chat transcript.
#[derive(Clone, Debug, Default)]
pub struct ScrollState {
    /// Lines scrolled up from bottom. 0 = at bottom (follow mode).
    pub offset: usize,
    /// Auto-scroll to bottom on new content.
    pub follow_bottom: bool,
    /// Total rendered lines from the last build.
    pub total_lines: usize,
    /// Viewport height from the last render.
    pub viewport_height: usize,
}

impl ScrollState {
    pub fn new() -> Self {
        Self {
            offset: 0,
            follow_bottom: true,
            total_lines: 0,
            viewport_height: 0,
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.offset = self.offset.saturating_add(n);
        let max = self.total_lines.saturating_sub(self.viewport_height.max(1));
        if self.offset > max {
            self.offset = max;
        }
        self.follow_bottom = self.offset == 0;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
        self.follow_bottom = self.offset == 0;
    }

    pub fn scroll_page_up(&mut self, n: usize) {
        self.scroll_up(n);
    }

    pub fn scroll_page_down(&mut self, n: usize) {
        self.scroll_down(n);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.offset = 0;
        self.follow_bottom = true;
    }

    /// Called after building lines to clamp offset.
    pub fn update_viewport(&mut self, total: usize, viewport_h: usize) {
        self.total_lines = total;
        self.viewport_height = viewport_h;
        if self.follow_bottom {
            self.offset = 0;
        } else {
            let max = total.saturating_sub(viewport_h);
            self.offset = cmp::min(self.offset, max);
        }
    }
}

// ── ChatWidget ──────────────────────────────────────────────────────────

/// The main chat transcript widget.
///
/// Renders messages with role-based coloring, separators between
/// different roles, tool status badges, and scroll support.
pub struct ChatWidget<'a> {
    /// Messages to render.
    pub messages: &'a [ChatMessage],
    /// Whether the assistant is currently streaming.
    pub is_streaming: bool,
    /// Scroll state (mutated during rendering).
    pub scroll: &'a mut ScrollState,
    /// Theme for styling.
    pub theme: &'a Theme,
    /// Whether to show thinking text.
    pub show_thinking: bool,
}

impl<'a> ChatWidget<'a> {
    pub fn new(
        messages: &'a [ChatMessage],
        scroll: &'a mut ScrollState,
        theme: &'a Theme,
    ) -> Self {
        Self {
            messages,
            is_streaming: false,
            scroll,
            theme,
            show_thinking: true,
        }
    }

    /// Build all rendered lines from messages.
    fn build_lines(&self, body_width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(self.messages.len() * 3);
        let mut prev_role: Option<ChatRole> = None;

        let width = body_width.saturating_sub(2).max(20) as usize;

        for msg in self.messages {
            // Insert divider between different roles
            if let Some(pr) = prev_role {
                if pr != msg.role
                    && pr != ChatRole::Divider
                    && msg.role != ChatRole::Divider
                    && pr != ChatRole::Status
                    && msg.role != ChatRole::Status
                {
                    let div_len = width.min(60);
                    lines.push(Line::from(Span::styled(
                        format!(" {}", "\u{2500}".repeat(div_len)),
                        self.theme.divider,
                    )));
                }
            }
            if msg.role != ChatRole::Divider && msg.role != ChatRole::Status {
                prev_role = Some(msg.role);
            }

            match msg.role {
                ChatRole::Divider => {
                    let div_len = width.min(60);
                    lines.push(Line::from(Span::styled(
                        format!(" {}", "\u{2500}".repeat(div_len)),
                        self.theme.divider,
                    )));
                }
                ChatRole::Status => {
                    lines.push(Line::from(Span::styled(
                        msg.content.clone(),
                        self.theme.error_message,
                    )));
                }
                ChatRole::User => {
                    for uline in Self::wrap_text_owned(&msg.content, width) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", uline),
                            self.theme.user_message,
                        )));
                    }
                }
                ChatRole::Thinking => {
                    if self.show_thinking {
                        for tline in msg.content.lines() {
                            lines.push(Line::from(Span::styled(
                                format!("  {}", tline),
                                self.theme.thinking_text,
                            )));
                        }
                    }
                }
                ChatRole::Assistant => {
                    for aline in Self::wrap_text_owned(&msg.content, width) {
                        lines.push(Line::from(Span::styled(
                            aline,
                            self.theme.assistant_message,
                        )));
                    }
                }
                ChatRole::Tool => {
                    let (badge, badge_style) = match msg.tool_status {
                        ToolStatus::Pending => {
                            ("...", Style::new().fg(Color::Yellow))
                        }
                        ToolStatus::Success => {
                            ("OK", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD))
                        }
                        ToolStatus::Failed => {
                            ("ERR", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD))
                        }
                        ToolStatus::None => ("", self.theme.tool_message),
                    };

                    let label = if msg.tool_label.is_empty() {
                        msg.content.clone()
                    } else {
                        msg.tool_label.clone()
                    };

                    lines.push(Line::from(vec![
                        Span::styled(format!("  [{}]", badge), badge_style),
                        Span::styled(format!(" {}", label), self.theme.tool_message),
                    ]));

                    // Show tool output (non-empty content beyond label)
                    let content_not_label = msg.content != msg.tool_label
                        && !msg.tool_label.is_empty();
                    if !msg.content.is_empty() && content_not_label {
                        for tline in msg.content.lines() {
                            let truncated = if tline.len() > 200 {
                                let end = tline.floor_char_boundary(200);
                                tline[..end].to_string()
                            } else {
                                tline.to_string()
                            };
                            lines.push(Line::from(Span::styled(
                                format!("    {}", truncated),
                                Style::new().fg(Color::Rgb(140, 140, 150)),
                            )));
                        }
                    }
                }
            }
        }

        lines
    }

    /// Word-wrap, returning owned Strings. Byte-safe for CJK.
    pub fn wrap_text_owned(text: &str, width: usize) -> Vec<String> {
        let width = width.max(1);
        let mut result = Vec::new();
        for paragraph in text.split('\n') {
            if paragraph.is_empty() {
                result.push(String::new());
                continue;
            }
            let mut current = String::with_capacity(width);
            let mut char_count = 0;
            for word in paragraph.split_inclusive(|c: char| c.is_whitespace()) {
                let wc = word.chars().count();
                if char_count + wc > width && char_count > 0 {
                    result.push(current.trim_end().to_string());
                    current.clear();
                    char_count = 0;
                    // Strip leading space from next word
                    let trimmed = word.trim_start();
                    let tc = trimmed.chars().count();
                    current.push_str(trimmed);
                    char_count = tc;
                } else {
                    current.push_str(word);
                    char_count += wc;
                }
            }
            if !current.is_empty() {
                result.push(current.trim_end().to_string());
            }
        }
        result
    }
}

impl Widget for ChatWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let lines: Vec<Line<'_>> = self.build_lines(area.width);
        let total = lines.len();
        let viewport_h = area.height as usize;

        self.scroll.update_viewport(total, viewport_h);

        let skip = cmp::min(self.scroll.offset, total);
        let visible: Vec<Line> = lines.into_iter().skip(skip).collect();

        let text = Text::from(visible);
        let para = Paragraph::new(text);
        para.render(area, buf);
    }
}
