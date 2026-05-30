use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::{App, ChatRole};
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
            let langs = [(Lang::En, "English", "Use English throughout the interface"),
                         (Lang::Zh, "中文",   "界面和对话使用中文")];
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
            lines.push(Line::from(Span::styled("  ↑↓ to change, Enter to confirm", Style::new().fg(DIM))));
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
                let _total = show.len().min(6);
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
                Span::raw("  tokens"),
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

    let popup = centered_rect(70, (app.sessions.len() + 8).min(24).max(12) as u16, area);
    frame.render_widget(Clear, popup);

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" Sessions — Select or start new ")
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
        let summary: String = s.last_summary.chars().take(30).collect();
        lines.push(Line::from(vec![
            Span::raw(format!("  {mark} ")),
            Span::styled(&s.seed, Style::new().fg(Color::Yellow).bold()),
            Span::raw("  "),
            Span::styled(ts, Style::new().fg(Color::Gray)),
            Span::raw("  "),
            Span::styled(format!("msgs:{:<5}", s.message_count), Style::new().fg(DIM)),
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
        Span::styled("+ New Session", new_style),
    ]));

    frame.render_widget(Paragraph::new(lines), list_area);

    let help = Line::from(vec![
        Span::styled(" ↑↓ ", Style::new().fg(Color::Black).bg(ACCENT)),
        Span::raw(" select  "),
        Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)),
        Span::raw(" resume/new  "),
        Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)),
        Span::raw(" quit"),
    ]);
    frame.render_widget(help, help_area);
}

// ── Chat interface ──

