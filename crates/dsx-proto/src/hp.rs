//! Agent ↔ HP (Health Platform) frame definitions (TCP JSON-LP).
//!
//! The HP daemon is a security boundary: it holds API keys and proxies
//! LLM requests, plus monitors process liveness.

use serde::{Deserialize, Serialize};

use super::Redacted;

/// Agent → HP frames (TCP).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum AgentToHp {
    #[serde(rename = "register")]
    Register {
        kind: String,
        name: String,
        pid: u32,
    },

    #[serde(rename = "heartbeat")]
    Heartbeat { pid: u32 },

    #[serde(rename = "unregister")]
    Unregister { pid: u32 },

    #[serde(rename = "judge")]
    Judge,

    #[serde(rename = "query")]
    Query { pid: u32 },

    #[serde(rename = "list")]
    List,

    /// API chat request — forwarded to LLM provider.
    #[serde(rename = "api_chat")]
    ApiChat {
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        system: Option<String>,
        messages: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_tokens: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tools: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_id: Option<String>,
        /// API key from agent's runtime config (bypasses HP's OnceLock cache).
        /// Redacted in Debug output to prevent log leakage.
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<Redacted>,
    },
}

/// HP → Agent frames (TCP).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum HpToAgent {
    #[serde(rename = "error")]
    Error { message: String },

    /// Streaming content delta from LLM.
    #[serde(rename = "content_delta")]
    ContentDelta {
        delta: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
    },

    /// Streaming tool call progress.
    #[serde(rename = "tool_progress")]
    ToolProgress {
        #[serde(default)]
        id: String,
        content: String,
        #[serde(default = "default_stream_type")]
        stream_type: String,
    },

    /// Final API response from LLM provider.
    #[serde(rename = "api_response")]
    ApiResponse {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking_signature: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<dsx_types::UsageInfo>,
    },
}

fn default_stream_type() -> String {
    "progress".into()
}
