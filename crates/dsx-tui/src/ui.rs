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
    let input_line = content_area.y + 8;
    let cursor_x = if app.setup.step == 0 {
        (content_area.x + 16).min(popup.x + popup.width.saturating_sub(2))
    } else {
        (content_area.x + 2 + cjk_width(val).min(40)).min(popup.x + popup.width.saturating_sub(2))
    };
    frame.set_cursor_position((cursor_x, input_line));
}

// ── Chat interface ──

pub fn render_chat(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let [header, body, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(3),
    ]).areas(area);

    let header_text = Line::from(vec![
        Span::raw("DeepX | "),
        Span::styled(format!("Phase: {}", app.phase), Style::new().fg(Color::Cyan)),
        Span::raw(" | "),
        Span::styled(format!("Tokens: {}", app.tokens), Style::new().fg(Color::Yellow)),
        Span::raw(" | "),
        Span::styled(&app.status, Style::new().fg(Color::Green)),
    ]);
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
                text_lines.push(Line::from(vec![
                    Span::styled("Think> ", Style::new().fg(Color::Rgb(200, 180, 100)).bold()),
                    Span::styled(&msg.content, Style::new().fg(Color::Rgb(200, 180, 100)).italic()),
                ]));
            }
            ChatRole::Assistant => {
                text_lines.push(Line::from(vec![
                    Span::styled("Assistant> ", Style::new().fg(Color::White).bold()),
                    Span::raw(&msg.content),
                ]));
            }
            ChatRole::Tool => {
                text_lines.push(Line::from(vec![
                    Span::styled("  Tool> ", Style::new().fg(Color::Cyan).bold()),
                    Span::styled(&msg.content, Style::new().fg(Color::Gray)),
                ]));
            }
        }
    }

    let content_height = body.height.saturating_sub(2) as usize;
    let scroll = if app.streaming && text_lines.len() > content_height {
        text_lines.len().saturating_sub(content_height) as u16
    } else {
        app.scroll as u16
    };

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
        + (app.input.len().min(input_area.width.saturating_sub(3) as usize) as u16);
    frame.set_cursor_position((cursor_x.min(area.width.saturating_sub(1)), input_area.y + 1));
}
