//! Markdown renderer — pulldown-cmark backed, event-driven ratatui output.
//!
//! `render_markdown(text)` parses the full markdown string and returns
//! styled ratatui Lines.  For streaming scenarios, simply call it every
//! frame with the accumulated text — pulldown-cmark handles incomplete
//! constructs gracefully and is fast enough for 10s of KB.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthStr;

const CODE_BG: Color = Color::Rgb(30, 34, 38);
const HEADING_COLOR: Color = Color::Rgb(180, 150, 255);
const QUOTE_FG: Color = Color::Rgb(140, 160, 140);
const LINK_FG: Color = Color::Rgb(100, 200, 255);
const TABLE_BORDER: Color = Color::Rgb(60, 70, 80);

// ── Public API ──

/// Render markdown text to ratatui Lines.
/// Panics from the parser/renderer are caught and returned as a plain-text error line.
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::all();
    opts.remove(Options::ENABLE_SMART_PUNCTUATION); // avoid Unicode smart-quotes breaking code
    let result = catch_unwind(AssertUnwindSafe(|| {
        let parser = Parser::new_ext(text, opts);
        MdRenderer::new().render(parser)
    }));
    match result {
        Ok(lines) => lines,
        Err(_) => {
            // Fallback: render as plain text with error indicator
            let mut lines: Vec<Line<'static>> = Vec::new();
            lines.push(Line::from(Span::styled(
                "⚠ markdown render error — showing raw text:",
                Style::new().fg(Color::Red).bold(),
            )));
            for line in text.lines() {
                lines.push(Line::from(Span::raw(line.to_string())));
            }
            lines
        }
    }
}

// ── Diff rendering ──

/// Detect if text is a unified diff and render with color-coded +/- lines.
pub fn render_diff(text: &str) -> Option<Vec<Line<'static>>> {
    if !is_unified_diff(text) {
        return None;
    }
    Some(render_diff_lines(text))
}

fn is_unified_diff(text: &str) -> bool {
    // Must have at least one --- a/ or +++ b/ header line, or an @@ hunk header
    let has_header = text.lines().any(|l| l.starts_with("--- ") || l.starts_with("+++ "));
    let has_hunk = text.lines().any(|l| l.starts_with("@@"));
    has_header && has_hunk
}

fn render_diff_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut old_ln: u32 = 0;
    let mut new_ln: u32 = 0;
    let mut file_hdr: Option<String> = None;
    let mut started = false;

    for line in text.lines() {
        // Capture file header
        if line.starts_with("--- ") {
            file_hdr = Some(line[4..].to_string());
            continue;
        }
        if line.starts_with("+++ ") { continue; }

        // Parse @@ hunk header to initialize line counters
        if line.starts_with("@@") {
            // @@ -old_start[,old_count] +new_start[,new_count] @@
            if let Some(rest) = line.strip_prefix("@@ -") {
                if let Some((old_part, rest2)) = rest.split_once(' ') {
                    // rest2 is "+new_start..." — strip leading +
                    let new_part = rest2.strip_prefix('+').unwrap_or(rest2);
                    old_ln = old_part.split(',').next().and_then(|s| s.parse().ok()).unwrap_or(old_ln);
                    new_ln = new_part.split(',').next().and_then(|s| s.parse().ok()).unwrap_or(new_ln);
                }
            }
            if old_ln > 0 { old_ln -= 1; } // @@ starts at the NEXT line
            if new_ln > 0 { new_ln -= 1; }
            started = true;

            // Show file header before first hunk
            if let Some(ref hdr) = file_hdr.take() {
                lines.push(Line::from(Span::styled(
                    format!(" {} ", hdr),
                    Style::new().fg(Color::Rgb(180, 180, 100)).bg(Color::Rgb(30, 30, 20)),
                )));
            }
            continue;
        }

        if !started { continue; }

        let (old_str, new_str, cls) = if line.starts_with('-') {
            old_ln += 1;
            (format!("{}", old_ln), String::new(), "del")
        } else if line.starts_with('+') {
            new_ln += 1;
            (String::new(), format!("{}", new_ln), "add")
        } else {
            old_ln += 1;
            new_ln += 1;
            (format!("{}", old_ln), format!("{}", new_ln), "ctx")
        };
        let body = &line[if line.starts_with('-') || line.starts_with('+') { 1 } else { 0 }..];

        let (fg, bg) = match cls {
            "del" => (Color::Rgb(255, 140, 140), Color::Rgb(50, 20, 20)),
            "add" => (Color::Rgb(140, 255, 140), Color::Rgb(20, 50, 20)),
            _ => (Color::Rgb(200, 210, 220), Color::Rgb(24, 28, 32)),
        };
        let dim = Color::Rgb(100, 110, 120);
        lines.push(Line::from(vec![
            Span::styled(format!("{:>4} ", old_str), Style::new().fg(dim).bg(bg)),
            Span::styled(format!("{:<4} ", new_str), Style::new().fg(dim).bg(bg)),
            Span::styled(body.to_string(), Style::new().fg(fg).bg(bg)),
        ]));
    }
    if lines.is_empty() {
        // Fallback: render raw
        for line in text.lines() {
            lines.push(Line::from(Span::raw(line.to_string())));
        }
    }
    lines
}

