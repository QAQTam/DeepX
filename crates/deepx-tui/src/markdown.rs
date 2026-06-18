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
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthStr;

const CODE_BG: Color = Color::Rgb(30, 34, 38);
const CODE_FG: Color = Color::Rgb(200, 200, 200);
const TABLE_BORDER: Color = Color::Rgb(60, 70, 80);
const HEADING_COLOR: Color = Color::Rgb(180, 150, 255);
const QUOTE_FG: Color = Color::Rgb(140, 160, 140);
const LINK_FG: Color = Color::Rgb(100, 200, 255);

// ── Public convenience ──

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
    /// True when a separator row (`|---|`) has been seen after the first
    /// data row — confirms this is a real table, not accidental pipe chars.
    table_confirmed: bool,
    /// The first tentative pipe line, saved in case it turns out NOT to be a table.
    /// Only used when table_rows has 1 entry and table_confirmed is false.
    tentative_first: Option<String>,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            state: MdState::Normal,
            code_lines: Vec::new(),
            code_lang: String::new(),
            table_rows: Vec::new(),
            table_confirmed: false,
            tentative_first: None,
        }
    }

    /// Push a raw line.  Returns rendered Lines (may be empty for
    /// lines consumed internally, e.g. code-block content).
    pub fn push_line(&mut self, line: &str) -> Vec<Line<'static>> {
        if self.state == MdState::CodeBlock && line.trim().starts_with("```") {
            self.state = MdState::Normal;
            let lns = std::mem::take(&mut self.code_lines);
            let lang = std::mem::take(&mut self.code_lang);
            return render_code_block(&lns, &lang);
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
            let lang = std::mem::take(&mut self.code_lang);
            lines.append(&mut render_code_block(&lns, &lang));
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
            let mut out = self.flush_table();
            if self.state == MdState::CodeBlock {
                self.state = MdState::Normal;
                let lns = std::mem::take(&mut self.code_lines);
                let lang = std::mem::take(&mut self.code_lang);
                out.extend(render_code_block(&lns, &lang));
                return out;
            } else {
                self.state = MdState::CodeBlock;
                self.code_lang = trimmed.strip_prefix("```").unwrap_or("").trim().to_string();
                return out;
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
            let text = trimmed.strip_prefix('>').unwrap_or(trimmed);
            return vec![Line::from(vec![
                Span::styled("│ ", Style::new().fg(QUOTE_FG)),
                Span::styled(text.to_string(), Style::new().fg(QUOTE_FG).italic()),
            ])];
        }

        // Task list (must check before unordered list)
        if let Some(text) = trimmed.strip_prefix("- [x] ").or_else(|| trimmed.strip_prefix("- [X] ")) {
            return vec![Line::from(vec![
                Span::styled("  ☑ ", Style::new().fg(Color::Rgb(100, 200, 120))),
                Span::styled(text.to_string(), Style::new().fg(Color::Rgb(160, 180, 160))),
            ])];
        }
        if let Some(text) = trimmed.strip_prefix("- [ ] ") {
            return vec![Line::from(vec![
                Span::styled("  ☐ ", Style::new().fg(Color::Rgb(140, 150, 160))),
                Span::styled(text.to_string(), Style::new().fg(Color::Rgb(180, 190, 200))),
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

        // Inline table (pipe syntax) — two-line lookahead to avoid false positives
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.matches('|').count() >= 2 {
            let cells = parse_table_row(trimmed);
            let is_sep = cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '));
            if is_sep {
                if !self.table_rows.is_empty() {
                    // Separator after data row → table confirmed
                    self.table_confirmed = true;
                }
                // Always discard separator rows — widths are computed from data
                return Vec::new();
            }
            // Save the raw first line in case this turns out not to be a table
            if self.table_rows.is_empty() && !self.table_confirmed {
                self.tentative_first = Some(line.to_string());
            }
            self.table_rows.push(cells);
            return Vec::new();
        }

        // Non-table line: flush buffered table (if confirmed) or roll back
        let mut out = if self.table_confirmed || self.table_rows.len() >= 2 {
            self.flush_table()
        } else if self.table_rows.len() == 1 {
            // Single pipe line without separator — false positive.  Render as plain text.
            let fallback = self.tentative_first.take().unwrap_or_default();
            self.table_rows.clear();
            self.table_confirmed = false;
            vec![format_inline(&fallback)]
        } else {
            Vec::new()
        };

        // Plain paragraph
        out.push(format_inline(trimmed));
        out
    }

    // ── Code block mode ──

    fn flush_table(&mut self) -> Vec<Line<'static>> {
        if self.table_rows.is_empty() { return Vec::new(); }
        let rows = std::mem::take(&mut self.table_rows);
        self.table_confirmed = false;
        self.tentative_first = None;
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
        out.push(render_table_top_separator(&widths));
        // Header row (first data row)
        if let Some(header) = rows.first() {
            out.push(render_table_row(header, &widths));
            out.push(render_table_mid_separator(&widths));
        }
        // Body rows
        for row in rows.iter().skip(1) {
            out.push(render_table_row(row, &widths));
        }
        out.push(render_table_bottom_separator(&widths));
        out
    }

    fn push_code_line(&mut self, line: &str) -> Vec<Line<'static>> {
        self.code_lines.push(line.to_string());
        Vec::new()
    }
}

// ── Syntax highlighting ──

fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static syntect::highlighting::Theme {
    static TH: OnceLock<syntect::highlighting::Theme> = OnceLock::new();
    TH.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes.get("base16-eighties.dark")
            .cloned()
            .unwrap_or_else(|| ts.themes.values().next().cloned().unwrap())
    })
}

