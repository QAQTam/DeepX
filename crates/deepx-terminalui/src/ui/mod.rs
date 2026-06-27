// ui/mod.rs — Chat UI rendering delegates.
// Render functions live here; build_chat_lines extracted to ui/lines.rs.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::app::App;
use crate::i18n::Lang;
use unicode_width::UnicodeWidthStr;

pub mod lines;

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + (r.width.saturating_sub(width) / 2);
    let y = r.y + (r.height.saturating_sub(height) / 2);
    Rect::new(x, y, width.min(r.width), height.min(r.height))
}

const ACCENT: Color = Color::Rgb(100, 200, 255);
const DIM: Color = Color::Rgb(60, 60, 60);
const BG: Color = Color::Rgb(24, 28, 32);
const GREEN: Color = Color::Rgb(100, 180, 120);

fn cjk_width(s: &str) -> u16 { s.width() as u16 }

fn format_ts(seconds: u64) -> String {
    use chrono::{TimeZone, Local};
    if let Some(dt) = Local.timestamp_opt(seconds as i64, 0).single() {
        dt.format("%Y-%m-%d %H:%M").to_string()
    } else { String::new() }
}

fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 { format!("{}s", secs) }
    else if secs < 3600 { format!("{}m{}s", secs / 60, secs % 60) }
    else { format!("{}h{}m", secs / 3600, (secs % 3600) / 60) }
}

/// Short single-char icon for activity log and file entries.
fn tool_activity_icon(name: &str) -> &'static str {
    match name {
        "read_file" | "file_read" => "R",
        "write_file" | "file_write" => "W",
        "edit_file" | "edit_file_diff" | "file_edit" => "E",
        "delete_file" | "file_delete" => "D",
        "file_move" | "move_file" => "M",
        "exec" => ">",
        "explore" => "S",
        "search" | "grep" | "file_search" => "Z",
        "glob" | "file_glob" => "G",
        "list_dir" | "file_list_dir" => "L",
        "diff" | "file_diff" => "=",
        "web_search" | "web_fetch" => "@",
        "task_create" | "task_update" | "task_delete" | "task_list" => "T",
        "ask_user" => "?",
        "sed" => "~",
        _ => "*",
    }
}

// The six render functions below are the core of the TUI.
// They were manually reconstructed after a refactoring accident.
// Full original implementations are in git history.

