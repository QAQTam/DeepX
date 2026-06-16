//! StatusPanel: right-side panel showing Tasks, Activity, and Files.

use crate::app::AppState;
use deepx_proto::TaskInfo;
use egui::{Color32, RichText};

const TEXT: Color32 = Color32::from_rgb(0x2C, 0x24, 0x16);
const MUTED: Color32 = Color32::from_rgb(0x9B, 0x8D, 0x7A);
const GREEN: Color32 = Color32::from_rgb(0x5A, 0x8A, 0x4A);
const RED: Color32 = Color32::from_rgb(0xC4, 0x55, 0x3D);
const ACCENT: Color32 = Color32::from_rgb(0xD4, 0x78, 0x3C);

/// Tool name → single-char icon.
fn tool_icon(name: &str) -> &'static str {
    match name {
        "read_file" => "R",
        "write_file" => "W",
        "edit_file" => "E",
        "edit_file_diff" => "E",
        "delete_file" => "D",
        "exec" => ">",
        "explore" => "S",
        "search" => "Z",
        "glob" => "G",
        "web_search" => "@",
        "web_fetch" => "@",
        "list_dir" => "L",
        "diff" => "=",
        "task_create" => "T",
        "task_update" => "T",
        "task_delete" => "T",
        "ask_user" => "?",
        _ => "*",
    }
}

/// Task status → icon.
fn task_status_icon(status: &str) -> &'static str {
    match status {
        "pending" => "○",
        "in_progress" => "●",
        "completed" => "✓",
        "cancelled" => "✗",
        _ => "?",
    }
}

/// Task status → color.
fn task_status_color(status: &str) -> Color32 {
    match status {
        "pending" => MUTED,
        "in_progress" => ACCENT,
        "completed" => GREEN,
        "cancelled" => RED,
        _ => MUTED,
    }
}

pub(crate) fn render_status_panel(ui: &mut egui::Ui, s: &AppState) {
    ui.add_space(4.0);

    // ── Tasks ──
    ui.horizontal(|ui| {
        ui.colored_label(TEXT, RichText::new("Tasks").size(13.0).strong());
        let completed = s.tasks.iter().filter(|t| t.status == "completed").count();
        if !s.tasks.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    MUTED,
                    RichText::new(format!("{}/{}", completed, s.tasks.len())).size(11.0),
                );
            });
        }
    });
    ui.add_space(2.0);

    if s.tasks.is_empty() {
        ui.colored_label(MUTED, RichText::new("No tasks").size(11.0));
    } else {
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for task in &s.tasks {
                    render_task_row(ui, task);
                }
            });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // ── Activity ──
    ui.horizontal(|ui| {
        ui.colored_label(TEXT, RichText::new("Activity").size(13.0).strong());
        if !s.activity_log.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    MUTED,
                    RichText::new(format!("{}", s.activity_log.len())).size(11.0),
                );
            });
        }
    });
    ui.add_space(2.0);

    if s.activity_log.is_empty() {
        ui.colored_label(MUTED, RichText::new("No activity").size(11.0));
    } else {
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for entry in s.activity_log.iter().rev().take(20) {
                    render_activity_row(ui, entry);
                }
            });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // ── Files ──
    ui.horizontal(|ui| {
        ui.colored_label(TEXT, RichText::new("Files").size(13.0).strong());
        if !s.recent_edits.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    MUTED,
                    RichText::new(format!("{}", s.recent_edits.len())).size(11.0),
                );
            });
        }
    });
    ui.add_space(2.0);

    if s.recent_edits.is_empty() {
        ui.colored_label(MUTED, RichText::new("No files").size(11.0));
    } else {
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for edit in &s.recent_edits {
                    render_file_row(ui, edit);
                }
            });
    }
}

fn render_task_row(ui: &mut egui::Ui, task: &TaskInfo) {
    let color = task_status_color(&task.status);
    ui.horizontal(|ui| {
        ui.colored_label(
            color,
            RichText::new(task_status_icon(&task.status)).size(11.0),
        );
        ui.colored_label(
            TEXT,
            RichText::new(format!("{}: {}", task.id, task.subject)).size(11.0),
        );
    });
}

fn render_activity_row(ui: &mut egui::Ui, entry: &crate::app::ActivityEntry) {
    let icon_color = if entry.success { GREEN } else { RED };
    ui.horizontal(|ui| {
        ui.colored_label(
            icon_color,
            RichText::new(tool_icon(&entry.tool_name)).size(11.0).monospace(),
        );
        ui.colored_label(
            MUTED,
            RichText::new(&entry.tool_name).size(11.0),
        );
        ui.colored_label(
            TEXT,
            RichText::new(&entry.summary).size(11.0),
        );
    });
}

fn render_file_row(ui: &mut egui::Ui, edit: &str) {
    let (tool, path) = edit.split_once(": ").unwrap_or((edit, ""));
    ui.horizontal(|ui| {
        ui.colored_label(
            ACCENT,
            RichText::new(tool_icon(tool)).size(11.0).monospace(),
        );
        ui.colored_label(
            MUTED,
            RichText::new(tool).size(11.0),
        );
        ui.colored_label(
            TEXT,
            RichText::new(path).size(11.0),
        );
    });
}
