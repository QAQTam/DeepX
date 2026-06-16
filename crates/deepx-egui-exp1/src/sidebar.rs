//! Sidebar: navigation, session list, connect/disconnect.

use crate::app::{AppState, View};
use egui::Color32;

pub(crate) fn render_sidebar(ui: &mut egui::Ui, s: &mut AppState) {
    let a = Color32::from_rgb(0xD4, 0x78, 0x3C);
    let t = Color32::from_rgb(0x2C, 0x24, 0x16);
    let tm = Color32::from_rgb(0x9B, 0x8D, 0x7A);

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.colored_label(a, egui::RichText::new(">").size(20.0).strong());
        ui.colored_label(t, egui::RichText::new("DeepX").size(16.0).strong());
    });
    ui.add_space(16.0);

    // ── Nav ──
    if ui
        .selectable_label(s.view == View::Chat, "💬 聊天")
        .clicked()
    {
        s.view = View::Chat;
    }
    if ui
        .selectable_label(s.view == View::Settings, "⚙ 设置")
        .clicked()
    {
        s.view = View::Settings;
    }
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // ── Sessions ──
    ui.horizontal(|ui| {
        ui.colored_label(tm, egui::RichText::new("会话").size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("+ 新建").clicked() {
                s.view = View::Chat;
                s.new_session();
            }
        });
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        let sessions = s.session_list();
        for se in &sessions {
            let sel = s.active_seed.as_deref() == Some(&se.seed);
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(sel, egui::RichText::new(&se.summary).size(13.0))
                    .clicked()
                {
                    s.view = View::Chat;
                    s.resume_session(&se.seed);
                }
                if ui.small_button("✕").clicked() {
                    s.delete_session(&se.seed);
                }
            });
        }
        if sessions.is_empty() {
            ui.colored_label(tm, egui::RichText::new("（空）").size(12.0));
        }
    });
}
