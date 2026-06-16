//! Chat view: message list + input bar + tool cards.

use crate::app::{AppState, Message, Role};
use egui::{Color32, Label, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::cell::RefCell;

thread_local! {
    static MD_CACHE: RefCell<CommonMarkCache> = RefCell::new(CommonMarkCache::default());
}

/// Main chat panel: InfoBar + scrollable message area + bottom input bar.
pub(crate) fn render_chat(ui: &mut egui::Ui, s: &mut AppState) {
    // ── InfoBar ──
    crate::infobar::render_infobar(ui, s);

    // ── Message list (scrollable, fills remaining space) ──
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let av = ui.available_size();
            // Fill available width but never exceed it
            ui.set_max_width(av.x - 8.0);
            ui.set_min_width((av.x - 16.0).min(300.0));
            if s.messages.is_empty() {
                ui.add_space(av.y * 0.35);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(if s.connected {
                            "发送消息..."
                        } else {
                            "点击左侧连接"
                        })
                        .color(Color32::GRAY),
                    );
                });
            }
            for msg in &s.messages {
                render_message(ui, msg);
            }
        });

    // ── Input bar ──
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.separator();
        ui.horizontal(|ui| {
            let btn_width = if s.streaming { 50.0 } else { 50.0 };
            let input_w = (ui.available_width() - btn_width - 8.0).max(60.0);
            let resp = ui.add_sized(
                [input_w, 24.0],
                egui::TextEdit::singleline(&mut s.input)
                    .hint_text(if s.connected { "输入..." } else { "未连接" })
                    .desired_width(f32::INFINITY),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            if s.streaming {
                if ui
                    .add_sized(
                        [btn_width, 24.0],
                        egui::Button::new(
                            RichText::new("⏹").size(14.0).color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0xC4, 0x55, 0x3D)),
                    )
                    .clicked()
                {
                    s.send_cancel();
                }
            } else if (enter
                || ui
                    .add_sized([btn_width, 24.0], egui::Button::new("发送"))
                    .clicked())
                && s.connected
            {
                let text = s.input.trim().to_string();
                if !text.is_empty() {
                    s.messages.push_back(Message {
                        tool_id: None,
                        tool_result: None,
                        tool_ok: None,
                        exec_draft: None,
                        finalized: true,
                        role: Role::User,
                        text: text.clone(),
                    });
                    s.send(&text);
                    s.input.clear();
                }
                resp.request_focus();
            }
        });
    });
}

// ── Message rendering ──

pub(crate) fn render_message(ui: &mut egui::Ui, msg: &Message) {
    let text = Color32::from_rgb(0x2C, 0x24, 0x16);
    let max_w = ui.available_width();
    match msg.role {
        Role::User => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.set_max_width(max_w);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(0xD4, 0x78, 0x3C, 30))
                    .stroke(egui::Stroke::new(
                        0.5,
                        Color32::from_rgba_unmultiplied(0xD4, 0x78, 0x3C, 64),
                    ))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_min_width(60.0);
                        ui.add(
                            Label::new(RichText::new(&msg.text).color(text))
                                .selectable(true),
                        );
                    });
            });
        }
            Role::Assistant => {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    ui.set_max_width(max_w);
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(7))
                        .stroke(egui::Stroke::new(
                            0.5,
                            Color32::from_rgba_premultiplied(0, 0, 0, 20),
                        ))
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(60.0);
                            if msg.finalized {
                                // Render markdown via egui_commonmark
                                MD_CACHE.with(|c| {
                                    let mut cache = c.borrow_mut();
                                    CommonMarkViewer::new().show(ui, &mut cache, &msg.text);
                                });
                            } else {
                                // Segmented by \n: fixed lines cache-hit, only last line costs
                                let mut lines = msg.text.split('\n').peekable();
                                while let Some(line) = lines.next() {
                                    if lines.peek().is_some() {
                                        ui.colored_label(text, line);
                                    } else {
                                        ui.horizontal(|ui| {
                                            ui.colored_label(text, line);
                                            ui.colored_label(text, "▌");
                                        });
                                    }
                                }
                            }
                        });
                });
            }
        Role::ToolCall => render_tool_card(ui, msg),
        Role::Thinking => render_thinking(ui, msg),
        _ => {
            ui.colored_label(Color32::from_rgb(0x9B, 0x8D, 0x7A), &msg.text);
        }
    }
    ui.add_space(4.0);
}

