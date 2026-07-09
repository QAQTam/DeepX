//! Markdown widget — implements [`Renderable`] for markdown text.
//!
//! Wraps the existing pulldown-cmark + syntect renderer in a
//! [`Renderable`]-conformant widget that uses the [`Theme`] system.
//!
//! ## Architecture
//!
//! - `MarkdownWidget` accepts markdown text and renders via the existing
//!   `crate::markdown::render_markdown` function
//! - `AnsiWidget` renders ANSI escape sequences via `render_ansi`
//! - Both implement the [`Renderable`] trait for use in the new layout tree

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;

use crate::render::renderable::Renderable;

// ── MarkdownWidget ──────────────────────────────────────────────────────

/// Renders markdown text to a ratatui buffer via the existing render pipeline.
pub struct MarkdownWidget<'a> {
    /// Markdown source text.
    pub text: &'a str,
}

impl<'a> MarkdownWidget<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl Renderable for MarkdownWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = crate::markdown::render_markdown(self.text);
        let text = Text::from(lines);
        Paragraph::new(text).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let lines = crate::markdown::render_markdown(self.text);
        let count = lines.len() as u16;
        if width == 0 {
            return count;
        }
        // Estimate wrapped height: each line wraps at width chars
        let mut total = 0u16;
        for line in &lines {
            let line_width = line.width() as u16;
            total += (line_width.saturating_add(width.saturating_sub(1))) / width.max(1);
        }
        total.max(1)
    }
}

// ── DiffWidget ──────────────────────────────────────────────────────────

/// Renders a unified diff with +/- coloring.
pub struct DiffWidget<'a> {
    pub text: &'a str,
}

impl Renderable for DiffWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = crate::markdown::render_diff(self.text)
            .unwrap_or_else(|| {
                crate::markdown::render_markdown(self.text)
            });
        let text = Text::from(lines);
        Paragraph::new(text).render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.text.lines().count() as u16
    }
}

// ── AnsiWidget ──────────────────────────────────────────────────────────

/// Renders ANSI escape code text (terminal output from exec).
pub struct AnsiWidget<'a> {
    pub text: &'a str,
}

impl Renderable for AnsiWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = crate::markdown::render_ansi(self.text);
        let text = Text::from(lines);
        Paragraph::new(text).render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.text.lines().count() as u16
    }
}

// ── Parse diff rows ─────────────────────────────────────────────────────

/// Parse a unified diff into structured rows (re-exported from markdown).
pub use crate::markdown::parse_diff_rows;

/// Check if text is a unified diff.
pub fn is_unified_diff(text: &str) -> bool {
    let has_header = text.lines().any(|l| l.starts_with("--- ") || l.starts_with("+++ "));
    let has_hunk = text.lines().any(|l| l.starts_with("@@"));
    has_header && has_hunk
}
