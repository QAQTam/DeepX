//! Setup wizard widget.
//!
//! Guided configuration flow for first-time setup.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use crate::theme::Theme;

/// Setup wizard step.
pub enum SetupStep {
    Welcome,
    ApiKey,
    Model,
    ContextLimit,
    Done,
}

/// Guided setup wizard.
pub struct SetupWizard<'a> {
    pub step: SetupStep,
    pub input: &'a str,
    pub status: &'a str,
    pub error: &'a str,
    pub theme: &'a Theme,
}

impl Widget for SetupWizard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let block = Block::new()
            .title(" DeepX Setup ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.theme.popup_border)
            .style(self.theme.popup_bg);

        let inner = block.inner(area);
        block.render(area, buf);

        let (step_title, prompt) = match self.step {
            SetupStep::Welcome => ("Welcome", "Press Enter to begin setup."),
            SetupStep::ApiKey => ("API Key", "Enter your API key:"),
            SetupStep::Model => ("Model", "Enter model name (or press Enter for default):"),
            SetupStep::ContextLimit => ("Context Limit", "Enter context token limit (default: 32768):"),
            SetupStep::Done => ("Done", "Setup complete. Press Enter to continue."),
        };

        let mut text = Vec::new();
        text.push(Line::from(Span::styled(
            format!("Step: {}", step_title),
            self.theme.highlight,
        )));
        text.push(Line::from(""));
        text.push(Line::from(prompt));

        if !self.input.is_empty() {
            text.push(Line::from(Span::styled(
                format!("> {}", self.input),
                self.theme.input_text,
            )));
        }

        if !self.status.is_empty() {
            text.push(Line::from(Span::styled(
                self.status,
                self.theme.assistant_message,
            )));
        }

        if !self.error.is_empty() {
            text.push(Line::from(Span::styled(
                self.error,
                self.theme.error_message,
            )));
        }

        Paragraph::new(text)
            .style(self.theme.help_fg)
            .render(inner, buf);
    }
}