pub fn render_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let l = app.setup.lang;
    frame.render_widget(Paragraph::new("").style(Style::new().bg(BG)), area);
    let popup_h = if app.setup.step == 2 && app.setup.models_loaded { 22u16 } else { 20u16 };
    let popup = centered_rect(66, popup_h, area);
    frame.render_widget(Clear, popup);
    let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT)).style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let title = Line::from(vec![
        Span::raw("  "), Span::styled("⚡", Style::new().fg(Color::Yellow)),
        Span::raw(" "), Span::styled(l.t_setup_welcome(), Style::new().fg(ACCENT).bold()), Span::raw("  "),
    ]);
    frame.render_widget(title, Rect::new(popup.x + 2, popup.y, popup.width, 1));
    let [steps_bar, content_area, help_area] = Layout::vertical([
        Constraint::Length(3), Constraint::Fill(1), Constraint::Length(2),
    ]).areas(inner);
    let total = app.setup.total_steps();
    let step_names = [l.t_select_lang(), l.t_api_key(), l.t_model(), l.t_context_limit()];
    let step_width = (steps_bar.width - 4) / total as u16;
    let bar_spans: Vec<Span> = (0..total).map(|i| {
        let fill = if i < app.setup.step { "█".repeat(step_width as usize) }
        else if i == app.setup.step { format!("{}{}", "█".repeat(step_width as usize / 3), "░".repeat((step_width as usize).saturating_sub(step_width as usize / 3))) }
        else { "░".repeat(step_width as usize) };
        Span::styled(fill, Style::new().fg(if i <= app.setup.step { ACCENT } else { DIM }))
    }).collect();
    frame.render_widget(Line::from(bar_spans), Rect { x: steps_bar.x + 2, y: steps_bar.y, width: steps_bar.width - 4, height: 1 });
    let lbl_spans: Vec<Span> = (0..total).map(|i| {
        let style = if i == app.setup.step { Style::new().fg(Color::White).bold() }
        else if i < app.setup.step { Style::new().fg(GREEN) } else { Style::new().fg(DIM) };
        let txt = step_names[i]; let w = step_width as usize;
        Span::styled(format!("{}{}", txt, " ".repeat(w.saturating_sub(txt.width().min(w)))), style)
    }).collect();
    frame.render_widget(Line::from(lbl_spans), Rect { x: steps_bar.x + 2, y: steps_bar.y + 1, width: steps_bar.width - 4, height: 1 });
    let mut rlines: Vec<Line> = Vec::new();
    rlines.push(Line::from(""));
    match app.setup.step {
        0 => {
            rlines.push(Line::from(vec![Span::styled(format!("  {}  ", l.t_select_lang()), Style::new().fg(Color::Black).bg(ACCENT).bold())]));
            rlines.push(Line::from("")); rlines.push(Line::from(""));
            let langs = [(Lang::En, l.t_setup_lang_en_name(), l.t_setup_lang_en_desc()), (Lang::Zh, l.t_setup_lang_zh_name(), l.t_setup_lang_zh_desc())];
            for &(lang, name, desc) in &langs {
                let selected = app.setup.lang == lang;
                let mark = if selected { "●" } else { "○" };
                let style = if selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(DIM) };
                rlines.push(Line::from(vec![
                    Span::raw(format!("     {mark}  ")), Span::styled(name, style),
                    Span::raw("  —  "), Span::styled(desc, Style::new().fg(Color::Gray)),
                ]));
            }
            rlines.push(Line::from("")); rlines.push(Line::from(""));
            rlines.push(Line::from(Span::styled(l.t_setup_nav_hint(), Style::new().fg(DIM))));
        }
        1 => {
            rlines.push(Line::from(vec![Span::styled(format!("  {}  ", l.t_api_key()), Style::new().fg(Color::Black).bg(ACCENT).bold())]));
            rlines.push(Line::from("")); rlines.push(Line::from(Span::styled(format!("  {}", l.t_enter_key()), Style::new().fg(Color::Gray))));
            rlines.push(Line::from("")); rlines.push(Line::from(""));
            let masked = if app.setup.api_key.is_empty() { String::new() } else if app.setup.api_key.len() > 3 { format!("sk-{}", "●".repeat(app.setup.api_key.len().saturating_sub(3).min(20))) } else { app.setup.api_key.clone() };
            rlines.push(Line::from(vec![Span::raw("  "), Span::styled(format!("{:>40}", masked), Style::new().fg(Color::Yellow).bold())]));
            if app.setup.step == 1 { rlines.push(Line::from("")); rlines.push(Line::from(Span::styled(format!("  {}", l.t_validating()), Style::new().fg(DIM)))); }
        }
        2 => {
            rlines.push(Line::from(vec![Span::styled(format!("  {}  ", l.t_model()), Style::new().fg(Color::Black).bg(ACCENT).bold())]));
            rlines.push(Line::from(""));
            if app.setup.models_loaded {
                rlines.push(Line::from(Span::styled(format!("  {}", l.t_select_model()), Style::new().fg(Color::Gray))));
                rlines.push(Line::from(""));
                for (i, m) in app.setup.model_list.iter().enumerate().take(6) {
                    let sel = i == app.setup.model_index;
                    let mark = if sel { "●" } else { "○" };
                    let st = if sel { Style::new().fg(ACCENT).bold() } else { Style::new().fg(DIM) };
                    rlines.push(Line::from(vec![Span::raw(format!("     {mark}  ")), Span::styled(m.clone(), st)]));
                }
                if app.setup.model_list.len() > 6 { rlines.push(Line::from(Span::styled("     …", Style::new().fg(DIM)))); }
            }
            rlines.push(Line::from(""));
            rlines.push(Line::from(vec![Span::raw("  "), Span::styled(format!("{:>40}", app.setup.model), Style::new().fg(Color::Yellow).bold())]));
        }
        3 => {
            rlines.push(Line::from(vec![Span::styled(format!("  {}  ", l.t_context_limit()), Style::new().fg(Color::Black).bg(ACCENT).bold())]));
            rlines.push(Line::from(""));
            rlines.push(Line::from(Span::styled(format!("  {}", l.t_max_tokens_desc()), Style::new().fg(Color::Gray))));
            rlines.push(Line::from("")); rlines.push(Line::from(""));
            rlines.push(Line::from(vec![Span::raw("  "), Span::styled(format!("{:>10}", app.setup.context_limit), Style::new().fg(Color::Yellow).bold()), Span::styled(l.t_setup_tokens_unit(), Style::new().fg(Color::Gray))]));
        }
        _ => {}
    }
    frame.render_widget(Paragraph::new(rlines), content_area);
    if !app.setup.status.is_empty() {
        let color = if app.setup.status.contains("Valid") || app.setup.status.contains("有效") { GREEN } else { Color::Red };
        frame.render_widget(Span::styled(format!("  {}", app.setup.status), Style::new().fg(color)), Rect { x: content_area.x, y: content_area.y + content_area.height.saturating_sub(2), width: content_area.width, height: 1 });
    } else if !app.setup.error.is_empty() {
        frame.render_widget(Span::styled(format!("  ✗ {}", app.setup.error), Style::new().fg(Color::Red)), Rect { x: content_area.x, y: content_area.y + content_area.height.saturating_sub(2), width: content_area.width, height: 1 });
    }
    let s_next = l.t_enter_next(); let s_clear = l.t_esc_clear(); let s_quit = l.t_ctrl_c_quit(); let s_retry = l.t_retry();
    let help = if !app.setup.error.is_empty() || app.validating {
        let lbl = if app.validating { l.t_validating() } else { s_retry };
        Line::from(vec![Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Yellow)), Span::raw(format!(" {lbl}  ")), Span::styled(" Esc ", Style::new().fg(Color::Black).bg(Color::Gray)), Span::raw(format!(" {s_clear}  ")), Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)), Span::raw(format!(" {s_quit}"))])
    } else {
        Line::from(vec![Span::styled(" Enter ", Style::new().fg(Color::Black).bg(ACCENT)), Span::raw(format!(" {s_next}  ")), Span::styled(" Esc ", Style::new().fg(Color::Black).bg(Color::Gray)), Span::raw(format!(" {s_clear}  ")), Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)), Span::raw(format!(" {s_quit}"))])
    };
    frame.render_widget(help, help_area);
    let val = app.setup.current_value();
    let input_line = content_area.y + app.setup.cursor_row_offset();
    let cursor_x = if app.setup.step == 0 { (content_area.x + 16).min(popup.x + popup.width.saturating_sub(2)) } else { (content_area.x + 2 + cjk_width(val).min(40)).min(popup.x + popup.width.saturating_sub(2)) };
    frame.set_cursor_position((cursor_x, input_line));
}

