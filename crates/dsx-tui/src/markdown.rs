//! Streaming markdown renderer — line-by-line classification + inline styling.
//!
//! Each time a new line of text arrives, it is classified and rendered into
//! ratatui::text::Line.  Code blocks and tables accumulate state until
//! their closing delimiter, then flush as styled output.
//!
//! Design principle: lines are the atomic unit.  No block-level lookahead —
//! a table row is rendered immediately, not held until the table ends.
//! This gives instant visual feedback during streaming.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

const CODE_BG: Color = Color::Rgb(30, 34, 38);
const CODE_FG: Color = Color::Rgb(200, 200, 200);
const TABLE_BORDER: Color = Color::Rgb(60, 70, 80);
const HEADING_COLOR: Color = Color::Rgb(180, 150, 255);
const QUOTE_FG: Color = Color::Rgb(140, 160, 140);
const LINK_FG: Color = Color::Rgb(100, 200, 255);

// ── Public convenience ──

/// Render a complete markdown string into ratatui Lines.
pub fn render_content(content: &str) -> Vec<Line<'static>> {
    let mut renderer = MarkdownRenderer::new();
    let mut lines = Vec::new();
    for line in content.lines() {
        for l in renderer.push_line(line) {
            lines.push(l);
        }
    }
    for l in renderer.flush() {
        lines.push(l);
    }
    lines
}

// ── State machine ──

#[derive(Debug, Clone, Copy, PartialEq)]
enum MdState {
    Normal,
    CodeBlock,
}

pub struct MarkdownRenderer {
    state: MdState,
    code_lines: Vec<String>,
    code_lang: String,
    table_rows: Vec<Vec<String>>,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            state: MdState::Normal,
            code_lines: Vec::new(),
            code_lang: String::new(),
            table_rows: Vec::new(),
        }
    }

    /// Push a raw line.  Returns rendered Lines (may be empty for
    /// lines consumed internally, e.g. code-block content).
    pub fn push_line(&mut self, line: &str) -> Vec<Line<'static>> {
        if self.state == MdState::CodeBlock && line.trim().starts_with("```") {
            self.state = MdState::Normal;
            let lns = std::mem::take(&mut self.code_lines);
            self.code_lang.clear();
            return render_code_block(&lns);
        }
        match self.state {
            MdState::CodeBlock => self.push_code_line(line),
            MdState::Normal => self.push_normal_line(line),
        }
    }

    /// Flush any pending state (e.g. an open code block at end of stream).
    pub fn flush(&mut self) -> Vec<Line<'static>> {
        let mut lines = self.flush_table();
        if self.state == MdState::CodeBlock {
            self.state = MdState::Normal;
            let lns = std::mem::take(&mut self.code_lines);
            self.code_lang.clear();
            lines.append(&mut render_code_block(&lns));
        }
        lines
    }

    // ── Normal mode ──

    fn push_normal_line(&mut self, line: &str) -> Vec<Line<'static>> {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            return vec![Line::from("")];
        }

        // ``` fence → start / end code block
        if trimmed.starts_with("```") {
            if self.state == MdState::CodeBlock {
                self.state = MdState::Normal;
                let lns = std::mem::take(&mut self.code_lines);
                self.code_lang.clear();
                return render_code_block(&lns);
            } else {
                self.state = MdState::CodeBlock;
                self.code_lang = trimmed[3..].trim().to_string();
                return Vec::new();
            }
        }

        // Heading
        if let Some(n) = heading_level(trimmed) {
            let text = trimmed[n as usize..].trim();
            return vec![
                Line::from(Span::styled(text.to_string(), Style::new().fg(HEADING_COLOR).bold())),
                Line::from(""),
            ];
        }

        // Blockquote
        if trimmed.starts_with('>') {
            let text = &trimmed[1..];
            return vec![Line::from(vec![
                Span::styled("│ ", Style::new().fg(QUOTE_FG)),
                Span::styled(text.to_string(), Style::new().fg(QUOTE_FG).italic()),
            ])];
        }

        // Unordered list
        if let Some(text) = trimmed.strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            return vec![format_inline(&format!("  • {text}"))];
        }

        // Ordered list
        if let Some(text) = strip_ordered_prefix(trimmed) {
            let num = ordered_number(trimmed);
            return vec![format_inline(&format!("  {num} {text}"))];
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            return vec![Line::from(Span::styled(
                "─────────────────────────────".to_string(),
                Style::new().fg(TABLE_BORDER),
            ))];
        }

        // Inline table (pipe syntax)
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            let cells = parse_table_row(trimmed);
            let is_sep = cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '));
            if is_sep {
                // Skipping separator — widths will be calculated from data rows
                return Vec::new();
            }
            self.table_rows.push(cells);
            return Vec::new();
        }

        // Non-table line: flush buffered table first
        let mut out = self.flush_table();
        // Plain paragraph
        out.push(format_inline(trimmed));
        out
    }

    // ── Code block mode ──

    fn flush_table(&mut self) -> Vec<Line<'static>> {
        if self.table_rows.is_empty() { return Vec::new(); }
        let rows = std::mem::take(&mut self.table_rows);
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; cols];
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                if i < cols {
                    widths[i] = widths[i].max(cell.width());
                }
            }
        }
        let mut out = Vec::new();
        // Separator line
        out.push(render_table_separator(&widths));
        for row in &rows {
            out.push(render_table_row(row, &widths));
        }
        out.push(render_table_separator(&widths));
        out
    }

    fn push_code_line(&mut self, line: &str) -> Vec<Line<'static>> {
        self.code_lines.push(line.to_string());
        Vec::new()
    }
}

// ── Line classification helpers ──

