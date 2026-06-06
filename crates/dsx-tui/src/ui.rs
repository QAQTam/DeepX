use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::app::{App, ChatRole, ToolStatus};
use crate::i18n::Lang;
use unicode_width::UnicodeWidthStr;

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + (r.width.saturating_sub(width) / 2);
    let y = r.y + (r.height.saturating_sub(height) / 2);
    Rect::new(x, y, width.min(r.width), height.min(r.height))
}

const ACCENT: Color = Color::Rgb(100, 200, 255);
const DIM: Color = Color::Rgb(60, 60, 60);
const BG: Color = Color::Rgb(24, 28, 32);
const GREEN: Color = Color::Rgb(100, 180, 120);

fn cjk_width(s: &str) -> u16 {
    s.width() as u16
}

fn format_ts(seconds: u64) -> String {
    use chrono::{TimeZone, Local};
    if let Some(dt) = Local.timestamp_opt(seconds as i64, 0).single() {
        dt.format("%Y-%m-%d %H:%M").to_string()
    } else {
        String::new()
    }
}

// ── Setup wizard ──

pub fn render_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let l = app.setup.lang;

    frame.render_widget(Paragraph::new("").style(Style::new().bg(BG)), area);

    let popup_h = if app.setup.step == 2 && app.setup.models_loaded { 22u16 } else { 20u16 };
    let popup = centered_rect(66, popup_h, area);
    frame.render_widget(Clear, popup);

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Title
    let title = Line::from(vec![
        Span::raw("  "),
        Span::styled("⚡", Style::new().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(l.t_setup_welcome(), Style::new().fg(ACCENT).bold()),
        Span::raw("  "),
    ]);
    frame.render_widget(title, Rect::new(popup.x + 2, popup.y, popup.width, 1));

    let [steps_bar, content_area, help_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ]).areas(inner);

    // Step progress
    let total = app.setup.total_steps();
    let step_names = [
        l.t_select_lang(),
        l.t_api_key(),
        l.t_model(),
        l.t_context_limit(),
    ];
    let step_width = (steps_bar.width - 4) / total as u16;
    let bar_spans: Vec<Span> = (0..total).map(|i| {
        let fill = if i < app.setup.step {
            "█".repeat(step_width as usize)
        } else if i == app.setup.step {
            let n = step_width as usize / 3;
            format!("{}{}", "█".repeat(n), "░".repeat((step_width as usize).saturating_sub(n)))
        } else {
            "░".repeat(step_width as usize)
        };
        let color = if i <= app.setup.step { ACCENT } else { DIM };
        Span::styled(fill, Style::new().fg(color))
    }).collect();
    frame.render_widget(Line::from(bar_spans), Rect {
        x: steps_bar.x + 2, y: steps_bar.y,
        width: steps_bar.width - 4, height: 1,
    });

    let lbl_spans: Vec<Span> = (0..total).map(|i| {
        let style = if i == app.setup.step {
            Style::new().fg(Color::White).bold()
        } else if i < app.setup.step {
            Style::new().fg(GREEN)
        } else {
            Style::new().fg(DIM)
        };
        let txt = step_names[i];
        let w = step_width as usize;
        let padding = w.saturating_sub(txt.width().min(w));
        Span::styled(format!("{}{}", txt, " ".repeat(padding)), style)
    }).collect();
    frame.render_widget(Line::from(lbl_spans), Rect {
        x: steps_bar.x + 2, y: steps_bar.y + 1,
        width: steps_bar.width - 4, height: 1,
    });

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    match app.setup.step {
        0 => {
            let tag = l.t_select_lang();
            lines.push(Line::from(vec![
                Span::styled(format!("  {tag}  "), Style::new().fg(Color::Black).bg(ACCENT).bold()),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(""));
            let langs = [(Lang::En, l.t_setup_lang_en_name(), l.t_setup_lang_en_desc()),
                         (Lang::Zh, l.t_setup_lang_zh_name(), l.t_setup_lang_zh_desc())];
            for &(lang, name, desc) in &langs {
                let selected = app.setup.lang == lang;
                let mark = if selected { "●" } else { "○" };
                let style = if selected { Style::new().fg(ACCENT).bold() }
                            else { Style::new().fg(DIM) };
                lines.push(Line::from(vec![
                    Span::raw(format!("     {mark}  ")),
                    Span::styled(name, style),
                    Span::raw("  —  "),
                    Span::styled(desc, Style::new().fg(Color::Gray)),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(l.t_setup_nav_hint(), Style::new().fg(DIM))));
        }
        1 => {
            let tag = l.t_api_key();
            lines.push(Line::from(vec![
                Span::styled(format!("  {tag}  "), Style::new().fg(Color::Black).bg(ACCENT).bold()),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(format!("  {}", l.t_enter_key()), Style::new().fg(Color::Gray))));
            lines.push(Line::from(""));
            lines.push(Line::from(""));
            let display = if app.setup.api_key.is_empty() {
                "sk-".to_string()
            } else {
                format!("sk-{}", "●".repeat(app.setup.api_key.len().saturating_sub(3).min(40)))
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(display, Style::new().fg(Color::Yellow)),
            ]));
        }
        2 => {
            let tag = l.t_model();
            lines.push(Line::from(vec![
                Span::styled(format!("  {tag}  "), Style::new().fg(Color::Black).bg(ACCENT).bold()),
            ]));
            lines.push(Line::from(""));

            if app.setup.models_loaded {
                // Dynamic list from API
                lines.push(Line::from(Span::styled(format!("  {}", l.t_select_model()), Style::new().fg(Color::Gray))));
                lines.push(Line::from(""));
                let show = &app.setup.model_list;
                for name in show.iter().take(6) {
                    let selected = app.setup.model == *name;
                    let mark = if selected { "●" } else { "○" };
                    let style = if selected { Style::new().fg(ACCENT).bold() }
                                else { Style::new().fg(DIM) };
                    lines.push(Line::from(vec![
                        Span::raw(format!("  {mark} ")),
                        Span::styled(name.clone(), style),
                    ]));
                }
                if show.len() > 6 {
                    lines.push(Line::from(Span::styled(
                        format!("  ... and {} more", show.len() - 6),
                        Style::new().fg(DIM),
                    )));
                }
                lines.push(Line::from(""));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  > "),
                Span::styled(app.setup.model.clone(), Style::new().fg(Color::Yellow)),
            ]));
        }
        3 => {
            let tag = l.t_context_limit();
            lines.push(Line::from(vec![
                Span::styled(format!("  {tag}  "), Style::new().fg(Color::Black).bg(ACCENT).bold()),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(format!("  {}", l.t_max_tokens_desc()), Style::new().fg(Color::Gray))));
            lines.push(Line::from(""));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{:>10}", app.setup.context_limit), Style::new().fg(Color::Yellow).bold()),
                Span::styled(l.t_setup_tokens_unit(), Style::new().fg(Color::Gray)),
            ]));
        }
        _ => {}
    }

    frame.render_widget(Paragraph::new(lines), content_area);

    // Status / error
    if !app.setup.status.is_empty() {
        let color = if app.setup.status.contains("Valid") || app.setup.status.contains("有效") {
            GREEN
        } else {
            Color::Red
        };
        frame.render_widget(
            Span::styled(format!("  {}", app.setup.status), Style::new().fg(color)),
            Rect { x: content_area.x, y: content_area.y + content_area.height.saturating_sub(2),
                  width: content_area.width, height: 1 },
        );
    } else if !app.setup.error.is_empty() {
        frame.render_widget(
            Span::styled(format!("  ✗ {}", app.setup.error), Style::new().fg(Color::Red)),
            Rect { x: content_area.x, y: content_area.y + content_area.height.saturating_sub(2),
                  width: content_area.width, height: 1 },
        );
    }

    // Bottom help
    let s_next = l.t_enter_next();
    let s_clear = l.t_esc_clear();
    let s_quit = l.t_ctrl_c_quit();
    let s_retry = l.t_retry();

    let help = if !app.setup.error.is_empty() || app.validating {
        let lbl = if app.validating { l.t_validating() } else { s_retry };
        Line::from(vec![
            Span::styled(format!(" Enter "), Style::new().fg(Color::Black).bg(Color::Yellow)),
            Span::raw(format!(" {lbl}  ")),
            Span::styled(" Esc ", Style::new().fg(Color::Black).bg(Color::Gray)),
            Span::raw(format!(" {s_clear}  ")),
            Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)),
            Span::raw(format!(" {s_quit}")),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Enter ", Style::new().fg(Color::Black).bg(ACCENT)),
            Span::raw(format!(" {s_next}  ")),
            Span::styled(" Esc ", Style::new().fg(Color::Black).bg(Color::Gray)),
            Span::raw(format!(" {s_clear}  ")),
            Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)),
            Span::raw(format!(" {s_quit}")),
        ])
    };
    frame.render_widget(help, help_area);

    // Cursor
    let val = app.setup.current_value();
    let input_line = content_area.y + app.setup.cursor_row_offset();
    let cursor_x = if app.setup.step == 0 {
        (content_area.x + 16).min(popup.x + popup.width.saturating_sub(2))
    } else {
        (content_area.x + 2 + cjk_width(val).min(40)).min(popup.x + popup.width.saturating_sub(2))
    };
    frame.set_cursor_position((cursor_x, input_line));
}