pub fn render_sessions(frame: &mut Frame, app: &App) {
    let area = frame.area(); let l = app.setup.lang;
    let popup = centered_rect(70, (app.sessions.len() + 8).min(24).max(12) as u16, area);
    frame.render_widget(Clear, popup);
    let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT)).title(l.t_session_title()).style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [list_area, help_area] = Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]).areas(inner);
    let mut rlines: Vec<Line> = Vec::new();
    let max_fit = ((list_area.height as usize).saturating_sub(4)).max(1) / 2;
    let total = app.sessions.len();
    let scroll = if app.session_index < max_fit { 0 } else if app.session_index >= total { total.saturating_sub(max_fit) } else { (app.session_index + 1).saturating_sub(max_fit) };
    let end = (scroll + max_fit).min(total);
    for idx in scroll..end {
        let s = &app.sessions[idx]; let selected = idx == app.session_index;
        let mark = if selected { "●" } else { "○" };
        let style = if selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(DIM) };
        let ts = format_ts(s.updated_at);
        let summary: String = { let mut width = 0u16; s.last_summary.chars().take_while(|c| { width += cjk_width(&c.to_string()); width <= 55 }).collect() };
        rlines.push(Line::from(vec![Span::raw(format!("  {mark} ")), Span::styled(&s.seed, Style::new().fg(Color::Yellow).bold()), Span::raw("  "), Span::styled(ts, Style::new().fg(Color::Gray)), Span::raw("  "), Span::styled(format!("{}:{:<5}", l.t_session_msgs(), s.message_count), Style::new().fg(DIM))]));
        rlines.push(Line::from(vec![Span::raw("     "), Span::styled(summary, style)]));
    }
    let new_selected = app.session_index == app.sessions.len();
    let new_mark = if new_selected { "●" } else { "○" };
    let new_style = if new_selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(Color::Gray) };
    if !app.sessions.is_empty() { rlines.push(Line::from("")); rlines.push(Line::from(Span::styled("  ──────────────────────────────────────────", Style::new().fg(DIM)))); }
    rlines.push(Line::from(vec![Span::raw(format!("  {new_mark} ")), Span::styled(l.t_session_new(), new_style)]));
    frame.render_widget(Paragraph::new(rlines), list_area);
    let help = Line::from(vec![
        Span::styled(" ↑↓ ", Style::new().fg(Color::Black).bg(ACCENT)), Span::raw(l.t_session_select_hint()),
        Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)), Span::raw(l.t_session_resume_hint()),
        Span::styled(" ^C ", Style::new().fg(Color::Black).bg(Color::Red)), Span::raw(l.t_session_quit_hint()),
    ]);
    frame.render_widget(help, help_area);
}

