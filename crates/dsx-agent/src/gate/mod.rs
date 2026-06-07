//! Gate module — HTTP provider client.
//!
//! Handles LLM API communication directly across multiple providers.
//! Currently only OpenAI-compatible protocol is supported.

mod types;
mod openai;
pub mod registry;

pub use types::{ProviderConfig, ProviderKind, StreamEvent};
pub use openai::query_balance;

use dsx_types::{Message, ToolDef};

pub fn chat_stream(
    provider: &ProviderConfig,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDef>>,
    max_tokens: u32,
    effort: Option<String>,
    user_id: Option<String>,
    on_event: &mut dyn FnMut(StreamEvent),
) -> anyhow::Result<()> {
    openai::chat_stream_openai(
        provider, &provider.model, messages, tools,
        max_tokens, effort, user_id, on_event,
    )
}