// ── Session selection screen ──

pub fn render_sessions(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let l = app.setup.lang;

    let popup = centered_rect(70, (app.sessions.len() + 8).min(24).max(12) as u16, area);
    frame.render_widget(Clear, popup);

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(l.t_session_title())
        .style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [list_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(2),
    ]).areas(inner);

    let mut lines: Vec<Line> = Vec::new();

    let max_fit = ((list_area.height as usize).saturating_sub(4)).max(1) / 2;
    let total = app.sessions.len();

    let scroll = if app.session_index < max_fit {
        0
    } else if app.session_index >= total {
        total.saturating_sub(max_fit)
    } else {
        (app.session_index + 1).saturating_sub(max_fit)
    };

    let end = (scroll + max_fit).min(total);
    for idx in scroll..end {
        let s = &app.sessions[idx];
        let selected = idx == app.session_index;
        let mark = if selected { "●" } else { "○" };
        let style = if selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(DIM) };

        let ts = format_ts(s.updated_at);
        let summary: String = {
            let mut width = 0u16;
            s.last_summary.chars().take_while(|c| {
                width += cjk_width(&c.to_string());
                width <= 55
            }).collect()
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  {mark} ")),
            Span::styled(&s.seed, Style::new().fg(Color::Yellow).bold()),
            Span::raw("  "),
            Span::styled(ts, Style::new().fg(Color::Gray)),
            Span::raw("  "),
            Span::styled(format!("{}:{:<5}", l.t_session_msgs(), s.message_count), Style::new().fg(DIM)),
        ]));
        lines.push(Line::from(vec![
            Span::raw("     "),
            Span::styled(summary, style),
        ]));
    }

    // "New Session" row
    let new_selected = app.session_index == app.sessions.len();
    let new_mark = if new_selected { "●" } else { "○" };
    let new_style = if new_selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(Color::Gray) };
    if !app.sessions.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  ──────────────────────────────────────────", Style::new().fg(DIM))));
    }
    lines.push(Line::from(vec![
        Span::raw(format!("  {new_mark} ")),
        Span::styled(l.t_session_new(), new_style),
    ]));

    frame.render_widget(Paragraph::new(lines), list_area);

    let help = Line::from(vec![
        Span::styled(" ↑↓ ", Style::new().fg(Color::Black).bg(ACCENT)),
        Span::raw(l.t_session_select_hint()),
        Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)),
        Span::raw(l.t_session_resume_hint()),
        Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)),
        Span::raw(l.t_session_quit_hint()),
    ]);
    frame.render_widget(help, help_area);
}

