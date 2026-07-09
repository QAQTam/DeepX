//! DeepX TUI theme system.
//!
//! Centralizes all color and style definitions, replacing the
//! hardcoded `Style::new()` calls scattered in `ui/mod.rs`.

use ratatui::style::{Color, Style};

/// Complete TUI color theme.
#[derive(Clone, Debug)]
pub struct Theme {
    // Chat messages
    pub user_message: Style,
    pub assistant_message: Style,
    pub tool_message: Style,
    pub error_message: Style,
    pub system_message: Style,

    // Thinking / code
    pub thinking_text: Style,
    pub code_block_bg: Style,
    pub code_block_text: Style,

    // Input area
    pub input_cursor: Style,
    pub input_text: Style,
    pub input_placeholder: Style,
    pub input_border: Style,

    // Status bar
    pub status_bar_bg: Style,
    pub status_bar_fg: Style,

    // Popups
    pub help_bg: Style,
    pub help_fg: Style,
    pub popup_bg: Style,
    pub popup_border: Style,

    // Session selector
    pub session_item: Style,
    pub session_selected: Style,

    // Debug panel
    pub debug_bg: Style,
    pub debug_fg: Style,

    // General
    pub scrollbar: Style,
    pub divider: Style,
    pub normal_text: Style,
    pub highlight: Style,
    pub title_bar: Style,
}

impl Theme {
    /// Dark theme (default).
    pub fn dark() -> Self {
        Self {
            user_message: Style::new().fg(Color::Cyan),
            assistant_message: Style::new().fg(Color::White),
            tool_message: Style::new().fg(Color::Gray),
            error_message: Style::new().fg(Color::Red),
            system_message: Style::new().fg(Color::Yellow),

            thinking_text: Style::new().fg(Color::DarkGray),
            code_block_bg: Style::new().bg(Color::Rgb(30, 30, 30)),
            code_block_text: Style::new().fg(Color::Rgb(200, 200, 200)),

            input_cursor: Style::new()
                .fg(Color::White)
                .bg(Color::Rgb(60, 60, 60)),
            input_text: Style::new().fg(Color::White),
            input_placeholder: Style::new().fg(Color::DarkGray),
            input_border: Style::new().fg(Color::Rgb(80, 80, 80)),

            status_bar_bg: Style::new().bg(Color::Rgb(40, 40, 40)),
            status_bar_fg: Style::new().fg(Color::White),

            help_bg: Style::new().bg(Color::Rgb(20, 20, 40)),
            help_fg: Style::new().fg(Color::White),
            popup_bg: Style::new().bg(Color::Rgb(30, 30, 30)),
            popup_border: Style::new().fg(Color::Rgb(100, 100, 100)),

            session_item: Style::new().fg(Color::White),
            session_selected: Style::new().fg(Color::Black).bg(Color::Cyan),

            debug_bg: Style::new().bg(Color::Rgb(10, 10, 20)),
            debug_fg: Style::new().fg(Color::Rgb(180, 180, 200)),

            scrollbar: Style::new().fg(Color::Rgb(60, 60, 60)),
            divider: Style::new().fg(Color::Rgb(50, 50, 50)),
            normal_text: Style::new().fg(Color::White),
            highlight: Style::new().fg(Color::Yellow),
            title_bar: Style::new()
                .fg(Color::Black)
                .bg(Color::Rgb(100, 100, 200)),
        }
    }

    /// Light theme.
    pub fn light() -> Self {
        Self {
            user_message: Style::new().fg(Color::Blue),
            assistant_message: Style::new().fg(Color::Black),
            tool_message: Style::new().fg(Color::DarkGray),
            error_message: Style::new().fg(Color::Red),
            system_message: Style::new().fg(Color::Rgb(180, 130, 0)),

            thinking_text: Style::new().fg(Color::Gray),
            code_block_bg: Style::new().bg(Color::Rgb(245, 245, 245)),
            code_block_text: Style::new().fg(Color::Rgb(40, 40, 40)),

            input_cursor: Style::new()
                .fg(Color::Black)
                .bg(Color::Rgb(200, 200, 200)),
            input_text: Style::new().fg(Color::Black),
            input_placeholder: Style::new().fg(Color::Gray),
            input_border: Style::new().fg(Color::Rgb(180, 180, 180)),

            status_bar_bg: Style::new().bg(Color::Rgb(230, 230, 230)),
            status_bar_fg: Style::new().fg(Color::Black),

            help_bg: Style::new().bg(Color::Rgb(240, 240, 255)),
            help_fg: Style::new().fg(Color::Black),
            popup_bg: Style::new().bg(Color::Rgb(245, 245, 245)),
            popup_border: Style::new().fg(Color::Rgb(150, 150, 150)),

            session_item: Style::new().fg(Color::Black),
            session_selected: Style::new().fg(Color::White).bg(Color::Blue),

            debug_bg: Style::new().bg(Color::Rgb(245, 245, 255)),
            debug_fg: Style::new().fg(Color::Rgb(40, 40, 60)),

            scrollbar: Style::new().fg(Color::Rgb(200, 200, 200)),
            divider: Style::new().fg(Color::Rgb(210, 210, 210)),
            normal_text: Style::new().fg(Color::Black),
            highlight: Style::new().fg(Color::Rgb(180, 130, 0)),
            title_bar: Style::new()
                .fg(Color::White)
                .bg(Color::Rgb(60, 60, 180)),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
