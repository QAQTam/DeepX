//! Codex-style main layout.
//!
//! ```
//! ┌──────────────────────────────┐
//! │  ChatWidgetV2 (messages)     │ flex=1
//! │                              │
//! ├──────────────────────────────┤
//! │  BottomPane                  │
//! │  ├ StatusLine                │
//! │  ├ TabBar (Chat | Files)     │
//! │  └ Composer                  │
//! └──────────────────────────────┘
//! ```

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::app::App;
use crate::render::renderable::{to_item, FlexRenderable, Renderable};
use crate::theme::Theme;
use crate::widgets::bottom_pane::{BottomPane, BottomTab};
use crate::widgets::chat_widget_v2::ChatWidgetV2;

pub fn build_main_layout<'a>(app: &'a App, theme: &'a Theme) -> FlexRenderable<'a> {
    let mut flex = FlexRenderable::new();

    // Chat area (flex=1)
    flex.push(1, to_item(ChatArea { app, theme }));

    // BottomPane (flex=0, fixed)
    flex.push(0, to_item(BottomPaneWrapper { app, theme }));

    flex
}

// ── Chat area wrapper ────────────────────────────────────────────────────

struct ChatArea<'a> {
    app: &'a App,
    theme: &'a Theme,
}

impl Renderable for ChatArea<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let chat = ChatWidgetV2 {
            messages: &self.app.messages,
            scroll_offset: self.app.scroll_offset,
            theme: self.theme,
            show_thinking: self.app.visibility.show_thinking,
        };
        chat.render(area, buf);
    }

    fn desired_height(&self, _w: u16) -> u16 {
        1 // flex will expand
    }
}

// ── BottomPane wrapper ───────────────────────────────────────────────────

struct BottomPaneWrapper<'a> {
    app: &'a App,
    theme: &'a Theme,
}

const BOTTOM_TABS: &[BottomTab] = &[BottomTab::Chat];

impl Renderable for BottomPaneWrapper<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let pane = BottomPane {
            active_tab: BottomTab::Chat,
            tabs: BOTTOM_TABS,
            status: if self.app.streaming { "Thinking..." } else { &self.app.status },
            is_streaming: self.app.streaming,
            composer_input: &self.app.input_state.input,
            composer_cursor: self.app.input_state.cursor,
            theme: self.theme,
        };
        pane.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let composer_h = self.app.input_state.input.lines().count().max(1) as u16;
        2 + composer_h // status + tabbar + composer
    }
}