/// Parse unified diff into side-by-side rows: (old_ln, new_ln, old_body, new_body, kind).
/// Adjacent del+add lines are paired as a modification row.
pub fn parse_diff_rows(text: &str) -> Vec<(String, String, String, String, String)> {
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    let mut old_ln: u32 = 0;
    let mut new_ln: u32 = 0;
    let mut started = false;
    // Collect raw lines first: (old_ln, new_ln, body, kind)
    let mut raw: Vec<(String, String, String, String)> = Vec::new();

    for line in text.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") { continue; }
        if line.starts_with("@@") {
            if let Some(rest) = line.strip_prefix("@@ -") {
                if let Some((old_part, rest2)) = rest.split_once(' ') {
                    let new_part = rest2.strip_prefix('+').unwrap_or(rest2);
                    old_ln = old_part.split(',').next().and_then(|s| s.parse().ok()).unwrap_or(old_ln);
                    new_ln = new_part.split(',').next().and_then(|s| s.parse().ok()).unwrap_or(new_ln);
                }
            }
            if old_ln > 0 { old_ln -= 1; }
            if new_ln > 0 { new_ln -= 1; }
            started = true;
            continue;
        }
        if !started { continue; }
        let body = &line[if line.starts_with('-') || line.starts_with('+') { 1 } else { 0 }..];
        if line.starts_with('-') {
            old_ln += 1;
            raw.push((format!("{}", old_ln), String::new(), body.to_string(), "del".into()));
        } else if line.starts_with('+') {
            new_ln += 1;
            raw.push((String::new(), format!("{}", new_ln), body.to_string(), "add".into()));
        } else {
            old_ln += 1;
            new_ln += 1;
            raw.push((format!("{}", old_ln), format!("{}", new_ln), body.to_string(), "ctx".into()));
        }
    }

    // Pair adjacent del+add into a single row (modification)
    let mut i = 0;
    while i < raw.len() {
        if raw[i].3 == "del" && i + 1 < raw.len() && raw[i+1].3 == "add" {
            let (old, _, old_body, _) = &raw[i];
            let (_, new, new_body, _) = &raw[i+1];
            rows.push((old.clone(), new.clone(), old_body.clone(), new_body.clone(), "mod".into()));
            i += 2;
        } else {
            let (old, new, body, kind) = &raw[i];
            if kind == "add" {
                rows.push((old.clone(), new.clone(), String::new(), body.clone(), kind.clone()));
            } else {
                rows.push((old.clone(), new.clone(), body.clone(), String::new(), kind.clone()));
            }
            i += 1;
        }
    }
    rows
}

// ── Internal renderer ──

/// Stack-based inline style tracker.
#[derive(Debug, Clone)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    fg: Option<Color>,
    bg: Option<Color>,
    underline: bool,
}