fn heading_level(line: &str) -> Option<u8> {
    let trimmed = line.trim();
    if trimmed.starts_with("######") { Some(6) }
    else if trimmed.starts_with("#####") { Some(5) }
    else if trimmed.starts_with("####") { Some(4) }
    else if trimmed.starts_with("###") { Some(3) }
    else if trimmed.starts_with("##") { Some(2) }
    else if trimmed.starts_with("#") { Some(1) }
    else { None }
}

fn strip_ordered_prefix(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    let dot = trimmed.find(". ")?;
    let prefix = &trimmed[..dot];
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        Some(&trimmed[dot + 2..])
    } else {
        None
    }
}

fn ordered_number(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(dot) = trimmed.find(". ") {
        format!("{}.", &trimmed[..dot])
    } else {
        String::new()
    }
}

// ── Table rendering (per-line instant) ──

fn parse_table_row(line: &str) -> Vec<String> {
    line.split('|')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

fn render_table_separator(col_widths: &[usize]) -> Line<'static> {
    let total: usize = col_widths.iter().map(|w| w + 3).sum::<usize>() + 1;
    let line: String = "─".repeat(total);
    Line::from(Span::styled(line, Style::new().fg(Color::Rgb(80, 85, 95))))
}

fn render_table_row(cells: &[String], col_widths: &[usize]) -> Line<'static> {
    let mut spans = vec![Span::styled("│ ".to_string(), Style::new().fg(Color::Rgb(140, 145, 155)))];
    for (i, cell) in cells.iter().enumerate() {
        let w = col_widths.get(i).copied().unwrap_or(8);
        let cw = cell.width();
        let pad = " ".repeat(w.saturating_sub(cw));
        let padded = format!("{cell}{pad} ");
        spans.push(Span::styled(padded, Style::new().fg(Color::Rgb(200, 200, 210))));
        spans.push(Span::styled("│ ".to_string(), Style::new().fg(Color::Rgb(140, 145, 155))));
    }
    Line::from(spans)
}

// ── Code block rendering ──

fn render_code_block(lines: &[String]) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(""));
    for line in lines {
        let l = line.replace('\t', "    ");
        out.push(Line::from(Span::styled(
            format!("  {l}"),
            Style::new().fg(CODE_FG).bg(CODE_BG),
        )));
    }
    out.push(Line::from(""));
    out
}

// ── Inline formatting ──

fn format_inline(text: &str) -> Line<'static> {
    if !text.contains('*') && !text.contains('`') && !text.contains("~~") && !text.contains("][") {
        return Line::from(Span::raw(text.to_string()));
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut remaining = text.to_string();

    while !remaining.is_empty() {
        if let Some(rest) = try_extract(&remaining, "**", &mut spans, true) {
            remaining = rest;
            continue;
        }
        if let Some(rest) = try_extract(&remaining, "*", &mut spans, false) {
            remaining = rest;
            continue;
        }
        if let Some(rest) = try_extract_strikethrough(&remaining, &mut spans) {
            remaining = rest;
            continue;
        }
        if let Some(rest) = try_extract_code(&remaining, &mut spans) {
            remaining = rest;
            continue;
        }
        if let Some(rest) = try_extract_link(&remaining, &mut spans) {
            remaining = rest;
            continue;
        }
        let next = remaining.find(|c| c == '*' || c == '`' || c == '~' || c == '[').unwrap_or(remaining.len());
        if next > 0 {
            spans.push(Span::raw(remaining[..next].to_string()));
            remaining = remaining[next..].to_string();
        } else if !remaining.is_empty() {
            spans.push(Span::raw(remaining[..1].to_string()));
            remaining = remaining[1..].to_string();
        }
    }

    Line::from(spans)
}

fn try_extract(text: &str, marker: &str, spans: &mut Vec<Span<'static>>, bold: bool) -> Option<String> {
    let start = text.find(marker)?;
    let prefix = &text[..start];
    let after_start = &text[start + marker.len()..];
    let end = after_start.find(marker)?;
    let inner = &after_start[..end];
    let rest = after_start[end + marker.len()..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    let style = if bold { Style::new().bold() } else { Style::new().italic() };
    spans.push(Span::styled(inner.to_string(), style));
    Some(rest)
}

fn try_extract_strikethrough(text: &str, spans: &mut Vec<Span<'static>>) -> Option<String> {
    let start = text.find("~~")?;
    let prefix = &text[..start];
    let after = &text[start + 2..];
    let end = after.find("~~")?;
    let inner = &after[..end];
    let rest = after[end + 2..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    spans.push(Span::styled(inner.to_string(), Style::new().crossed_out()));
    Some(rest)
}

fn try_extract_code(text: &str, spans: &mut Vec<Span<'static>>) -> Option<String> {
    let start = text.find('`')?;
    let prefix = &text[..start];
    let after = &text[start + 1..];
    let end = after.find('`')?;
    let inner = &after[..end];
    let rest = after[end + 1..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    spans.push(Span::styled(inner.to_string(), Style::new().fg(Color::Cyan)));
    Some(rest)
}

fn try_extract_link(text: &str, spans: &mut Vec<Span<'static>>) -> Option<String> {
    let start = text.find('[')?;
    let prefix = &text[..start];
    let rest = &text[start + 1..];
    let label_end = rest.find(']')?;
    let label = &rest[..label_end];
    let after_label = &rest[label_end + 1..];
    if !after_label.starts_with('(') {
        return None;
    }
    let url_end = after_label[1..].find(')')?;
    let url = &after_label[1..][..url_end];
    let remaining = after_label[url_end + 2..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    spans.push(Span::styled(label.to_string(), Style::new().fg(LINK_FG).underlined()));
    spans.push(Span::styled(format!(" ({url})"), Style::new().fg(Color::Gray)));
    Some(remaining)
}