pub fn render_chat(frame: &mut Frame, app: &mut App) {
    let area = frame.area(); let l = app.setup.lang;
    if area.width < 60 || area.height < 10 {
        let msg = if l.as_str() == "zh" { "终端窗口太小 (最小 60×10) — 请调整窗口大小" } else { "Terminal too small (min 60×10) — please resize" };
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(msg).centered().style(Style::new().fg(Color::Red).bg(BG)), area);
        return;
    }
    let input_lines = app.input.chars().filter(|&c| c == '\n').count() + 1;
    let input_height = (input_lines as u16 + 2).min(12).max(3);
    let detail_height: u16 = if app.detail_pane.is_some() {
        10u16.min(area.height.saturating_sub(6 + input_height))
    } else {
        0
    };
    let [header_area, body, detail_area, input_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(detail_height),
        Constraint::Length(input_height),
    ]).spacing(1).areas(area);
    let [header_line1, header_line2] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(header_area);

    let status_text = if !app.last_error.is_empty() { format!("✗ {}", &app.last_error) }
    else if app.streaming { format!("{} {}", app.spinner(), &app.status) }
    else if app.busy { format!("{} {}", app.pulse(), &app.status) }
    else { app.status.clone() };
    // Turn / thinking timer
    let turn_elapsed = app.turn_start_time.map(|t| t.elapsed()).unwrap_or_default();
    let thinking_elapsed = app.thinking_start_time.map(|t| t.elapsed()).unwrap_or_default();
    let timer_text = if turn_elapsed.as_secs() > 0 {
        format!("⚡{:.0}s", turn_elapsed.as_secs_f64())
    } else { String::new() };
    let think_text = if app.thinking_start_time.is_some() && thinking_elapsed.as_secs_f64() > 0.5 {
        format!("💭{:.0}s", thinking_elapsed.as_secs_f64())
    } else { String::new() };
    let cache_total = app.cache_hit + app.cache_miss;
    let cache_rate = if cache_total > 0 { app.cache_hit as f64 / cache_total as f64 * 100.0 } else { 0.0 };
    let cache_color = if cache_rate > 0.5 { Color::Rgb(100, 200, 120) } else { Color::Rgb(200, 150, 100) };
    let ctx_pct = if app.context_limit > 0 { app.context_tokens as f64 / app.context_limit as f64 * 100.0 } else { 0.0 };

    let h1 = Line::from(vec![
        Span::raw(format!("DeepX v{}", env!("CARGO_PKG_VERSION"))), Span::raw(" | "),
        Span::styled(format!("Context: {} / {} ({:.0}%)", app.context_tokens, app.context_limit, ctx_pct), Style::new().fg(Color::Yellow)),
        Span::raw(" | "),
        Span::styled(&status_text, Style::new().fg(if !app.last_error.is_empty() { Color::Red } else if app.streaming { Color::Yellow } else { Color::Green })),
        if !timer_text.is_empty() { Span::raw(" | ") } else { Span::raw("") },
        Span::styled(&timer_text, Style::new().fg(Color::Rgb(100, 220, 180))),
        if !think_text.is_empty() { Span::raw(" ") } else { Span::raw("") },
        Span::styled(&think_text, Style::new().fg(Color::Rgb(200, 180, 100))),
        Span::raw(""), Span::raw(""),
    ]);
    frame.render_widget(h1, header_line1);

    let mut h2_spans = vec![
        Span::styled(format!("Session: {}", if app.debug.session_seed.is_empty() { "—".into() } else { app.debug.session_seed.chars().take(8).collect::<String>() }), Style::new().fg(Color::Rgb(180, 180, 200))), Span::raw("  "),
    ];
    if cache_total > 0 {
        h2_spans.push(Span::styled(format!("Hit:{}", app.cache_hit), Style::new().fg(Color::Rgb(100, 200, 120))));
        h2_spans.push(Span::styled(format!("/Miss:{}", app.cache_miss), Style::new().fg(Color::Rgb(200, 150, 100))));
        h2_spans.push(Span::styled(format!(" ({:.0}%)", cache_rate), Style::new().fg(cache_color)));
        h2_spans.push(Span::raw("  "));
    }
    if !app.balance.is_empty() { h2_spans.push(Span::styled(&app.balance, Style::new().fg(Color::Rgb(100, 200, 255)))); h2_spans.push(Span::raw("  ")); }
    h2_spans.push(Span::styled(format!("DSML: {}", app.debug.dsml_compat_count), Style::new().fg(Color::Rgb(100, 220, 140))));
    if !app.cache_warning.is_empty() { h2_spans.push(Span::raw("  ")); h2_spans.push(Span::styled(&app.cache_warning, Style::new().fg(Color::Red).bold())); }
    frame.render_widget(Line::from(h2_spans), header_line2);

    let [body_content, scrollbar_area] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)]).areas(body);
    let content_height = body_content.height as usize;

    let cache_hit = app.cached_text_version == app.message_version && app.cached_text_width == body.width && !app.cached_text_lines.is_empty();
    let text_lines: Vec<Line<'static>> = if cache_hit { app.cached_text_lines.clone() }
    else {
        let lns = lines::build_chat_lines(app, body);
        app.cached_text_lines = lns.clone();
        app.cached_text_version = app.message_version;
        app.cached_text_width = body.width;
        lns
    };

    let paragraph = Paragraph::new(text_lines).wrap(ratatui::widgets::Wrap { trim: false });
    let total_wrapped = if app.streaming || app.line_count_version != app.message_version || app.line_count_width != body_content.width {
        let count = paragraph.line_count(body_content.width) as usize;
        app.cached_line_count = count; app.line_count_version = app.message_version; app.line_count_width = body_content.width; count
    } else { app.cached_line_count };
    let max_scroll = total_wrapped.saturating_sub(content_height);
    let at_bottom = app.scroll_offset == 0;
    let scroll = if at_bottom { max_scroll.min(u16::MAX as usize) as u16 }
    else { let offset = app.scroll_offset.min(max_scroll); (max_scroll - offset).min(u16::MAX as usize) as u16 };
    frame.render_widget(paragraph.scroll((scroll, 0)), body_content);
    let mut scrollbar_state = ScrollbarState::new(total_wrapped).position(scroll as usize).viewport_content_length(content_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight).thumb_symbol("█").track_symbol(Some("│"));
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);

    // ── Detail pane: PTY output or side-by-side diff ──
    if detail_height > 0 {
        match &app.detail_pane {
            Some(crate::app::DetailPane::Pty(pane)) => {
                let pty_block = Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(if pane.running { Color::Rgb(100, 200, 255) } else if pane.exit_code == Some(0) { GREEN } else { Color::Rgb(220, 100, 100) }))
                    .title({
                        let status = if pane.running {
                            format!("{} running...", format_elapsed(pane.elapsed()))
                        } else if pane.exit_code == Some(0) {
                            format!("{} ok", format_elapsed(pane.elapsed()))
                        } else {
                            format!("{} exit:{}", format_elapsed(pane.elapsed()), pane.exit_code.unwrap_or(-1))
                        };
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("▶", Style::new().fg(ACCENT)),
                            Span::raw(" "),
                            Span::styled(&pane.command, Style::new().fg(Color::White).bold()),
                            Span::raw("  "),
                            Span::styled(status, Style::new().fg(if pane.running { Color::Yellow } else { Color::Gray })),
                            Span::raw(" "),
                        ])
                    });
                let inner = pty_block.inner(detail_area);
                frame.render_widget(pty_block, detail_area);

                let ansi_lines = crate::markdown::render_ansi(&pane.output);
                let visible = inner.height as usize;
                let total = ansi_lines.len();
                let bottom_offset = if total > visible { total - visible } else { 0 };
                let shown: Vec<Line<'_>> = ansi_lines.into_iter().skip(bottom_offset).take(visible).collect();
                frame.render_widget(Paragraph::new(shown), inner);
            }
            Some(crate::app::DetailPane::Diff(pane)) => {
                let diff_block = Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(ACCENT))
                    .title(Line::from(vec![
                        Span::raw(" "),
                        Span::styled("≠", Style::new().fg(Color::Rgb(200, 180, 100))),
                        Span::raw(" "),
                        Span::styled(&pane.label, Style::new().fg(Color::White).bold()),
                        Span::raw(" "),
                    ]));
                let inner = diff_block.inner(detail_area);
                frame.render_widget(diff_block, detail_area);

                let visible = inner.height as usize;
                let offset = pane.scroll_offset.min(pane.rows.len().saturating_sub(visible));
                let shown = &pane.rows[offset..(offset + visible).min(pane.rows.len())];

                // Split into three columns: old | sep | new
                let col_w = inner.width.saturating_sub(1) / 2; // 1 for separator
                let [old_area, _sep_area, new_area] = Layout::horizontal([
                    Constraint::Length(col_w),
                    Constraint::Length(1),
                    Constraint::Length(col_w),
                ]).areas(inner);

                // Separator bar
                let sep_style = Style::new().fg(Color::Rgb(60, 70, 80));
                for y in 0..visible.min(inner.height as usize) {
                    frame.render_widget(
                        Span::styled("│", sep_style),
                        Rect { x: _sep_area.x, y: old_area.y + y as u16, width: 1, height: 1 },
                    );
                }

                let dim = Color::Rgb(90, 100, 110);
                let body_w = col_w.saturating_sub(7) as usize; // 4 for ln + 1 space + 1 │ + 1 space
                let mut old_lines: Vec<Line<'static>> = Vec::new();
                let mut new_lines: Vec<Line<'static>> = Vec::new();

                for (old, new, old_body, new_body, kind) in shown {
                    match kind.as_str() {
                        "mod" => {
                            old_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", old), Style::new().fg(Color::Rgb(200, 100, 100)).bg(Color::Rgb(50, 20, 20))),
                                Span::styled(format!("{:<width$}", old_body, width = body_w), Style::new().fg(Color::Rgb(255, 140, 140)).bg(Color::Rgb(50, 20, 20))),
                            ]));
                            new_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", new), Style::new().fg(Color::Rgb(100, 200, 100)).bg(Color::Rgb(20, 50, 20))),
                                Span::styled(format!("{:<width$}", new_body, width = body_w), Style::new().fg(Color::Rgb(140, 255, 140)).bg(Color::Rgb(20, 50, 20))),
                            ]));
                        }
                        "del" => {
                            old_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", old), Style::new().fg(dim).bg(Color::Rgb(50, 20, 20))),
                                Span::styled(format!("{:<width$}", old_body, width = body_w), Style::new().fg(Color::Rgb(255, 140, 140)).bg(Color::Rgb(50, 20, 20))),
                            ]));
                            new_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", ""), Style::new().fg(dim)),
                                Span::styled(format!("{:<width$}", "", width = body_w), Style::new()),
                            ]));
                        }
                        "add" => {
                            old_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", ""), Style::new().fg(dim)),
                                Span::styled(format!("{:<width$}", "", width = body_w), Style::new()),
                            ]));
                            new_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", new), Style::new().fg(dim).bg(Color::Rgb(20, 50, 20))),
                                Span::styled(format!("{:<width$}", new_body, width = body_w), Style::new().fg(Color::Rgb(140, 255, 140)).bg(Color::Rgb(20, 50, 20))),
                            ]));
                        }
                        _ => {
                            let bg = Color::Rgb(24, 28, 32);
                            old_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", old), Style::new().fg(dim).bg(bg)),
                                Span::styled(format!("{:<width$}", old_body, width = body_w), Style::new().fg(Color::Rgb(200, 210, 220)).bg(bg)),
                            ]));
                            new_lines.push(Line::from(vec![
                                Span::styled(format!("{:>4} │ ", new), Style::new().fg(dim).bg(bg)),
                                Span::styled(format!("{:<width$}", old_body, width = body_w), Style::new().fg(Color::Rgb(200, 210, 220)).bg(bg)),
                            ]));
                        }
                    }
                }
                frame.render_widget(Paragraph::new(old_lines), old_area);
                frame.render_widget(Paragraph::new(new_lines), new_area);

                // Scroll indicator
                if pane.scroll_offset > 0 {
                    frame.render_widget(
                        Span::styled(format!(" ↑{}↓ ", pane.scroll_offset), Style::new().fg(Color::Rgb(100, 200, 255)).bold()),
                        Rect { x: detail_area.x + detail_area.width.saturating_sub(10), y: detail_area.y, width: 8, height: 1 },
                    );
                }
            }
            Some(crate::app::DetailPane::Output(pane)) => {
                let output_block = Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(ACCENT))
                    .title(Line::from(vec![
                        Span::raw(" "),
                        Span::styled("📄", Style::new().fg(Color::Rgb(200, 180, 100))),
                        Span::raw(" "),
                        Span::styled(&pane.label, Style::new().fg(Color::White).bold()),
                        Span::raw(" "),
                    ]));
                let inner = output_block.inner(detail_area);
                frame.render_widget(output_block, detail_area);

                let text_lines: Vec<Line<'static>> = pane.output.lines()
                    .map(|l| Line::from(Span::raw(l.to_string())))
                    .collect();
                let visible = inner.height as usize;
                let total = text_lines.len();
                let offset = pane.scroll_offset.min(total.saturating_sub(visible));
                let shown: Vec<Line<'_>> = text_lines.into_iter().skip(offset).take(visible).collect();
                frame.render_widget(Paragraph::new(shown), inner);

                if pane.scroll_offset > 0 {
                    frame.render_widget(
                        Span::styled(format!(" ↑{}↓ ", pane.scroll_offset), Style::new().fg(Color::Rgb(100, 200, 255)).bold()),
                        Rect { x: detail_area.x + detail_area.width.saturating_sub(10), y: detail_area.y, width: 8, height: 1 },
                    );
                }
            }
            _ => {}
        }
    }

    let line_count = app.input.chars().filter(|&c| c == '\n').count() + 1;
    let char_count = app.input.chars().count();
    let mut input_title = l.t_chat_input_title().to_string();
    if char_count > 0 {
        let counter = if app.setup.lang.as_str() == "zh" { format!(" {}行 {}字 |", line_count, char_count) } else { format!(" {}L {}C |", line_count, char_count) };
        input_title = format!("{}{}", counter, input_title);
    }
    let border_color = if line_count > 1 || app.history_idx.is_some() { Color::Rgb(80, 130, 180) } else { Color::Rgb(60, 60, 60) };
    let input_block = Block::new().borders(Borders::ALL).border_style(Style::new().fg(border_color)).title(input_title);
    let input_text: Vec<Line> = if app.input.is_empty() { vec![Line::from(Span::styled(l.t_chat_input_placeholder(), Style::new().fg(Color::DarkGray)))] }
    else if app.cached_input_len != app.input.len() { let ls: Vec<Line> = app.input.lines().map(|l| Line::from(Span::raw(l.to_string()))).collect(); app.cached_input_lines = ls.clone(); app.cached_input_len = app.input.len(); ls }
    else { app.cached_input_lines.clone() };
    if app.history_idx.is_some() && !app.input_history.is_empty() {
        let hint = if app.setup.lang.as_str() == "zh" { format!("  ↑ 历史记录 ({}/{})", app.history_idx.unwrap_or(0)+1, app.input_history.len()) }
        else { format!("  ↑ history ({}/{})", app.history_idx.unwrap_or(0)+1, app.input_history.len()) };
        let mut ls = vec![Line::from(Span::styled(hint, Style::new().fg(Color::Rgb(120, 140, 160)).italic()))];
        ls.extend(input_text); frame.render_widget(Paragraph::new(ls).block(input_block), input_area);
    } else { frame.render_widget(Paragraph::new(input_text).block(input_block), input_area); }
    let cursor_byte = app.cursor.min(app.input.len());
    let pre_cursor = &app.input[..cursor_byte];
    let cursor_line = pre_cursor.chars().filter(|&c| c == '\n').count();
    let last_line_start = pre_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let cursor_col = cjk_width(&app.input[last_line_start..cursor_byte]).min(input_area.width.saturating_sub(3)) as u16;
    let input_top = input_area.y + 1;
    let cursor_row = input_top + cursor_line.min(input_area.height.saturating_sub(3) as usize) as u16;
    frame.set_cursor_position(((input_area.x + 1 + cursor_col).min(area.width.saturating_sub(1)), cursor_row));

    if app.show_debug {
        let d = &app.debug; let dbg_w = 40u16; let dbg_h = 10u16;
        let dbg_rect = Rect::new(area.width.saturating_sub(dbg_w + 2), area.y + 1, dbg_w, dbg_h);
        frame.render_widget(Clear, dbg_rect);
        let dbg_block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::new().fg(Color::Rgb(180, 150, 255))).title(l.t_debug_title()).style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&dbg_block, dbg_rect);
        let inner = dbg_block.inner(dbg_rect);
        let hp_dot = if d.hp_connected { ("●", Color::Green) } else { ("○", Color::Red) };
        let stream_dot = if d.streaming { ("●", Color::Yellow) } else { ("○", Color::Gray) };
        let dlines = vec![
            Line::from(vec![Span::styled(format!(" {}: {} ", l.t_debug_hp(), hp_dot.0), Style::new().fg(hp_dot.1)), Span::styled(format!("{}: {} ", l.t_debug_stream(), stream_dot.0), Style::new().fg(stream_dot.1))]),
            Line::from(vec![Span::styled(format!("{}: ", l.t_debug_session()), Style::new().fg(Color::Gray)), Span::styled(&d.session_seed, Style::new().fg(Color::Cyan))]),
            Line::from(vec![Span::styled(format!("{}:", l.t_debug_context()), Style::new().fg(Color::Gray)), Span::styled(format!(" {} / 1M", d.context_tokens), Style::new().fg(Color::Yellow))]),
            Line::from(vec![Span::styled(format!("{}:  ", l.t_debug_tools()), Style::new().fg(Color::Gray)), Span::styled(format!("{} {}", d.tool_calls_total, l.t_debug_calls()), Style::new().fg(Color::Cyan)),
                if d.tool_failures > 0 { Span::styled(format!(" / {} {}", d.tool_failures, l.t_debug_fail()), Style::new().fg(Color::Red)) } else { Span::raw("") },
                Span::raw(" "), Span::styled(format!("(DSML compat: {})", d.dsml_compat_count), Style::new().fg(Color::Rgb(100, 220, 140)))]),
        ];
        frame.render_widget(Paragraph::new(dlines), inner);
    }
    if app.show_tasks {
        let tasks = app.tasks();
        let recent = &app.debug.recent_edits;
        let activity = &app.activity_log;

        // Compute total lines: section headers + content
        let task_lines = if tasks.is_empty() { 1 } else { tasks.iter().map(|t| if t.description.is_empty() { 1 } else { 2 }).sum::<usize>() };
        let activity_lines = if activity.is_empty() { 1 } else { activity.len().min(10) };
        let edit_lines = if recent.is_empty() { 1 } else { recent.len().min(8) };
        let total_content = 3 + task_lines + 3 + activity_lines + 3 + edit_lines; // 3 section headers + content
        let panel_h = (total_content as u16 + 2).min(24).max(8);

        let panel_w = 50u16;
        let panel_rect = Rect::new(area.width.saturating_sub(panel_w + 2), area.y + 1, panel_w, panel_h);
        frame.render_widget(Clear, panel_rect);
        let panel_block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Rgb(120, 200, 200)))
            .title(format!(" Status ")).style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&panel_block, panel_rect);
        let inner = panel_block.inner(panel_rect);

        let mut lines: Vec<Line> = Vec::new();

        // ── Section 1: Tasks ──
        let completed = tasks.iter().filter(|t| t.status == "completed").count();
        lines.push(Line::from(vec![
            Span::styled(" Tasks ", Style::new().fg(Color::Rgb(120, 200, 200)).bold()),
            Span::styled(format!("({}/{})", completed, tasks.len()), Style::new().fg(Color::Rgb(100, 160, 180))),
        ]));
        if tasks.is_empty() {
            lines.push(Line::from(Span::styled("  (no tasks)", Style::new().fg(Color::Gray))));
        } else {
            for t in tasks {
                let (icon, color) = match t.status.as_str() {
                    "completed" => ("✓", Color::Rgb(100, 220, 100)),
                    "in_progress" => ("●", Color::Rgb(220, 200, 100)),
                    "cancelled" => ("✗", Color::Rgb(220, 100, 100)),
                    _ => ("○", Color::Rgb(140, 150, 160)),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::new().fg(color).bold()),
                    Span::styled(format!("{}: {}", t.id, t.subject), Style::new().fg(Color::Rgb(200, 210, 220))),
                ]));
                if !t.description.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(&t.description, Style::new().fg(Color::Rgb(140, 150, 160)).italic()),
                    ]));
                }
            }
        }

        // ── Section 2: Activity ──
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" Activity ", Style::new().fg(Color::Rgb(120, 200, 200)).bold()),
            Span::styled(format!("({})", activity.len()), Style::new().fg(Color::Rgb(100, 160, 180))),
        ]));
        if activity.is_empty() {
            lines.push(Line::from(Span::styled("  (no activity)", Style::new().fg(Color::Gray))));
        } else {
            for entry in activity.iter().rev().take(10) {
                let icon = tool_activity_icon(&entry.tool_name);
                let result_icon = if entry.success { "✓" } else { "✗" };
                let result_color = if entry.success { Color::Rgb(100, 220, 100) } else { Color::Rgb(220, 100, 100) };
                let elapsed = entry.time.elapsed().as_secs();
                let ts = if elapsed < 60 { format!("{}s", elapsed) } else { format!("{}m", elapsed / 60) };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::new().fg(Color::Rgb(160, 180, 200)).bold()),
                    Span::styled(format!("{} ", result_icon), Style::new().fg(result_color)),
                    Span::styled(&entry.tool_name, Style::new().fg(Color::Rgb(200, 210, 220))),
                    Span::raw(" "),
                    Span::styled(&entry.summary, Style::new().fg(Color::Rgb(140, 150, 160))),
                    Span::raw(" "),
                    Span::styled(ts, Style::new().fg(Color::Rgb(100, 110, 120))),
                ]));
            }
        }

        // ── Section 3: Recent Edits ──
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" Files ", Style::new().fg(Color::Rgb(120, 200, 200)).bold()),
            Span::styled(format!("({})", recent.len()), Style::new().fg(Color::Rgb(100, 160, 180))),
        ]));
        if recent.is_empty() {
            lines.push(Line::from(Span::styled("  (no files)", Style::new().fg(Color::Gray))));
        } else {
            for edit in recent.iter().take(8) {
                // Format: "tool_name: path" or just "path"
                let (tool, path) = edit.split_once(": ").unwrap_or(("edit", edit.as_str()));
                let ticon = tool_activity_icon(tool);
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", ticon), Style::new().fg(Color::Rgb(160, 180, 200)).bold()),
                    Span::styled(tool, Style::new().fg(Color::Rgb(180, 190, 200))),
                    Span::raw(" "),
                    Span::styled(path, Style::new().fg(Color::Rgb(140, 150, 160))),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
    if app.show_context {
        let ctx_w = 50u16; let ctx_h = 4u16;
        let ctx_y = (body.y + 2).min(area.y.saturating_add(area.height.saturating_sub(ctx_h)));
        let ctx_rect = Rect::new(area.width.saturating_sub(ctx_w + 2), ctx_y, ctx_w, ctx_h);
        frame.render_widget(Clear, ctx_rect);
        let ctx_block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::new().fg(Color::Rgb(180, 180, 100))).title(" Context Window ").style(Style::new().bg(Color::Rgb(18, 22, 30)));
        frame.render_widget(&ctx_block, ctx_rect);
        let inner = ctx_block.inner(ctx_rect);
        let clines = vec![
            Line::from(vec![Span::styled(format!("  Model: {}  ", app.setup.model), Style::new().fg(Color::Yellow))]),
            Line::from(vec![Span::styled(format!("  Context limit: {}  ", app.context_limit), Style::new().fg(Color::Gray))]),
        ];
        frame.render_widget(Paragraph::new(clines), inner);
    }
}