impl InlineStyle {
    fn base() -> Self {
        Self { bold: false, italic: false, strikethrough: false, fg: None, bg: None, underline: false }
    }

    fn to_ratatui(&self) -> Style {
        let mut s = Style::new();
        if self.bold { s = s.bold(); }
        if self.italic { s = s.italic(); }
        if self.strikethrough { s = s.crossed_out(); }
        if self.underline { s = s.underlined(); }
        if let Some(c) = self.fg { s = s.fg(c); }
        if let Some(c) = self.bg { s = s.bg(c); }
        s
    }
}

struct MdRenderer {
    lines: Vec<Line<'static>>,
    /// Current inline spans being accumulated for the current line.
    span_buf: Vec<Span<'static>>,
    /// Stack of inline styles (pushed on Start(Strong/Emphasis/…), popped on End).
    style_stack: Vec<InlineStyle>,
    /// Current inline style.
    style: InlineStyle,
    /// Accumulated code block lines.
    code_lines: Vec<String>,
    code_lang: String,
    /// Table buffering.
    table_rows: Vec<Vec<String>>,
    table_alignments: Vec<pulldown_cmark::Alignment>,
    /// Current block-level nesting.
    in_code_block: bool,
    in_table: bool,
    in_blockquote: bool,
    in_heading: Option<u8>,
    in_list: bool,
    list_depth: u32,
    list_order: Option<u64>,
}