pub fn render_chat(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let [header, body, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(3),
    ]).areas(area);

    let status_text = if app.streaming {
        format!("{} {}", app.spinner(), &app.status)
    } else {
        app.status.clone()
    };
    let header_text = Line::from(vec![
        Span::raw("DeepX | "),
        Span::styled(format!("Phase: {}", app.phase), Style::new().fg(Color::Cyan)),
        Span::raw(" | "),
        Span::styled(format!("Tokens: {}", app.session_tokens), Style::new().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(format!("({:.0}%)", if app.context_limit > 0 { app.session_tokens as f64 / app.context_limit as f64 * 100.0 } else { 0.0 }), Style::new().fg(Color::Gray)),
        if app.cache_hit > 0 || app.cache_miss > 0 {
            Span::raw(" ")
        } else { Span::raw("") },
        Span::styled(format!("hit:{}", app.cache_hit), Style::new().fg(Color::Rgb(100, 200, 120))),
        Span::styled("/", Style::new().fg(DIM)),
        Span::styled(format!("miss:{}", app.cache_miss), Style::new().fg(Color::Rgb(200, 150, 100))),
        if !app.balance.is_empty() {
            Span::raw(" | ")
        } else {
            Span::raw("")
        },
        Span::styled(&app.balance, Style::new().fg(Color::Rgb(100, 200, 255))),
        Span::raw(" | "),
        Span::styled(&status_text, Style::new().fg(if app.streaming { Color::Yellow } else { Color::Green })),
    ]);
    if !app.cache_warning.is_empty() {
        frame.render_widget(
            Span::styled(&app.cache_warning, Style::new().fg(Color::Red).bold()),
            Rect { x: area.x, y: area.y, width: area.width, height: 1 },
        );
    }
    frame.render_widget(header_text, header);

    let chat_block = Block::new().borders(Borders::ALL).title(" Chat ");
    let mut text_lines: Vec<Line> = Vec::new();
    for msg in &app.messages {
        match msg.role {
            ChatRole::Divider => {
                text_lines.push(Line::from(Span::styled("  ──", Style::new().fg(DIM))));
            }
            ChatRole::Status => {
                text_lines.push(Line::from(Span::styled(&msg.content, Style::new().fg(Color::Red))));
            }
            ChatRole::User => {
                text_lines.push(Line::from(vec![
                    Span::styled("You> ", Style::new().fg(Color::Green).bold()),
                    Span::raw(&msg.content),
                ]));
            }
            ChatRole::Thinking => {
                let prefix = Span::styled("Think> ", Style::new().fg(Color::Rgb(200, 180, 100)).bold());
                if msg.lines.is_empty() {
                    text_lines.push(Line::from(vec![prefix, Span::styled(
                        &msg.content, Style::new().fg(Color::Rgb(200, 180, 100)).italic(),
                    )]));
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
            ChatRole::Assistant => {
                let prefix = Span::styled("DeepX> ", Style::new().fg(Color::White).bold());
                if msg.lines.is_empty() {
                    text_lines.push(Line::from(vec![prefix, Span::raw(&msg.content)]));
                } else {
                    let first_char = msg.lines[0].spans.first()
                        .and_then(|s| s.content.chars().next());
                    let is_table = first_char.map_or(false, |c| {
                        c == '│' || c == '├' || c == '└' || c == '┌' || c == '┐' || c == '┘'
                    });

                    if is_table {
                        text_lines.push(Line::from(prefix.clone()));
                        for line in &msg.lines {
                            text_lines.push(line.clone());
                        }
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
            ChatRole::Tool => {
                for line in &msg.lines {
                    text_lines.push(line.clone());
                }
            }
        }
    }

    let content_height = body.height.saturating_sub(2) as usize;
    let body_width = body.width.saturating_sub(2) as usize;
    // Use logical line count for scroll — simpler and more predictable with wrapping
    let logical_lines = text_lines.len();
    let max_scroll = logical_lines.saturating_sub(content_height);
    let offset = if app.streaming { 0 } else { app.scroll_offset.min(max_scroll) };
    let scroll = max_scroll.saturating_sub(offset) as u16;

    let paragraph = Paragraph::new(text_lines)
        .block(chat_block)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, body);

    let input_block = Block::new()
        .borders(Borders::ALL)
        .title(" Input (Enter: send, Esc: cancel, Ctrl-C: quit) ");
    let input_text = if app.input.is_empty() {
        Line::from(Span::styled("Type a message...", Style::new().fg(Color::DarkGray)))
    } else {
        Line::from(Span::raw(&app.input))
    };
    frame.render_widget(Paragraph::new(input_text).block(input_block), input_area);

    let cursor_x = input_area.x + 1
        + (cjk_width(&app.input).min(input_area.width.saturating_sub(3)) as u16);
    frame.set_cursor_position((cursor_x.min(area.width.saturating_sub(1)), input_area.y + 1));

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
            .title(" Debug (F12) ")
            .style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&dbg_block, dbg_rect);

        let inner = dbg_block.inner(dbg_rect);
        let hp_dot = if d.hp_connected { ("●", Color::Green) } else { ("○", Color::Red) };
        let stream_dot = if d.streaming { ("●", Color::Yellow) } else { ("○", Color::Gray) };
        let lines = vec![
            Line::from(vec![
                Span::styled(format!(" HP: {} ", hp_dot.0), Style::new().fg(hp_dot.1)),
                Span::styled(format!("Stream: {} ", stream_dot.0), Style::new().fg(stream_dot.1)),
            ]),
            Line::from(vec![
                Span::styled("Session: ", Style::new().fg(Color::Gray)),
                Span::styled(&d.session_seed, Style::new().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Phase:  ", Style::new().fg(Color::Gray)),
                Span::styled(&d.current_phase, Style::new().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Context:", Style::new().fg(Color::Gray)),
                Span::styled(format!(" {} / 1M", d.context_tokens), Style::new().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("Tools:  ", Style::new().fg(Color::Gray)),
                Span::styled(format!("{} calls", d.tool_calls_total), Style::new().fg(Color::Cyan)),
                if d.tool_failures > 0 {
                    Span::styled(format!(" / {} fail", d.tool_failures), Style::new().fg(Color::Red))
                } else {
                    Span::raw("")
                },
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

pub fn render_ask(frame: &mut Frame, app: &App) {
    let ask = match &app.ask {
        Some(a) => a,
        None => return,
    };
    let area = frame.area();
    let h = (ask.options.len() + 5).min(20) as u16;
    let popup = centered_rect(60, h, area);
    frame.render_widget(Clear, popup);

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(255, 180, 100)))
        .title(" Ask ")
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
                Span::styled("Other: ", Style::new().fg(Color::Gray)),
                Span::styled(display, Style::new().fg(Color::Yellow).bold()),
            ]));
        } else if opt.is_empty() {
            lines.push(Line::from(vec![
                Span::raw(format!("  {mark} ")),
                Span::styled("Other (______)", Style::new().fg(Color::Gray)),
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
        " ↑↓ select  Enter confirm  Esc cancel",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}