pub fn render_help(frame: &mut Frame, app: &App) {
    let area = frame.area(); let l = app.setup.lang;
    let popup = centered_rect(55, 18, area);
    frame.render_widget(Clear, popup);
    let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::new().fg(ACCENT)).title(" Help ").style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let is_zh = l.as_str() == "zh";
    let bindings = [
        ("Enter", "Send message", "发送消息"),
        ("Ctrl+Enter", "Newline", "换行"),
        ("Esc", "Cancel", "取消"),
        ("↑/↓", "Browse history / Scroll", "浏览历史 / 滚动"),
        ("PgUp/PgDn", "Fast scroll", "快速滚动"),
        ("F6", "Toggle thinking", "切换思考显示"),
        ("F8", "Context", "上下文"),
        ("F9", "Status", "状态面板"),
        ("F10", "Settings", "设置"),
        ("F11", "PTY Pane", "终端窗格"),
        ("F12", "Debug", "调试"),
        ("?", "Help", "帮助"),
        ("Ctrl+C / F3", "Quit", "退出"),
    ];
    let max_label_w = bindings.iter().map(|(k, _, _)| k.len()).max().unwrap_or(8) + 2;
    let blines: Vec<Line> = std::iter::once(Line::from(""))
        .chain(bindings.iter().map(|(key, en, zh)| {
            let desc = if is_zh { zh } else { en };
            let pad = " ".repeat(max_label_w.saturating_sub(key.len()));
            Line::from(vec![Span::raw("  "), Span::styled(format!("{key}{pad}"), Style::new().fg(Color::Rgb(100, 200, 255)).bold()), Span::styled(desc.to_string(), Style::new().fg(Color::Rgb(180, 190, 200)))])
        })).collect();
    let footer = if is_zh { "  ? 或 Esc 关闭此窗口" } else { "  Press ? or Esc to close" };
    let blines: Vec<Line> = blines.into_iter().chain(std::iter::once(Line::from(Span::styled(footer, Style::new().fg(Color::Gray))))).collect();
    frame.render_widget(Paragraph::new(blines), inner);
}