impl MdRenderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            span_buf: Vec::new(),
            style_stack: Vec::new(),
            style: InlineStyle::base(),
            code_lines: Vec::new(),
            code_lang: String::new(),
            table_rows: Vec::new(),
            table_alignments: Vec::new(),
            in_code_block: false,
            in_table: false,
            in_blockquote: false,
            in_heading: None,
            in_list: false,
            list_depth: 0,
            list_order: None,
        }
    }

    fn render(mut self, parser: Parser<'_>) -> Vec<Line<'static>> {
        for event in parser {
            self.handle_event(event);
        }
        // Flush any pending state
        self.flush_code_block();
        self.flush_table();
        std::mem::take(&mut self.lines)
    }

    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.on_start(tag),
            Event::End(tag_end) => self.on_end(tag_end),
            Event::Text(text) => self.on_text(&text),
            Event::Code(text) => {
                self.push_span(Span::styled(
                    text.to_string(),
                    Style::new().fg(Color::Cyan).bg(Color::Rgb(40, 44, 48)),
                ));
            }
            Event::InlineHtml(html) => {
                self.push_span(Span::styled(html.to_string(), Style::new().fg(Color::Gray)));
            }
            Event::Html(html) => {
                self.push_span(Span::styled(html.to_string(), Style::new().fg(Color::Gray)));
            }
            Event::DisplayMath(math) => {
                self.push_span(Span::styled(math.to_string(), Style::new().fg(Color::Cyan).italic()));
            }
            Event::InlineMath(math) => {
                self.push_span(Span::styled(math.to_string(), Style::new().fg(Color::Cyan)));
            }
            Event::FootnoteReference(label) => {
                self.push_span(Span::styled(
                    format!("[^{}]", label),
                    Style::new().fg(Color::Rgb(100, 200, 255)),
                ));
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "☑" } else { "☐" };
                let color = if checked { Color::Rgb(100, 200, 120) } else { Color::Rgb(140, 150, 160) };
                self.push_span(Span::styled(format!(" {marker} "), Style::new().fg(color)));
            }
            Event::SoftBreak => {
                self.push_span(Span::raw(" "));
            }
            Event::HardBreak => {
                self.commit_line();
            }
            Event::Rule => {
                self.flush_code_block();
                self.flush_table();
                self.lines.push(Line::from(Span::styled(
                    "─────────────────────────────".to_string(),
                    Style::new().fg(TABLE_BORDER),
                )));
            }
        }
    }

    fn on_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                // Start a new paragraph — if we have buffered spans, commit them first
                if !self.span_buf.is_empty() {
                    self.commit_line();
                    self.lines.push(Line::from(""));
                }
            }
            Tag::Heading { level, .. } => {
                self.flush_code_block();
                self.flush_table();
                self.in_heading = Some(level as u8);
            }
            Tag::BlockQuote(_) => {
                self.flush_code_block();
                self.in_blockquote = true;
            }
            Tag::CodeBlock(kind) => {
                self.flush_table();
                self.in_code_block = true;
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::Table(alignments) => {
                self.flush_code_block();
                self.in_table = true;
                self.table_alignments = alignments;
            }
            Tag::TableHead => {} // handled via rows
            Tag::TableRow => {
                self.table_rows.push(Vec::new());
            }
            Tag::TableCell => {
                // Text events will fill the last cell in the last row
            }
            Tag::List(order) => {
                self.in_list = true;
                self.list_depth += 1;
                self.list_order = order;
            }
            Tag::Item => {
                // Emit list bullet/number before the item content
                let indent = "  ".repeat(self.list_depth.saturating_sub(1) as usize);
                if let Some(start) = self.list_order {
                    let num = start; // pulldown-cmark handles numbering
                    self.push_span(Span::raw(format!("{indent}{num}. ")));
                } else {
                    self.push_span(Span::raw(format!("{indent}• ")));
                }
            }
            Tag::Emphasis => {
                self.style_stack.push(self.style.clone());
                self.style.italic = true;
            }
            Tag::Strong => {
                self.style_stack.push(self.style.clone());
                self.style.bold = true;
            }
            Tag::Strikethrough => {
                self.style_stack.push(self.style.clone());
                self.style.strikethrough = true;
            }
            Tag::Link { link_type: _, dest_url: _, title: _, id: _ } => {
                self.style_stack.push(self.style.clone());
                self.style.fg = Some(LINK_FG);
                self.style.underline = true;
                // Store URL for later rendering after link text
            }
            Tag::Image { link_type: _, dest_url, title: _, id: _ } => {
                self.push_span(Span::styled(
                    format!("[Image: {}]", dest_url),
                    Style::new().fg(Color::Gray).italic(),
                ));
            }
            Tag::MetadataBlock(_) => {} // ignore frontmatter
            _ => {} // FootnoteDefinition, etc.
        }
    }

    fn on_end(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Paragraph => {
                self.commit_line();
                self.lines.push(Line::from(""));
            }
            TagEnd::Heading(_) => {
                // Render heading with special style
                let level = self.in_heading.take().unwrap_or(1);
                let spans = std::mem::take(&mut self.span_buf);
                let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
                self.lines.push(Line::from(Span::styled(
                    text,
                    Style::new().fg(HEADING_COLOR).bold(),
                )));
                // Extra blank after h1/h2
                if level <= 2 {
                    self.lines.push(Line::from(""));
                }
            }
            TagEnd::BlockQuote(_) => {
                self.in_blockquote = false;
                self.commit_line();
            }
            TagEnd::CodeBlock => {
                self.flush_code_block();
            }
            TagEnd::Table => {
                self.flush_table();
            }
            TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell => {}
            TagEnd::List(_) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                if self.list_depth == 0 {
                    self.in_list = false;
                }
                self.commit_line();
            }
            TagEnd::Item => {
                self.commit_line();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                if let Some(prev) = self.style_stack.pop() {
                    self.style = prev;
                }
            }
            _ => {}
        }
    }

    fn on_text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_lines.push(text.to_string());
            return;
        }
        if self.in_table {
            // Accumulate cell text — the last row gets cells appended
            if let Some(last_row) = self.table_rows.last_mut() {
                if last_row.is_empty() || text.starts_with('\n') {
                    last_row.push(text.trim().to_string());
                } else {
                    // Append to last cell
                    let idx = last_row.len().saturating_sub(1);
                    if idx < last_row.len() {
                        last_row[idx].push_str(text);
                    } else {
                        last_row.push(text.to_string());
                    }
                }
            }
            return;
        }
        // Inline text — apply current style
        let style = self.style.to_ratatui();
        if self.in_blockquote {
            self.span_buf.push(Span::styled("│ ", Style::new().fg(QUOTE_FG)));
            self.push_span(Span::styled(text.to_string(), style.fg(QUOTE_FG).italic()));
        } else {
            self.push_span(Span::styled(text.to_string(), style));
        }
    }

    fn push_span(&mut self, span: Span<'static>) {
        self.span_buf.push(span);
    }

    fn commit_line(&mut self) {
        let spans = std::mem::take(&mut self.span_buf);
        if spans.is_empty() {
            return;
        }
        self.lines.push(Line::from(spans));
    }

    fn flush_code_block(&mut self) {
        if !self.in_code_block {
            return;
        }
        self.in_code_block = false;
        let lines = std::mem::take(&mut self.code_lines);
        let lang = std::mem::take(&mut self.code_lang);
        if lines.is_empty() {
            return;
        }
        self.lines.extend(render_code_block(&lines, &lang));
    }

    fn flush_table(&mut self) {
        if !self.in_table {
            return;
        }
        self.in_table = false;
        let rows = std::mem::take(&mut self.table_rows);
        let _alignments = std::mem::take(&mut self.table_alignments);
        if rows.is_empty() {
            return;
        }
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
        if let Some(header) = rows.first() {
            out.push(render_table_row(header, &widths));
            out.push(render_table_mid_separator(&widths));
        }
        for row in rows.iter().skip(1) {
            out.push(render_table_row(row, &widths));
        }
        out.push(render_table_bottom_separator(&widths));
        self.lines.extend(out);
    }
}

