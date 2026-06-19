//! deepx-gate: LLM API gateway — HTTP streaming + message format conversion.
//!
//! Currently supports OpenAI-compatible protocol.

mod types;
mod openai;
pub mod tool_parser;

pub use types::{ProviderConfig, ProviderKind, StreamEvent};
pub use openai::query_balance;

use deepx_types::{Message, ToolDef};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Send a chat completion request with SSE streaming.
///
/// `cancel` is an optional shared abort flag. When set to `true`, the
/// streaming read loop will return `Err("cancelled by user")` within
/// `SSE_READ_TIMEOUT` (200ms), aborting the HTTP response promptly
/// instead of waiting for the server to finish.
pub fn chat_stream(
    provider: &ProviderConfig,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDef>>,
    max_tokens: u32,
    effort: Option<String>,
    user_id: Option<String>,
    cancel: Option<&Arc<AtomicBool>>,
    on_event: &mut dyn FnMut(StreamEvent),
) -> anyhow::Result<()> {
    openai::chat_stream_openai(
        provider, &provider.model, messages, tools,
        max_tokens, effort, user_id, cancel, on_event,
    )
}

/// Synchronous (non-streaming) chat for internal use (compact, etc.).
pub fn chat_sync(provider: &ProviderConfig, messages: Vec<Message>, max_tokens: u32) -> Result<String, String> {
    openai::chat_sync_openai(provider, &provider.model, messages, max_tokens)
}