// ── Thinking bubble ──

fn render_thinking(ui: &mut egui::Ui, msg: &Message) {
    let accent = Color32::from_rgb(0x9B, 0x8D, 0x7A);
    let text = Color32::from_rgb(0x2C, 0x24, 0x16);
    let bg = Color32::from_rgba_premultiplied(0x9B, 0x8D, 0x7A, 15);
    let border = Color32::from_rgba_premultiplied(0x9B, 0x8D, 0x7A, 40);
    let max_w = ui.available_width();

    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
        ui.set_max_width(max_w);
        egui::Frame::new()
            .fill(bg)
            .stroke(egui::Stroke::new(0.5, border))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        accent,
                        egui::RichText::new("💭").size(11.0),
                    );
                    if msg.finalized {
                        // Render markdown via egui_commonmark
                        MD_CACHE.with(|c| {
                            let mut cache = c.borrow_mut();
                            CommonMarkViewer::new().show(ui, &mut cache, &msg.text);
                        });
                    } else {
                        // Streaming thinking: segmented by \n for cache efficiency
                        let mut lines = msg.text.split('\n').peekable();
                        while let Some(line) = lines.next() {
                            if lines.peek().is_some() {
                                ui.colored_label(text, egui::RichText::new(line).size(12.0).italics());
                            } else {
                                ui.horizontal(|ui| {
                                    ui.colored_label(text, egui::RichText::new(line).size(12.0).italics());
                                    ui.colored_label(text, "▌");
                                });
                            }
                        }
                    }
                });
            });
    });
    ui.add_space(4.0);
}

// ── Tool card ──

pub(crate) fn render_tool_card(ui: &mut egui::Ui, msg: &Message) {
    let accent = Color32::from_rgb(0xD4, 0x78, 0x3C);
    let text = Color32::from_rgb(0x2C, 0x24, 0x16);
    let tm = Color32::from_rgb(0x9B, 0x8D, 0x7A);
    let bg = Color32::from_rgb(0xF3, 0xEF, 0xE9);

    let id = egui::Id::new(msg.tool_id.as_deref().unwrap_or("tool"));
    let mut open = ui
        .data_mut(|d| d.get_temp::<bool>(id).unwrap_or(false));

    let status = match msg.tool_ok {
        Some(true) => " ✓",
        Some(false) => " ✗",
        None => " ⏳",
    };

    let header_resp = egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(
            0.5,
            Color32::from_rgba_premultiplied(0, 0, 0, 25),
        ))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            let w = ui.available_width().min(400.0);
            ui.set_min_width(w);
            ui.horizontal(|ui| {
                ui.colored_label(accent, egui::RichText::new("🔧").size(12.0));
                ui.colored_label(
                    accent,
                    egui::RichText::new(&msg.text).size(12.0).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status_color = match msg.tool_ok {
                        Some(true) => Color32::from_rgb(0x5A, 0x8A, 0x4A),
                        Some(false) => Color32::from_rgb(0xC4, 0x55, 0x3D),
                        None => tm,
                    };
                    ui.colored_label(status_color, status);
                });
            });
        });

    if header_resp
        .response
        .interact(egui::Sense::click())
        .clicked()
    {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }

    if open {
        if let Some(ref result) = msg.tool_result {
            egui::Frame::new()
                .fill(Color32::from_rgb(0xEB, 0xE5, 0xDB))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    let w = ui.available_width().min(400.0);
                    ui.set_min_width(w);
                    ui.add(
                        Label::new(
                            egui::RichText::new(result).size(12.0).monospace().color(text),
                        )
                        .selectable(true),
                    );
                });
        } else {
            ui.colored_label(tm, "  等待结果...");
        }
    }
    ui.add_space(4.0);
}