// ── Syntax highlighting (syntect) ──

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
            .unwrap_or_else(|| ts.themes.values().next().cloned().expect("ThemeSet has at least one theme"))
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

// ── Code block rendering ──

fn render_code_block(lines: &[String], lang: &str) -> Vec<Line<'static>> {
    let syntax = resolve_syntax(lang);
    let theme = theme();
    let mut highlighter = HighlightLines::new(syntax, theme);
    let ss = syntax_set();

    let max_w = 80usize;
    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(Line::from(Span::styled(
        format!("┌─ {} ──────────────────────────────", if lang.is_empty() { "code" } else { lang }),
        Style::new().fg(TABLE_BORDER),
    )));

    for line in lines {
        match highlighter.highlight_line(line, &ss) {
            Ok(hl) => {
                let spans: Vec<Span> = hl.into_iter().map(|(style, text)| {
                    Span::styled(
                        text.to_string(),
                        Style::new()
                            .fg(syntect_color(style.foreground))
                            .bg(if syntect_color(style.background) != Color::Reset {
                                syntect_color(style.background)
                            } else {
                                CODE_BG
                            }),
                    )
                }).collect();
                out.push(Line::from(spans));
            }
            Err(_) => {
                out.push(Line::from(Span::styled(
                    line.chars().take(max_w).collect::<String>(),
                    Style::new().fg(Color::Rgb(200, 200, 200)).bg(CODE_BG),
                )));
            }
        }
    }

    out.push(Line::from(Span::styled(
        "└────────────────────────────────────────".to_string(),
        Style::new().fg(TABLE_BORDER),
    )));
    out
}

// ── Table rendering helpers ──

fn render_table_top_separator(widths: &[usize]) -> Line<'static> {
    let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
    Line::from(Span::styled(
        format!("┌{}┐", parts.join("┬")),
        Style::new().fg(TABLE_BORDER),
    ))
}

fn render_table_mid_separator(widths: &[usize]) -> Line<'static> {
    let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
    Line::from(Span::styled(
        format!("├{}┤", parts.join("┼")),
        Style::new().fg(TABLE_BORDER),
    ))
}

fn render_table_bottom_separator(widths: &[usize]) -> Line<'static> {
    let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
    Line::from(Span::styled(
        format!("└{}┘", parts.join("┴")),
        Style::new().fg(TABLE_BORDER),
    ))
}

