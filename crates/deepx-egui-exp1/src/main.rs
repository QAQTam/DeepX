//! deepx-egui-exp1 — DeepX chat prototype with egui.
//!
//! Multi-module architecture:
//! - `app`    : AppState, Message types, event routing
//! - `agent`  : Agent subprocess + IPC channels
//! - `chat`   : Message list, input bar, tool cards
//! - `sidebar`: Navigation, session list, connect
//! - `settings`: Settings view placeholder
//! - `theme`  : Visual theme + CJK font

mod agent;
mod app;
mod chat;
mod infobar;
mod settings;
mod sidebar;
mod status;
mod theme;

#[cfg(test)]
mod bench;

use app::{AppState, View};

fn main() -> eframe::Result<()> {
    deepx_session::SessionManager::init(deepx_types::platform::data_dir());

    eframe::run_ui_native(
        "deepx-egui",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([860.0, 600.0])
                .with_title("DeepX — egui prototype"),
            ..Default::default()
        },
        |ui, _frame| {
            // One-time init
            if !ui
                .data(|d| d.get_temp::<bool>(egui::Id::new("init")).unwrap_or(false))
            {
                theme::load_cjk_font(ui);
                theme::apply_theme(ui);
                // Show URLs on hover for markdown links
                ui.style_mut().url_in_tooltip = true;
                ui.data_mut(|d| d.insert_temp(egui::Id::new("init"), true));
            }

            // Restore state
            let mut s = ui
                .data_mut(|d| d.get_temp::<AppState>(egui::Id::NULL).unwrap_or_default());

            // Auto-connect on first frame
            if !s.connected {
                s.connect();
            }

            // Drain agent events
            s.poll_agent();

            // Layout: sidebar + main
            egui::Panel::left("sb")
                .default_size(200.0)
                .min_size(40.0)
                .max_size(300.0)
                .show(ui, |ui| sidebar::render_sidebar(ui, &mut s));

            // Right status panel (collapsible)
            let mut status_open = ui
                .data(|d| d.get_temp::<bool>(egui::Id::new("status_open")).unwrap_or(true));
            egui::Panel::right("status")
                .default_size(220.0)
                .min_size(40.0)
                .resizable(true)
                .show_collapsible(ui, &mut status_open, |ui| {
                    status::render_status_panel(ui, &s);
                });
            ui.data_mut(|d| d.insert_temp(egui::Id::new("status_open"), status_open));

            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(egui::Color32::from_rgb(0xFA, 0xF8, 0xF5)))
                .show(ui, |ui| match s.view {
                    View::Chat => chat::render_chat(ui, &mut s),
                    View::Settings => settings::render_settings(ui, &mut s),
                });

            // Smart repaint: immediate if new events, periodic if streaming (for cursor blink)
            if s.dirty {
                ui.request_repaint();
            } else if s.streaming {
                ui.request_repaint_after(std::time::Duration::from_millis(120));
            }

            // Persist state
            ui.data_mut(|d| d.insert_temp(egui::Id::NULL, s));
        },
    )
}
