//! deepx-gate: LLM API gateway — HTTP streaming + message format conversion.
//!
//! Currently supports OpenAI-compatible protocol.

mod types;
mod openai;

pub use types::{ProviderConfig, ProviderKind, StreamEvent};
pub use openai::query_balance;

use dsx_types::{Message, ToolDef};

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