pub fn render_ask(frame: &mut Frame, app: &App) {
    let ask = match &app.ask { Some(a) => a, None => return };
    let area = frame.area(); let l = app.setup.lang;
    let popup = centered_rect(55, (ask.options.len() + 6).min(18).max(8) as u16, area);
    frame.render_widget(Clear, popup);
    let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::new().fg(ACCENT)).title(l.t_ask_title()).style(Style::new().bg(Color::Rgb(18, 22, 26)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut rlines: Vec<Line> = Vec::new();
    rlines.push(Line::from(Span::styled(&ask.question, Style::new().fg(Color::White).bold())));
    rlines.push(Line::from(""));
    for (i, opt) in ask.options.iter().enumerate() {
        let selected = i == ask.selected;
        let mark = if selected { "●" } else { "○" };
        let style = if selected { Style::new().fg(ACCENT).bold() } else { Style::new().fg(DIM) };
        rlines.push(Line::from(vec![Span::raw(format!("  {mark}  ")), Span::styled(opt.clone(), style)]));
    }
    let other_label = if ask.selected == ask.options.len() {
        if ask.custom_input.is_empty() { l.t_ask_other_placeholder() } else { &ask.custom_input }
    } else { l.t_ask_other() };
    rlines.push(Line::from(vec![
        Span::raw(format!("  {}  ", if ask.selected == ask.options.len() { "●" } else { "○" })),
        Span::styled(other_label.to_string(), if ask.selected == ask.options.len() { Style::new().fg(ACCENT).bold() } else { Style::new().fg(DIM) }),
    ]));
    rlines.push(Line::from(""));
    rlines.push(Line::from(Span::styled(l.t_ask_help(), Style::new().fg(DIM))));
    frame.render_widget(Paragraph::new(rlines), inner);
}

pub fn render_menu(frame: &mut Frame, menu: &crate::app::MenuState) {
    use crate::app::MenuItemKind;
    let area = frame.area();
    frame.render_widget(Paragraph::new("").style(Style::new().bg(BG)), area);
    let [_, list_area, status_area, footer_area] = Layout::vertical([Constraint::Length(3), Constraint::Fill(1), Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let title_block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::new().fg(ACCENT)).title(menu.lang.t_menu_title()).style(Style::new().bg(Color::Rgb(18, 22, 26)));
    frame.render_widget(title_block, Rect::new(area.x, area.y, area.width, 3));
    let nav = menu.lang.t_menu_nav(); let toggle_hint = menu.lang.t_menu_toggle_edit(); let back_hint = menu.lang.t_menu_back();
    let title_lines = vec![Line::from(vec![Span::raw("  "), Span::styled("Menu", Style::new().fg(ACCENT).bold()), Span::raw("  |  "), Span::styled(nav, Style::new().fg(DIM)), Span::raw("  "), Span::styled(toggle_hint, Style::new().fg(DIM)), Span::raw("  "), Span::styled(back_hint, Style::new().fg(DIM))])];
    frame.render_widget(Paragraph::new(title_lines), Rect::new(area.x + 2, area.y + 1, area.width - 4, 1));
    let visible = list_area.height.saturating_sub(2) as usize;
    let max_scroll = menu.items.len().saturating_sub(visible);
    let scroll = if menu.selected < max_scroll { menu.selected } else { max_scroll };
    let mut rlines: Vec<Line> = Vec::new();
    let show_from = scroll; let show_to = (scroll + visible).min(menu.items.len());
    for idx in show_from..show_to {
        let item = &menu.items[idx]; let selected = idx == menu.selected;
        let line = match item.kind {
            MenuItemKind::Section => Line::from(vec![Span::raw("  "), Span::styled(&item.label, Style::new().fg(Color::Rgb(100, 200, 255)).bold())]),
            MenuItemKind::Toggle => {
                let on = item.value == "ON" || item.value == "en";
                let val_style = if on { Style::new().fg(Color::Rgb(100, 220, 100)).bold() } else { Style::new().fg(Color::Rgb(220, 100, 100)).bold() };
                let sel_mark = if selected && !menu.editing { "● " } else { "  " };
                let label_style = if selected && !menu.editing { Style::new().fg(Color::Yellow).bold() } else { Style::new().fg(Color::White) };
                Line::from(vec![Span::raw(sel_mark), Span::styled(format!("{:<20}", item.label), label_style), Span::raw("  "), Span::styled(&item.value, val_style)])
            }
            MenuItemKind::Value => {
                let sel_mark = if selected && !menu.editing { "● " } else { "  " };
                let label_style = if selected && !menu.editing { Style::new().fg(Color::Yellow).bold() } else { Style::new().fg(Color::White) };
                let display = if selected && menu.editing { if menu.edit_buf.is_empty() { item.value.clone() } else { menu.edit_buf.clone() } } else { item.value.clone() };
                Line::from(vec![Span::raw(sel_mark), Span::styled(format!("{:<20}", item.label), label_style), Span::raw("  "), Span::styled(display, Style::new().fg(Color::Rgb(180, 200, 220)))])
            }
            MenuItemKind::Action => {
                let is_active = item.label.starts_with("▶");
                let sel_mark = if selected { "● " } else { "  " };
                let label_style = if selected { Style::new().fg(Color::Yellow).bold() } else if is_active { Style::new().fg(Color::Rgb(100, 220, 100)).bold() } else { Style::new().fg(Color::White) };
                Line::from(vec![Span::raw(sel_mark), Span::styled(format!("{:<20}", item.label), label_style), Span::raw("  "), Span::styled(&item.value, Style::new().fg(Color::Rgb(160, 170, 180)))])
            }
        };
        rlines.push(line);
    }
    frame.render_widget(Paragraph::new(rlines), list_area);
    if !menu.status.is_empty() {
        frame.render_widget(Span::styled(format!("  {}", menu.status), Style::new().fg(if menu.status.contains("saved") || menu.status.contains("已保存") { GREEN } else { Color::Red })), Rect { x: area.x + 2, y: status_area.y, width: area.width - 4, height: 1 });
    }
    let footer = if menu.editing { if menu.lang.as_str() == "zh" { "  Enter 确认  Esc 取消  Backspace 删除" } else { "  Enter confirm  Esc cancel  Backspace delete" } }
    else { if menu.lang.as_str() == "zh" { "  ↑↓ 导航  Enter 切换/编辑  Esc 保存并返回" } else { "  ↑↓ navigate  Enter toggle/edit  Esc save & back" } };
    frame.render_widget(Line::from(Span::styled(footer, Style::new().fg(DIM))), footer_area);
    if menu.editing {
        let item = match menu.items.get(menu.selected) { Some(i) => i, None => return };
        let display_value = if menu.edit_buf.is_empty() { &item.value } else { &menu.edit_buf };
        let val_len = cjk_width(display_value);
        // Compute prefix display width: "  " (sel_mark) + formatted_label + "  " (separator)
        let label_fmt = format!("{:<20}", item.label);
        let prefix_width = 2u16 + cjk_width(&label_fmt) + 2u16;
        let cursor_x = list_area.x + prefix_width + val_len.min(30) as u16;
        let row = (menu.selected.saturating_sub(scroll) + 1) as u16;
        let cursor_y = list_area.y + row;
        frame.set_cursor_position((cursor_x.min(area.width.saturating_sub(1)), cursor_y.min(area.height.saturating_sub(1))));
    }
}
