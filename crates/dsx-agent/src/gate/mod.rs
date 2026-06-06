//! Gate module — HTTP provider client.
//!
//! Handles LLM API communication directly across multiple providers.
//! Routes to the correct protocol implementation based on ProviderKind.

mod types;
mod openai;
mod anthropic;
pub mod registry;

pub use types::{ProviderConfig, ProviderKind, StreamEvent};
pub use openai::query_balance;

use dsx_types::{Message, ToolDef};

/// Route to the correct provider implementation.
pub fn chat_stream(
    provider: &ProviderConfig,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDef>>,
    max_tokens: u32,
    effort: Option<String>,
    user_id: Option<String>,
    on_event: &mut dyn FnMut(StreamEvent),
) -> anyhow::Result<()> {
    match provider.kind {
        ProviderKind::OpenAi => {
            openai::chat_stream_openai(
                provider, &provider.model, messages, tools,
                max_tokens, effort, user_id, on_event,
            )
        }
        ProviderKind::Anthropic => {
            anthropic::chat_stream_anthropic(
                provider, system, messages, tools,
                max_tokens, effort, on_event,
            )
        }
    }
}
