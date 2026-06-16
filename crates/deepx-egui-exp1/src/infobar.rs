//! InfoBar: top bar showing model, session, context, cache, compact.

use crate::app::AppState;
use egui::{Color32, ProgressBar, RichText};

/// Color constants from the DeepX palette.
const ACCENT: Color32 = Color32::from_rgb(0xD4, 0x78, 0x3C);
const TEXT: Color32 = Color32::from_rgb(0x2C, 0x24, 0x16);
const MUTED: Color32 = Color32::from_rgb(0x9B, 0x8D, 0x7A);
const GREEN: Color32 = Color32::from_rgb(0x5A, 0x8A, 0x4A);
const RED: Color32 = Color32::from_rgb(0xC4, 0x55, 0x3D);

pub(crate) fn render_infobar(ui: &mut egui::Ui, s: &mut AppState) {
    // ── Error banner ──
    let error_msg = s.error.clone();
    if let Some(ref error) = error_msg {
        let mut close = false;
        egui::Frame::new()
            .fill(Color32::from_rgba_premultiplied(0xC4, 0x55, 0x3D, 30))
            .inner_margin(egui::Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(RED, RichText::new("⚠").size(12.0));
                    ui.colored_label(RED, RichText::new(error).size(12.0));
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .small_button(
                                    RichText::new("✕").size(10.0).color(MUTED),
                                )
                                .clicked()
                            {
                                close = true;
                            }
                        },
                    );
                });
            });
        if close {
            s.clear_error();
        }
    }

    // ── Main bar ──
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(4, 2))
        .show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                // Status dot
                let (dot_color, dot_tip) = if s.streaming {
                    (GREEN, "Streaming")
                } else if s.error.is_some() {
                    (RED, "Error")
                } else if s.connected {
                    (GREEN, "Connected")
                } else {
                    (MUTED, "Disconnected")
                };
                ui.colored_label(
                    dot_color,
                    RichText::new("●").size(10.0),
                )
                .on_hover_text(dot_tip);

                // Model
                ui.colored_label(MUTED, RichText::new("model:").size(11.0));
                ui.colored_label(
                    TEXT,
                    RichText::new(if s.model.is_empty() { "—" } else { &s.model })
                        .size(11.0),
                );
                ui.separator();

                // Session seed
                if let Some(ref seed) = s.active_seed {
                    ui.colored_label(
                        MUTED,
                        RichText::new("#").size(11.0),
                    );
                    ui.colored_label(
                        TEXT,
                        RichText::new(&seed[..8.min(seed.len())])
                            .size(11.0)
                            .monospace(),
                    );
                    ui.separator();
                }

                // Context tokens
                ui.colored_label(MUTED, RichText::new("ctx:").size(11.0));
                ui.colored_label(
                    TEXT,
                    RichText::new(format!(
                        "{} / {}",
                        fmt_tokens(s.context_tokens),
                        fmt_tokens(s.context_limit)
                    ))
                    .size(11.0)
                    .monospace(),
                );
                if s.context_limit > 0 {
                    let pct = s.context_tokens as f32 / s.context_limit as f32;
                    ui.add(
                        ProgressBar::new(pct)
                            .desired_width(36.0)
                            .desired_height(6.0)
                            .fill(if pct > 0.8 { RED } else { ACCENT }),
                    );
                }
                ui.separator();

                // Cache hit
                ui.colored_label(MUTED, RichText::new("cache:").size(11.0));
                let cache_pct = if s.context_tokens > 0 {
                    s.prompt_cache_hit as f32 / s.context_tokens as f32
                } else {
                    0.0
                };
                ui.colored_label(
                    if cache_pct > 0.3 { GREEN } else { MUTED },
                    RichText::new(format!("{:.0}%", cache_pct * 100.0))
                        .size(11.0)
                        .monospace(),
                );
                if s.context_tokens > 0 {
                    ui.add(
                        ProgressBar::new(cache_pct)
                            .desired_width(36.0)
                            .desired_height(6.0)
                            .fill(GREEN),
                    );
                }
                ui.separator();

                // Compact button / progress
                if s.is_compacting {
                    let pct = s.compact_pct as f32 / 100.0;
                    ui.add(
                        ProgressBar::new(pct)
                            .desired_width(40.0)
                            .desired_height(10.0)
                            .fill(ACCENT)
                            .text(RichText::new(format!("{}%", s.compact_pct)).size(10.0)),
                    );
                } else {
                    if ui
                        .add_sized(
                            [16.0, 16.0],
                            egui::Button::new(
                                RichText::new("⬆").size(12.0).color(MUTED),
                            )
                            .fill(Color32::TRANSPARENT)
                            .frame(false),
                        )
                        .on_hover_text("Compact history")
                        .clicked()
                    {
                        s.send_compact();
                    }
                }

                // Compact toast
                if let Some(ref msg) = s.compact_result {
                    ui.colored_label(
                        GREEN,
                        RichText::new(msg).size(11.0),
                    );
                }
            });
        });
    ui.separator();
}

/// Format token count: 12345 → "12.3K", 1234567 → "1.2M"
fn fmt_tokens(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
