use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
};
use crate::app::App;
use crate::app::ToolStatus;

/// Build flat Vec<Line> from App messages. Extracted from render_chat to avoid
/// lifetime conflicts with the text_lines cache (&mut App vs Vec<Line<'static>>).
pub fn build_chat_lines(app: &App, body: Rect) -> Vec<Line<'static>> {
    use crate::app::ChatRole;
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(app.messages.len() * 4);
    let mut prev_role: Option<ChatRole> = None;
    let dim_color = Color::Rgb(140, 140, 150);
    let dim_border = Color::Rgb(60, 70, 80);

    for msg in &app.messages {
        if let Some(pr) = prev_role {
            if pr != msg.role && pr != ChatRole::Divider && msg.role != ChatRole::Divider
                && pr != ChatRole::Status && msg.role != ChatRole::Status
            {
                let div_len = body.width.saturating_sub(2).min(60) as usize;
                lines.push(Line::from(Span::styled(
                    format!(" {}", "─".repeat(div_len)),
                    Style::new().fg(dim_border),
                )));
            }
        }
        if msg.role != ChatRole::Divider && msg.role != ChatRole::Status {
            prev_role = Some(msg.role);
        }

        match msg.role {
            ChatRole::Divider => {
                let div_len = body.width.saturating_sub(2).min(60) as usize;
                lines.push(Line::from(Span::styled(
                    format!(" {}", "─".repeat(div_len)),
                    Style::new().fg(dim_border),
                )));
            }
            ChatRole::Status => {
                lines.push(Line::from(Span::styled(&msg.content, Style::new().fg(Color::Red))));
            }
            ChatRole::User => {
                let bg = Color::Rgb(55, 55, 65);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}", &msg.content), Style::new().fg(Color::White).bg(bg)),
                ]).alignment(Alignment::Left));
            }
            ChatRole::Thinking => {
                let dim = dim_color;
                if !app.show_thinking {
                    let char_count = msg.content.chars().count();
                    let summary: String = msg.content
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .chars().take(80).collect();
                    let hint = if app.setup.lang.as_str() == "zh" {
                        format!("  💭 {}…  ({} 字符, F6 展开)", summary, char_count)
                    } else {
                        format!("  💭 {}…  ({} chars, F6 expand)", summary, char_count)
                    };
                    lines.push(Line::from(Span::styled(hint, Style::new().fg(Color::Rgb(100, 110, 120)).italic())));
                } else if msg.lines.is_empty() {
                    lines.push(Line::from(vec![
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
                        lines.push(Line::from(spans));
                    }
                }
            }
            ChatRole::Assistant => {
                let msg_age = app.last_msg_time.elapsed().as_secs_f64();
                let is_fresh = msg_age < 1.5 && msg.lines.iter().any(|l|
                    l.spans.iter().any(|s| !s.content.trim().is_empty())
                );
                if msg.lines.is_empty() {
                    let pulse_marker = if is_fresh {
                        Span::styled("▎", Style::new().fg(Color::Rgb(100, 200, 255)).bold())
                    } else {
                        Span::raw("  ")
                    };
                    lines.push(Line::from(vec![
                        pulse_marker,
                        Span::styled(format!("{}", &msg.content), Style::new().fg(Color::White)),
                    ]));
                } else {
                    let first_char = msg.lines[0].spans.first()
                        .and_then(|s| s.content.chars().next());
                    let is_table = first_char.map_or(false, |c| {
                        c == '│' || c == '├' || c == '└' || c == '┌' || c == '┐' || c == '┘'
                    });
                    if is_table {
                        for line in &msg.lines {
                            lines.push(line.clone());
                        }
                    } else {
                        for (li, line) in msg.lines.iter().enumerate() {
                            let mut spans: Vec<Span> = line.spans.iter().map(|s| s.clone()).collect();
                            if spans.first().map_or(true, |s| !s.content.starts_with("  ")) {
                                if li == 0 && is_fresh {
                                    spans.insert(0, Span::styled("▎", Style::new().fg(Color::Rgb(100, 200, 255)).bold()));
                                } else {
                                    spans.insert(0, Span::raw("  "));
                                }
                            }
                            lines.push(Line::from(spans));
                        }
                    }
                }
            }
            ChatRole::Tool => {
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
                    lines.push(Line::from(vec![prefix, Span::raw(&msg.content)]));
                } else {
                    for (i, line) in msg.lines.iter().enumerate() {
                        let mut spans: Vec<Span> = line.spans.iter().map(|s| s.clone()).collect();
                        if i == 0 {
                            spans.insert(0, prefix.clone());
                        }
                        lines.push(Line::from(spans));
                    }
                }
                if msg.tool_status == ToolStatus::Pending {
                    if let Some(start) = app.tool_batch_start {
                        let elapsed = start.elapsed();
                        let elapsed_str = format!(" {:.1}s", elapsed.as_secs_f64());
                        if let Some(last_line) = lines.last_mut() {
                            last_line.spans.push(Span::styled(
                                elapsed_str,
                                Style::new().fg(Color::Rgb(180, 160, 100)).bold(),
                            ));
                        }
                    }
                }
                if msg.tool_status == ToolStatus::Pending
                    && app.tool_batch_total > 1
                    && app.tool_batch_done < app.tool_batch_total
                {
                    let pending_after: usize = app.messages.iter()
                        .skip_while(|m| !std::ptr::eq(*m, msg))
                        .filter(|m| m.role == ChatRole::Tool && m.tool_status == ToolStatus::Pending)
                        .count();
                    if pending_after <= 1 {
                        let done = app.tool_batch_done as usize;
                        let total = app.tool_batch_total as usize;
                        let bar_w = 30usize;
                        let filled = if total > 0 { bar_w * done / total } else { 0 };
                        let gauge_str = format!(
                            "  ╰─[{}{}] {}/{}",
                            "█".repeat(filled),
                            "░".repeat(bar_w - filled),
                            done,
                            total,
                        );
                        lines.push(Line::from(Span::styled(
                            gauge_str,
                            Style::new().fg(Color::Rgb(120, 140, 160)),
                        )));
                    }
                }
            }
        }
    }
    // Safety: all spans contain owned data (Strings), so transmuting
    // the inferred local lifetime to 'static is sound.
    unsafe { std::mem::transmute(lines) }
}