fn render_table_row(cells: &[String], widths: &[usize]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("│", Style::new().fg(TABLE_BORDER)));
    for (i, cell) in cells.iter().enumerate() {
        let target_w = widths.get(i).copied().unwrap_or(0);
        let cell_w = cell.width();
        let pad = target_w.saturating_sub(cell_w);
        let padded = format!(" {} {:<pad$}", cell, "", pad = pad);
        spans.push(Span::raw(padded));
        spans.push(Span::styled("│", Style::new().fg(TABLE_BORDER)));
    }
    Line::from(spans)
}

// ═══════════════════════════════════════════════════════════════════════════
// ANSI escape sequence → ratatui Line renderer
// ═══════════════════════════════════════════════════════════════════════════

/// Check if text contains ANSI escape sequences (for routing to ANSI renderer).
pub fn has_ansi(text: &str) -> bool {
    text.as_bytes().windows(2).any(|w| w == [0x1b, b'['])
}

/// Render ANSI-escaped terminal output to ratatui Lines.
/// SGR (colors, bold, etc.) is applied; cursor movements and other CSI are stripped.
pub fn render_ansi(text: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut style = Style::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut text_start = 0usize;

    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Flush accumulated text before this escape
            if text_start < i {
                let s = String::from_utf8_lossy(&bytes[text_start..i]);
                current_line.push(Span::styled(s.to_string(), style));
            }
            // Parse CSI: ESC [ ... final-byte
            i += 2;
            let param_start = i;
            while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                i += 1;
            }
            if i >= bytes.len() { break; }
            let params = std::str::from_utf8(&bytes[param_start..i]).unwrap_or("");
            let final_byte = bytes[i];
            i += 1;
            text_start = i;

            match final_byte {
                b'm' => {
                    // SGR — update style
                    style = apply_sgr(style, params);
                }
                b'J' | b'K' => {
                    // Erase display/line — ignore in streaming context
                }
                b'H' | b'f' | b'A' | b'B' | b'C' | b'D' | b'E' | b'F' | b'G'
                | b'd' | b'n' | b's' | b'u' | b'r' => {
                    // Cursor movement — ignore
                }
                b'h' | b'l' => {
                    // DEC private mode set/reset — ignore
                }
                _ => {} // Unknown CSI — strip
            }
        } else if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b']' {
            // OSC: skip until ST (ESC \) or BEL
            if text_start < i {
                let s = String::from_utf8_lossy(&bytes[text_start..i]);
                current_line.push(Span::styled(s.to_string(), style));
            }
            i += 2;
            while i < bytes.len() {
                if bytes[i] == 0x07 { i += 1; break; }
                if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' { i += 2; break; }
                i += 1;
            }
            text_start = i;
        } else if bytes[i] == b'\n' {
            if text_start < i {
                let s = String::from_utf8_lossy(&bytes[text_start..i]);
                current_line.push(Span::styled(s.to_string(), style));
            }
            lines.push(Line::from(std::mem::take(&mut current_line)));
            i += 1;
            text_start = i;
        } else if bytes[i] == b'\r' {
            // Carriage return: flush text, then if followed by \n, skip it (CRLF → single newline)
            if text_start < i {
                let s = String::from_utf8_lossy(&bytes[text_start..i]);
                current_line.push(Span::styled(s.to_string(), style));
            }
            i += 1;
            if i < bytes.len() && bytes[i] == b'\n' {
                // CRLF — treat as single line ending
                lines.push(Line::from(std::mem::take(&mut current_line)));
                i += 1;
            }
            text_start = i;
        } else {
            i += 1;
        }
    }
    // Flush remaining
    if text_start < bytes.len() {
        let s = String::from_utf8_lossy(&bytes[text_start..]);
        current_line.push(Span::styled(s.to_string(), style));
    }
    if !current_line.is_empty() {
        lines.push(Line::from(current_line));
    }
    if lines.is_empty() {
        // Fallback: render as plain text
        for line in text.lines() {
            lines.push(Line::from(Span::raw(line.to_string())));
        }
    }
    lines
}