fn resolve_syntax(lang: &str) -> &'static syntect::parsing::SyntaxReference {
    let ss = syntax_set();
    let lang_lower = lang.trim().to_lowercase();
    match lang_lower.as_str() {
        "rs" | "rust" => ss.find_syntax_by_extension("rs"),
        "py" | "python" => ss.find_syntax_by_extension("py"),
        "js" | "javascript" => ss.find_syntax_by_extension("js"),
        "ts" | "typescript" => ss.find_syntax_by_extension("ts"),
        "go" => ss.find_syntax_by_extension("go"),
        "c" => ss.find_syntax_by_extension("c"),
        "cpp" | "c++" => ss.find_syntax_by_extension("cpp"),
        "h" | "hpp" => ss.find_syntax_by_extension("h"),
        "java" => ss.find_syntax_by_extension("java"),
        "sh" | "bash" | "shell" | "zsh" => ss.find_syntax_by_extension("sh"),
        "json" => ss.find_syntax_by_extension("json"),
        "yaml" | "yml" => ss.find_syntax_by_extension("yaml"),
        "toml" => ss.find_syntax_by_extension("toml"),
        "md" | "markdown" => ss.find_syntax_by_extension("md"),
        "sql" => ss.find_syntax_by_extension("sql"),
        "html" => ss.find_syntax_by_extension("html"),
        "css" => ss.find_syntax_by_extension("css"),
        "xml" => ss.find_syntax_by_extension("xml"),
        "diff" | "patch" => ss.find_syntax_by_extension("diff"),
        "proto" | "protobuf" => ss.find_syntax_by_extension("proto"),
        "lua" => ss.find_syntax_by_extension("lua"),
        "rb" | "ruby" => ss.find_syntax_by_extension("rb"),
        "php" => ss.find_syntax_by_extension("php"),
        "swift" => ss.find_syntax_by_extension("swift"),
        "kt" | "kotlin" => ss.find_syntax_by_extension("kt"),
        "scala" => ss.find_syntax_by_extension("scala"),
        "r" => ss.find_syntax_by_extension("r"),
        "dart" => ss.find_syntax_by_extension("dart"),
        "elm" => ss.find_syntax_by_extension("elm"),
        "erl" | "erlang" => ss.find_syntax_by_extension("erl"),
        "hs" | "haskell" => ss.find_syntax_by_extension("hs"),
        "clj" | "clojure" => ss.find_syntax_by_extension("clj"),
        "ex" | "elixir" => ss.find_syntax_by_extension("ex"),
        "zig" => ss.find_syntax_by_extension("zig"),
        "nim" => ss.find_syntax_by_extension("nim"),
        "dockerfile" | "docker" => ss.find_syntax_by_name("Dockerfile"),
        "makefile" | "make" => ss.find_syntax_by_name("Makefile"),
        "cmake" => ss.find_syntax_by_name("CMake"),
        "ini" | "cfg" | "conf" => ss.find_syntax_by_extension("ini"),
        "nix" => ss.find_syntax_by_extension("nix"),
        _ => ss.find_syntax_by_extension(&lang_lower),
    }
    .unwrap_or_else(|| ss.find_syntax_plain_text())
}

fn syntect_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
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

fn table_style() -> Style {
    Style::new().fg(Color::Rgb(80, 85, 95))
}

/// Build a separator line from ratatui's built-in box-drawing symbol set.
fn render_table_sep(col_widths: &[usize], left: &str, mid: &str, right: &str) -> Line<'static> {
    let s = table_style();
    let line = ratatui::symbols::line::NORMAL;
    let mut spans = vec![Span::styled(left.to_string(), s)];
    for (i, w) in col_widths.iter().enumerate() {
        spans.push(Span::styled(line.horizontal.repeat(w + 2), s));
        if i + 1 < col_widths.len() {
            spans.push(Span::styled(mid.to_string(), s));
        }
    }
    spans.push(Span::styled(right.to_string(), s));
    Line::from(spans)
}

fn render_table_top_separator(col_widths: &[usize]) -> Line<'static> {
    let s = ratatui::symbols::line::NORMAL;
    render_table_sep(col_widths, s.top_left, s.horizontal_down, s.top_right)
}

fn render_table_mid_separator(col_widths: &[usize]) -> Line<'static> {
    let s = ratatui::symbols::line::NORMAL;
    render_table_sep(col_widths, s.vertical_right, s.cross, s.vertical_left)
}

fn render_table_bottom_separator(col_widths: &[usize]) -> Line<'static> {
    let s = ratatui::symbols::line::NORMAL;
    render_table_sep(col_widths, s.bottom_left, s.horizontal_up, s.bottom_right)
}