// ── Chat interface ──

pub fn render_chat(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let l = app.setup.lang;
    let input_lines = app.input.chars().filter(|&c| c == '\n').count() + 1;
    let input_height = (input_lines as u16 + 2).min(12).max(3);

    let [header_area, body, input_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(input_height),
    ]).spacing(1).areas(area);

    let [header_line1, header_line2] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
    ]).areas(header_area);

    let status_text = if app.streaming {
        format!("{} {}", app.spinner(), &app.status)
    } else {
        app.status.clone()
    };
    let cache_total = app.cache_hit + app.cache_miss;
    let cache_rate = if cache_total > 0 { app.cache_hit as f64 / cache_total as f64 * 100.0 } else { 0.0 };
    let cache_color = if cache_rate > 0.5 { Color::Rgb(100, 200, 120) } else { Color::Rgb(200, 150, 100) };
    let ctx_pct = if app.context_limit > 0 {
        app.context_tokens as f64 / app.context_limit as f64 * 100.0
    } else { 0.0 };

    // Line 1: version | context bar | status | think toggle
    let h1 = Line::from(vec![
        Span::raw(format!("DeepX v{}", env!("CARGO_PKG_VERSION"))),
        Span::raw(" | "),
        Span::styled(format!("Context: {} / {} ({:.0}%)", app.context_tokens, app.context_limit, ctx_pct),
            Style::new().fg(Color::Yellow)),
        Span::raw(" | "),
        Span::styled(&status_text, Style::new().fg(if app.streaming { Color::Yellow } else { Color::Green })),
        Span::raw(""), Span::raw(""),
    ]);
    frame.render_widget(h1, header_line1);

    // Line 2: session | cache | balance | dsml | warning
    let mut h2_spans = vec![
        Span::styled(format!("Session: {}", app.session_tokens), Style::new().fg(Color::Rgb(180, 180, 200))),
        Span::raw("  "),
    ];
    if cache_total > 0 {
        h2_spans.push(Span::styled(format!("Hit:{}", app.cache_hit), Style::new().fg(Color::Rgb(100, 200, 120))));
        h2_spans.push(Span::styled(format!("/Miss:{}", app.cache_miss), Style::new().fg(Color::Rgb(200, 150, 100))));
        h2_spans.push(Span::styled(format!(" ({:.0}%)", cache_rate), Style::new().fg(cache_color)));
        h2_spans.push(Span::raw("  "));
    }
    if !app.balance.is_empty() {
        h2_spans.push(Span::styled(&app.balance, Style::new().fg(Color::Rgb(100, 200, 255))));
        h2_spans.push(Span::raw("  "));
    }
    h2_spans.push(Span::styled(format!("DSML: {}", app.debug.dsml_compat_count), Style::new().fg(Color::Rgb(100, 220, 140))));
    if !app.cache_warning.is_empty() {
        h2_spans.push(Span::raw("  "));
        h2_spans.push(Span::styled(&app.cache_warning, Style::new().fg(Color::Red).bold()));
    }
    frame.render_widget(Line::from(h2_spans), header_line2);

    let mut text_lines: Vec<Line> = Vec::with_capacity(app.messages.len() * 4);
    let mut prev_role: Option<ChatRole> = None;
    for msg in &app.messages {
        // Thinking: rendered inline (v5 — per-round)

        // Role separator: dim line between different non-divider roles
        if let Some(pr) = prev_role {
            if pr != msg.role && pr != ChatRole::Divider && msg.role != ChatRole::Divider
                && pr != ChatRole::Status && msg.role != ChatRole::Status
            {
                let div_len = body.width.saturating_sub(2).min(60) as usize;
                text_lines.push(Line::from(Span::styled(
                    format!(" {}", "─".repeat(div_len)),
                    Style::new().fg(DIM),
                )));
            }
        }
        if msg.role != ChatRole::Divider && msg.role != ChatRole::Status {
            prev_role = Some(msg.role);
        }

        match msg.role {
            ChatRole::Divider => {
                let div_len = body.width.saturating_sub(2).min(60) as usize;
                text_lines.push(Line::from(Span::styled(
                    format!(" {}", "─".repeat(div_len)),
                    Style::new().fg(DIM),
                )));
            }
            ChatRole::Status => {
                text_lines.push(Line::from(Span::styled(&msg.content, Style::new().fg(Color::Red))));
            }
            ChatRole::User => {
                let bg = Color::Rgb(55, 55, 65);
                text_lines.push(Line::from(vec![
                    Span::styled(format!("  {}", &msg.content), Style::new().fg(Color::White).bg(bg)),
                ]).alignment(Alignment::Left));
            }
            ChatRole::Thinking => {
                let dim = Color::Rgb(140, 140, 150);
                if msg.lines.is_empty() {
                    text_lines.push(Line::from(vec![
                        Span::styled(format!("  {}", &msg.content), Style::new().fg(dim).italic()),
                    ]));
                } else {
                    for line in msg.lines.iter() {
                        if line.spans.is_empty() || line.spans.iter().all(|s| s.content.trim().is_empty()) {
                            continue;
                        }
                        let mut spans: Vec<Span> = line.spans.iter().map(|s| {
                            Span::styled(s.content.clone(), s.style.italic())
                        }).collect();
                        if spans.first().map_or(true, |s| !s.content.starts_with("  ")) {
                            spans.insert(0, Span::raw("  "));
                        }
                        text_lines.push(Line::from(spans));
                    }
                }
            }
            ChatRole::Assistant => {
                if msg.lines.is_empty() {
                    text_lines.push(Line::from(vec![
                        Span::styled(format!("  {}", &msg.content), Style::new().fg(Color::White)),
                    ]));
                } else {
                    let first_char = msg.lines[0].spans.first()
                        .and_then(|s| s.content.chars().next());
                    let is_table = first_char.map_or(false, |c| {
                        c == '│' || c == '├' || c == '└' || c == '┌' || c == '┐' || c == '┘'
                    });
                    if is_table {
                        for line in &msg.lines {
                            text_lines.push(line.clone());
                        }
                    } else {
                        for line in msg.lines.iter() {
                            let mut spans: Vec<Span> = line.spans.iter().map(|s| s.clone()).collect();
                            if spans.first().map_or(true, |s| !s.content.starts_with("  ")) {
                                spans.insert(0, Span::raw("  "));
                            }
                            text_lines.push(Line::from(spans));
                        }
                    }
                }
            }
            ChatRole::Tool => {
                // Prefix: spinner for pending, ✓ for success, ✗ for failed
                let prefix = match msg.tool_status {
                    ToolStatus::Pending if app.busy => Span::styled(
                        format!("{} ", app.spinner()),
                        Style::new().fg(Color::Yellow),
                    ),
                    ToolStatus::Pending => Span::styled("○ ", Style::new().fg(Color::Gray)),
                    ToolStatus::Success => Span::styled("✓ ", Style::new().fg(Color::Green)),
                    ToolStatus::Failed => Span::styled("✗ ", Style::new().fg(Color::Red)),
                    ToolStatus::None => Span::raw(""),
                };
                if msg.lines.is_empty() {
                    text_lines.push(Line::from(vec![prefix, Span::raw(&msg.content)]));
                } else {
                    for (i, line) in msg.lines.iter().enumerate() {
                        let mut spans: Vec<Span> = line.spans.iter().map(|s| s.clone()).collect();
                        if i == 0 {
                            spans.insert(0, prefix.clone());
                        }
                        text_lines.push(Line::from(spans));
                    }
                }
            }
        }
    }

    let [body_content, scrollbar_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(1),
    ]).areas(body);

    let content_height = body_content.height as usize;

    // Build paragraph first so we can query ratatui's exact line count.
    // Cache result — line_count() is expensive, only recompute when messages change.
    let paragraph = Paragraph::new(text_lines)
        .wrap(ratatui::widgets::Wrap { trim: false });
    let total_wrapped = if app.line_count_msg_len != app.messages.len()
        || app.line_count_width != body_content.width
    {
        let count = paragraph.line_count(body_content.width) as usize;
        app.cached_line_count = count;
        app.line_count_msg_len = app.messages.len();
        app.line_count_width = body_content.width;
        count
    } else {
        app.cached_line_count
    };
    let max_scroll = total_wrapped.saturating_sub(content_height);
    let at_bottom = app.streaming || app.scroll_offset == 0;
    let scroll = if at_bottom {
        max_scroll.min(u16::MAX as usize) as u16
    } else {
        let offset = app.scroll_offset.min(max_scroll);
        (max_scroll - offset).min(u16::MAX as usize) as u16
    };

    frame.render_widget(paragraph.scroll((scroll, 0)), body_content);

    let mut scrollbar_state = ScrollbarState::new(total_wrapped.max(1))
        .position(scroll as usize)
        .viewport_content_length(content_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_symbol("█")
        .track_symbol(Some("│"));
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);

    let input_block = Block::new()
        .borders(Borders::ALL)
        .title(l.t_chat_input_title());
    let input_text: Vec<Line> = if app.input.is_empty() {
        vec![Line::from(Span::styled(l.t_chat_input_placeholder(), Style::new().fg(Color::DarkGray)))]
    } else if app.cached_input_len != app.input.len() {
        let lines: Vec<Line> = app.input.lines().map(|l| Line::from(Span::raw(l.to_string()))).collect();
        app.cached_input_lines = lines.clone();
        app.cached_input_len = app.input.len();
        lines
    } else {
        app.cached_input_lines.clone()
    };
    frame.render_widget(Paragraph::new(input_text).block(input_block), input_area);

    // Cursor: position based on actual cursor byte offset
    let cursor_byte = app.cursor.min(app.input.len());
    let pre_cursor = &app.input[..cursor_byte];
    let cursor_line = pre_cursor.chars().filter(|&c| c == '\n').count();
    let last_line_start = pre_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let cursor_col = cjk_width(&app.input[last_line_start..cursor_byte]).min(input_area.width.saturating_sub(3)) as u16;
    let input_top = input_area.y + 1;
    // Ensure cursor line doesn't exceed input area height
    let cursor_row = input_top + cursor_line.min(input_area.height.saturating_sub(3) as usize) as u16;
    frame.set_cursor_position((
        (input_area.x + 1 + cursor_col).min(area.width.saturating_sub(1)),
        cursor_row,
    ));

    // Debug overlay
    if app.show_debug {
        let d = &app.debug;
        let dbg_w = 40u16;
        let dbg_h = 10u16;
        let dbg_rect = Rect::new(
            area.width.saturating_sub(dbg_w + 2),
            area.y + 1,
            dbg_w,
            dbg_h,
        );
        frame.render_widget(Clear, dbg_rect);
        let dbg_block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Rgb(180, 150, 255)))
            .title(l.t_debug_title())
            .style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&dbg_block, dbg_rect);

        let inner = dbg_block.inner(dbg_rect);
        let hp_dot = if d.hp_connected { ("●", Color::Green) } else { ("○", Color::Red) };
        let stream_dot = if d.streaming { ("●", Color::Yellow) } else { ("○", Color::Gray) };
        let lines = vec![
            Line::from(vec![
                Span::styled(format!(" {}: {} ", l.t_debug_hp(), hp_dot.0), Style::new().fg(hp_dot.1)),
                Span::styled(format!("{}: {} ", l.t_debug_stream(), stream_dot.0), Style::new().fg(stream_dot.1)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}: ", l.t_debug_session()), Style::new().fg(Color::Gray)),
                Span::styled(&d.session_seed, Style::new().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}:", l.t_debug_context()), Style::new().fg(Color::Gray)),
                Span::styled(format!(" {} / 1M", d.context_tokens), Style::new().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}:  ", l.t_debug_tools()), Style::new().fg(Color::Gray)),
                Span::styled(format!("{} {}", d.tool_calls_total, l.t_debug_calls()), Style::new().fg(Color::Cyan)),
                if d.tool_failures > 0 {
                    Span::styled(format!(" / {} {}", d.tool_failures, l.t_debug_fail()), Style::new().fg(Color::Red))
                } else {
                    Span::raw("")
                },
                Span::raw(" "),
                Span::styled(format!("(DSML compat: {})", d.dsml_compat_count), Style::new().fg(Color::Rgb(100, 220, 140))),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);

        // Document tracking overlay
        if !d.documents.is_empty() {
            let n = d.documents.len().min(10);
            let doc_w = 55u16;
            let doc_h = (n as u16 + 3).min(15);
            let doc_rect = Rect::new(
                area.width.saturating_sub(doc_w + 2),
                area.y + 10,
                doc_w,
                doc_h,
            );
            frame.render_widget(Clear, doc_rect);
            let doc_block = Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::Rgb(180, 140, 100)))
                .title(format!(" Docs ({}) ", n))
                .style(Style::new().bg(Color::Rgb(18, 22, 30)));
            frame.render_widget(&doc_block, doc_rect);

            let inner = doc_block.inner(doc_rect);
            let doc_lines: Vec<Line> = d.documents.iter().take(10).map(|doc| {
                let tag_style = if doc.is_stale {
                    Style::new().fg(Color::Rgb(220, 100, 100))
                } else {
                    Style::new().fg(Color::Rgb(100, 200, 120))
                };
                let turns_text = format!("{}t", doc.turns_since_read);
                Line::from(vec![
                    Span::styled(format!(" {} ", doc.tag), tag_style.bold()),
                    Span::styled(format!("{} ", doc.path), Style::new().fg(Color::Rgb(180, 180, 200))),
                    Span::styled(turns_text, Style::new().fg(Color::Rgb(100, 120, 140))),
                ])
            }).collect();
            frame.render_widget(Paragraph::new(doc_lines), inner);
        }
    }

    // Task overlay
    if app.show_tasks {
        let tasks = app.tasks();
        let task_w = 50u16;
        let task_h = (tasks.len() as u16 + 3).min(20).max(5);
        let task_rect = Rect::new(
            area.width.saturating_sub(task_w + 2),
            area.y + 14,
            task_w,
            task_h,
        );
        frame.render_widget(Clear, task_rect);
        let task_block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Rgb(120, 200, 200)))
            .title(format!(" Tasks ({}) ", tasks.len()))
            .style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&task_block, task_rect);

        let inner = task_block.inner(task_rect);
        let lines: Vec<Line> = if tasks.is_empty() {
            vec![Line::from(Span::styled("  (no tasks)", Style::new().fg(Color::Gray)))]
        } else {
            tasks.iter().map(|(icon, text)| {
                let icon_color = match icon.as_str() {
                    "✓" => Color::Rgb(100, 220, 100),
                    "●" => Color::Rgb(220, 200, 100),
                    _ => Color::Rgb(140, 150, 160),
                };
                Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::new().fg(icon_color).bold()),
                    Span::styled(text.to_string(), Style::new().fg(Color::Rgb(200, 210, 220))),
                ])
            }).collect()
        };
        frame.render_widget(Paragraph::new(lines), inner);
    }

    // Context window
    if app.show_context {
        let d = &app.debug;
        let ctx_w = 50u16;
        let ctx_h = 4u16;
        let ctx_rect = Rect::new(
            area.width.saturating_sub(ctx_w + 2),
            area.y + 1,
            ctx_w,
            ctx_h,
        );
        frame.render_widget(Clear, ctx_rect);
        let ctx_block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Rgb(120, 180, 255)))
            .title(" Context ")
            .style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&ctx_block, ctx_rect);
        let inner = ctx_block.inner(ctx_rect);
        let lines: Vec<Line> = vec![
            Line::from(Span::styled(
                format!(" tokens: {} / {}", d.context_tokens, 0),
                Style::new().fg(Color::Rgb(120, 200, 200)),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

pub fn render_ask(frame: &mut Frame, app: &App) {
    let ask = match &app.ask {
        Some(a) => a,
        None => return,
    };
    let l = app.setup.lang;
    let area = frame.area();
    let h = (ask.options.len() + 5).min(20) as u16;
    let popup = centered_rect(60, h, area);
    frame.render_widget(Clear, popup);

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(255, 180, 100)))
        .title(l.t_ask_title())
        .style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(&block, popup);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(&ask.question, Style::new().fg(Color::White).bold())));
    lines.push(Line::from(""));

    for (i, opt) in ask.options.iter().enumerate() {
        let selected = i == ask.selected;
        let mark = if selected { "●" } else { " " };
        let style = if selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(Color::Gray) };

        if opt.is_empty() && selected {
            let display = if ask.custom_input.is_empty() {
                "  ______".to_string()
            } else {
                ask.custom_input.clone()
            };
            lines.push(Line::from(vec![
                Span::raw(format!("  {mark} ")),
                Span::styled(format!("{}: ", l.t_ask_other()), Style::new().fg(Color::Gray)),
                Span::styled(display, Style::new().fg(Color::Yellow).bold()),
            ]));
        } else if opt.is_empty() {
            lines.push(Line::from(vec![
                Span::raw(format!("  {mark} ")),
                Span::styled(l.t_ask_other_placeholder(), Style::new().fg(Color::Gray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw(format!("  {mark} ")),
                Span::styled(opt.clone(), style),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        l.t_ask_help(),
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── Menu screen ──

pub fn render_menu(frame: &mut Frame, menu: &crate::app::MenuState) {
    use crate::app::MenuItemKind;

    let area = frame.area();
    frame.render_widget(Paragraph::new("").style(Style::new().bg(BG)), area);

    let [_, list_area, status_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]).areas(area);

    let title_block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(menu.lang.t_menu_title())
        .style(Style::new().bg(Color::Rgb(18, 22, 26)));
    frame.render_widget(title_block, Rect::new(area.x, area.y, area.width, 3));

    let nav = menu.lang.t_menu_nav();
    let toggle_hint = menu.lang.t_menu_toggle_edit();
    let back_hint = menu.lang.t_menu_back();
    let title_lines = vec![
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Menu", Style::new().fg(ACCENT).bold()),
            Span::raw("  |  "),
            Span::styled(nav, Style::new().fg(DIM)),
            Span::raw("  "),
            Span::styled(toggle_hint, Style::new().fg(DIM)),
            Span::raw("  "),
            Span::styled(back_hint, Style::new().fg(DIM)),
        ]),
    ];
    frame.render_widget(Paragraph::new(title_lines), Rect::new(area.x + 2, area.y + 1, area.width - 4, 1));

    let visible = list_area.height.saturating_sub(2) as usize;
    let max_scroll = menu.items.len().saturating_sub(visible);
    let scroll = if menu.selected < max_scroll {
        menu.selected
    } else {
        max_scroll
    };

    let mut lines: Vec<Line> = Vec::new();
    let show_from = scroll;
    let show_to = (scroll + visible).min(menu.items.len());

    for idx in show_from..show_to {
        let item = &menu.items[idx];
        let selected = idx == menu.selected;

        let line = match item.kind {
            MenuItemKind::Section => {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(&item.label, Style::new().fg(Color::Rgb(100, 200, 255)).bold()),
                ])
            }
            MenuItemKind::Toggle => {
                let on = item.value == "ON" || item.value == "en";
                let val_style = if on {
                    Style::new().fg(Color::Rgb(100, 220, 100)).bold()
                } else {
                    Style::new().fg(Color::Rgb(220, 100, 100)).bold()
                };
                let sel_mark = if selected && !menu.editing { "● " } else { "  " };
                let label_style = if selected && !menu.editing {
                    Style::new().fg(Color::Yellow).bold()
                } else {
                    Style::new().fg(Color::White)
                };
                Line::from(vec![
                    Span::raw(sel_mark),
                    Span::styled(format!("{:<20}", item.label), label_style),
                    Span::raw("  "),
                    Span::styled(&item.value, val_style),
                ])
            }
            MenuItemKind::Value => {
                let sel_mark = if selected && !menu.editing { "● " } else { "  " };
                let label_style = if selected && !menu.editing {
                    Style::new().fg(Color::Yellow).bold()
                } else {
                    Style::new().fg(Color::White)
                };
                let display = if selected && menu.editing {
                    if menu.edit_buf.is_empty() { item.value.clone() } else { menu.edit_buf.clone() }
                } else {
                    item.value.clone()
                };
                Line::from(vec![
                    Span::raw(sel_mark),
                    Span::styled(format!("{:<20}", item.label), label_style),
                    Span::raw("  "),
                    Span::styled(display, Style::new().fg(Color::Rgb(180, 200, 220))),
                ])
            }
            MenuItemKind::Action => {
                let is_active = item.label.starts_with("▶");
                let sel_mark = if selected { "● " } else { "  " };
                let label_style = if selected {
                    Style::new().fg(Color::Yellow).bold()
                } else if is_active {
                    Style::new().fg(GREEN).bold()
                } else {
                    Style::new().fg(Color::White)
                };
                Line::from(vec![
                    Span::raw(sel_mark),
                    Span::styled(format!("{:<20}", item.label), label_style),
                    Span::raw("  "),
                    Span::styled(&item.value, Style::new().fg(Color::Gray)),
                ])
            }
        };
        lines.push(line);
    }

    let list_block = Block::new()
        .borders(Borders::ALL)
        .style(Style::new().bg(Color::Rgb(18, 22, 26)));
    frame.render_widget(Paragraph::new(lines).block(list_block), list_area);

    if !menu.status.is_empty() {
        frame.render_widget(
            Span::styled(format!("  {}", menu.status), Style::new().fg(GREEN)),
            status_area,
        );
    }

    let footer = Line::from(vec![
        Span::styled(" F10 ", Style::new().fg(Color::Black).bg(ACCENT)),
        Span::raw(menu.lang.t_menu_close()),
        Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)),
        Span::raw(menu.lang.t_menu_toggle_edit()),
        Span::styled(" Esc ", Style::new().fg(Color::Black).bg(Color::Gray)),
        Span::raw(menu.lang.t_menu_back_label()),
    ]);
    frame.render_widget(footer, footer_area);

    // Cursor for editing
    if menu.editing {
        let val_len = if menu.edit_buf.is_empty() {
            menu.items.get(menu.selected).map_or(0, |i| cjk_width(&i.value))
        } else {
            cjk_width(&menu.edit_buf)
        };
        let cursor_x = list_area.x + 25 + val_len.min(30) as u16;
        let row = (menu.selected.saturating_sub(scroll) + 1) as u16;
        let cursor_y = list_area.y + row;
        frame.set_cursor_position((cursor_x.min(area.width.saturating_sub(1)), cursor_y.min(area.height.saturating_sub(1))));
    }
}

/// Count visual rows after word-boundary wrapping, matching ratatui's WordWrapper.
fn count_wrap_rows(text: &str, width: usize) -> usize {
    if width == 0 || text.is_empty() {
        return 1;
    }
    let total_w = unicode_width::UnicodeWidthStr::width(text);
    if total_w <= width {
        return 1;
    }
    let mut rows = 1usize;
    let mut line_used = 0usize;
    for word in text.split_inclusive(char::is_whitespace) {
        let trimmed = word.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let word_w = unicode_width::UnicodeWidthStr::width(trimmed);
        // Long word that exceeds line width: break into multiple rows
        if word_w > width {
            if line_used > 0 {
                rows += 1; // move to next line first
                line_used = 0;
            }
            // Each full-width chunk is one row, plus partial remainder
            rows += word_w / width;
            line_used = word_w % width;
            if line_used > 0 {
                // remainder starts new line
            }
            continue;
        }
        let sep = if line_used == 0 { 0 } else { 1 };
        if line_used + sep + word_w > width {
            rows += 1;
            line_used = word_w;
        } else {
            line_used += sep + word_w;
        }
    }
    rows
}