/// Parse SGR parameters and return updated Style.
fn apply_sgr(mut style: Style, params: &str) -> Style {
    if params.is_empty() {
        // ESC[m = reset
        return Style::new();
    }
    let mut iter = params.split(';').filter_map(|s| s.parse::<u8>().ok());
    loop {
        let Some(n) = iter.next() else { break };
        match n {
            0 => style = Style::new(),
            1 => style = style.bold(),
            2 => style = style.dim(),
            3 => style = style.italic(),
            4 => style = style.underlined(),
            7 => style = style.reversed(),
            9 => style = style.crossed_out(),
            22 => style = style.not_bold().not_dim(),
            23 => style = style.not_italic(),
            24 => style = style.not_underlined(),
            27 => style = style.not_reversed(),
            29 => style = style.not_crossed_out(),
            30..=37 => style = style.fg(ansi_4bit(n - 30)),
            38 => {
                // Extended foreground: 38;5;N or 38;2;R;G;B
                match iter.next() {
                    Some(5) => { if let Some(c) = iter.next() { style = style.fg(ansi_256(c)); } }
                    Some(2) => {
                        let r = iter.next().unwrap_or(0);
                        let g = iter.next().unwrap_or(0);
                        let b = iter.next().unwrap_or(0);
                        style = style.fg(Color::Rgb(r, g, b));
                    }
                    _ => {}
                }
            }
            39 => style = style.fg(Color::Reset),
            40..=47 => style = style.bg(ansi_4bit(n - 40)),
            48 => {
                // Extended background: 48;5;N or 48;2;R;G;B
                match iter.next() {
                    Some(5) => { if let Some(c) = iter.next() { style = style.bg(ansi_256(c)); } }
                    Some(2) => {
                        let r = iter.next().unwrap_or(0);
                        let g = iter.next().unwrap_or(0);
                        let b = iter.next().unwrap_or(0);
                        style = style.bg(Color::Rgb(r, g, b));
                    }
                    _ => {}
                }
            }
            49 => style = style.bg(Color::Reset),
            90..=97 => style = style.fg(ansi_4bit(n - 90 + 8)), // bright foreground
            100..=107 => style = style.bg(ansi_4bit(n - 100 + 8)), // bright background
            _ => {} // Unknown SGR — ignore
        }
    }
    style
}

/// Map ANSI 4-bit color index (0-15) to ratatui Color.
fn ansi_4bit(n: u8) -> Color {
    match n {
        0 => Color::Rgb(0, 0, 0),
        1 => Color::Rgb(170, 0, 0),
        2 => Color::Rgb(0, 170, 0),
        3 => Color::Rgb(170, 85, 0),
        4 => Color::Rgb(0, 0, 170),
        5 => Color::Rgb(170, 0, 170),
        6 => Color::Rgb(0, 170, 170),
        7 => Color::Rgb(170, 170, 170),
        8 => Color::Rgb(85, 85, 85),
        9 => Color::Rgb(255, 85, 85),
        10 => Color::Rgb(85, 255, 85),
        11 => Color::Rgb(255, 255, 85),
        12 => Color::Rgb(85, 85, 255),
        13 => Color::Rgb(255, 85, 255),
        14 => Color::Rgb(85, 255, 255),
        15 => Color::Rgb(255, 255, 255),
        _ => Color::Rgb(170, 170, 170),
    }
}

/// Map ANSI 256-color palette index to ratatui Color.
fn ansi_256(n: u8) -> Color {
    match n {
        0..=15 => ansi_4bit(n),
        16..=231 => {
            let n = n - 16;
            let r = (n / 36) * 51;
            let g = ((n % 36) / 6) * 51;
            let b = (n % 6) * 51;
            Color::Rgb(r, g, b)
        }
        232..=255 => {
            let v = (n - 232) * 10 + 8;
            Color::Rgb(v, v, v)
        }
    }
}