fn render_table_row(cells: &[String], col_widths: &[usize]) -> Line<'static> {
    let v = ratatui::symbols::line::NORMAL.vertical;
    let border_style = Style::new().fg(Color::Rgb(140, 145, 155));
    let cell_style = Style::new().fg(Color::Rgb(200, 200, 210));
    let mut spans = vec![Span::styled(format!("{v} "), border_style)];
    for (i, cell) in cells.iter().enumerate() {
        let w = col_widths.get(i).copied().unwrap_or(8);
        let cw = cell.width();
        let pad = " ".repeat(w.saturating_sub(cw));
        spans.push(Span::styled(format!("{cell}{pad} "), cell_style));
        spans.push(Span::styled(format!("{v} "), border_style));
    }
    Line::from(spans)
}

// ── Code block rendering ──

fn render_code_block(lines: &[String], lang: &str) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(""));
    if !lang.is_empty() {
        out.push(Line::from(Span::styled(
            format!("  {} {} ", "─".repeat(2), lang),
            Style::new().fg(Color::Rgb(100, 100, 110)),
        )));
    }
    let syntax = resolve_syntax(lang);
    let theme = theme();
    let mut highlighter = HighlightLines::new(syntax, theme);
    for line in lines {
        let expanded = line.replace('\t', "    ");
        let ranges = highlighter.highlight_line(&expanded, syntax_set());
        match ranges {
            Ok(ranges) => {
                let mut line_spans = vec![Span::raw("  ")];
                for (style, text) in ranges {
                    line_spans.push(Span::styled(
                        text.to_string(),
                        Style::new()
                            .fg(syntect_color(style.foreground))
                            .bg(CODE_BG),
                    ));
                }
                out.push(Line::from(line_spans));
            }
            Err(_) => {
                out.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(expanded, Style::new().fg(CODE_FG).bg(CODE_BG)),
                ]));
            }
        }
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
        // Links MUST be extracted before ** and * so that [**bold**](url)
        // renders as a styled link, not orphan brackets.
        if let Some(rest) = try_extract_link(&remaining, &mut spans) {
            remaining = rest;
            continue;
        }
        if let Some(rest) = try_extract(&remaining, "**", &mut spans, true) {
            remaining = rest;
            continue;
        }
        if let Some(rest) = try_extract_italic(&remaining, &mut spans) {
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
        let next = remaining.find(|c| c == '*' || c == '`' || c == '~' || c == '[').unwrap_or(remaining.len());
        if next > 0 {
            spans.push(Span::raw(remaining[..next].to_string()));
            remaining = remaining[next..].to_string();
        } else if !remaining.is_empty() {
            let p = remaining.floor_char_boundary(1);
            spans.push(Span::raw(remaining[..p].to_string()));
            remaining = remaining[p..].to_string();
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
    if inner.contains('\n') { return None; }
    let rest = after_start[end + marker.len()..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    let style = if bold { Style::new().bold() } else { Style::new().italic() };
    spans.push(Span::styled(inner.to_string(), style));
    Some(rest)
}

/// Like `try_extract` for `*` but skips `**` pairs (bold) when searching
/// for the closing `*`, so that `*italic **bold** text*` works correctly.
fn try_extract_italic(text: &str, spans: &mut Vec<Span<'static>>) -> Option<String> {
    let start = text.find('*')?;
    // If the opening `*` is followed by another `*`, this is bold territory —
    // let try_extract("**") handle it.
    if text[start..].starts_with("**") {
        return None;
    }
    let prefix = &text[..start];
    let after = &text[start + 1..];
    // Search for closing `*`, skipping `**` pairs
    let mut pos = 0;
    let closing = loop {
        let found = after[pos..].find('*')?;
        let abs = pos + found;
        // If this `*` is part of `**`, skip the pair
        if after[abs..].starts_with("**") {
            pos = abs + 2;
            continue;
        }
        break abs;
    };
    let inner = &after[..closing];
    if inner.contains('\n') { return None; }
    let rest = after[closing + 1..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    spans.push(Span::styled(inner.to_string(), Style::new().italic()));
    Some(rest)
}

fn try_extract_strikethrough(text: &str, spans: &mut Vec<Span<'static>>) -> Option<String> {
    let start = text.find("~~")?;
    let prefix = &text[..start];
    let after = &text[start + 2..];
    let end = after.find("~~")?;
    let inner = &after[..end];
    if inner.contains('\n') { return None; }
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
    if inner.contains('\n') { return None; }
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
    if label.contains('\n') { return None; }
    let after_label = &rest[label_end + 1..];
    if !after_label.starts_with('(') {
        return None;
    }
    let after_paren = after_label.strip_prefix('(').unwrap_or("");
    let url_end = after_paren.find(')')?;
    let url = &after_paren[..url_end];
    let remaining = after_paren[url_end + 1..].to_string();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    spans.push(Span::styled(label.to_string(), Style::new().fg(LINK_FG).underlined()));
    spans.push(Span::styled(format!(" ({url})"), Style::new().fg(Color::Gray)));
    Some(remaining)
}
