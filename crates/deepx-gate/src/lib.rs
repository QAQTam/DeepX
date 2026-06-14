//! deepx-gate: LLM API gateway — HTTP streaming + message format conversion.
//!
//! Currently supports OpenAI-compatible protocol.

mod types;
mod openai;
pub mod tool_parser;

pub use types::{ProviderConfig, ProviderKind, StreamEvent};
pub use openai::query_balance;

use deepx_types::{Message, ToolDef};

/// Send a chat completion request with SSE streaming.
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

/// Synchronous (non-streaming) chat for internal use (compact, etc.).
pub fn chat_sync(provider: &ProviderConfig, messages: Vec<Message>, max_tokens: u32) -> Result<String, String> {
    openai::chat_sync_openai(provider, &provider.model, messages, max_tokens)
}
