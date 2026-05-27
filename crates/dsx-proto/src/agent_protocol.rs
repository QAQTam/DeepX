//! UI ↔ Agent frame definitions (channel-based, mpsc in-process).
//!
//! Pure Rust enums passed via `mpsc::Sender`/`Receiver`. Serde derives
//! are retained for JSON-LP headless mode over stdin/stdout.

use serde::{Deserialize, Serialize};

/// UI → Agent frames (mpsc channel / stdin pipe in headless mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Ui2Agent {
    #[serde(rename = "user_input")]
    UserInput { text: String },

    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        action: String,
        args: serde_json::Value,
    },

    #[serde(rename = "set_phase")]
    SetPhase { phase: String },

    #[serde(rename = "tool_confirm")]
    ToolConfirm { id: String, approved: bool },

    #[serde(rename = "cancel")]
    Cancel,

    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Agent → UI frames (mpsc channel / stdout pipe in headless mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Agent2Ui {
    /// Streaming content delta (one token or small chunk).
    #[serde(rename = "content_delta")]
    ContentDelta {
        delta: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
    },

    /// Streaming tool progress.
    #[serde(rename = "tool_progress")]
    ToolProgress {
        id: String,
        content: String,
        stream_type: String,
    },

    /// Full API response (non-streaming fallback or final).
    #[serde(rename = "api_response")]
    ApiResponse {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<dsx_types::ToolCall>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<dsx_types::UsageInfo>,
        /// Full context token count (system + tools + messages).
        #[serde(default)]
        context_tokens: u32,
    },

    #[serde(rename = "ask_user")]
    AskUser {
        id: String,
        question: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<Vec<String>>,
    },

    #[serde(rename = "phase_changed")]
    PhaseChanged { phase: String },

    #[serde(rename = "tool_state")]
    ToolState {
        explored: bool,
        declared_files: Vec<String>,
        read_files: Vec<String>,
        written_this_turn: Vec<String>,
    },

    /// End of a turn (agent ready for next input).
    #[serde(rename = "done")]
    Done,

    /// Error during processing.
    #[serde(rename = "error")]
    Error { message: String },

    /// Predicted KV cache hit rate (client-side estimate).
    #[serde(rename = "cache_prediction")]
    CachePrediction { hit_rate: f64 },

    /// Shutdown acknowledgement.
    #[serde(rename = "shutdown_ack")]
    ShutdownAck,

    /// Tool execution result.
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        name: String,
        content: String,
        success: bool,
    },

    /// Session restored from disk (resumed conversation).
    #[serde(rename = "session_restored")]
    SessionRestored {
        seed: String,
        message_count: u64,
        summary: String,
        tokens_used: u32,
        cache_hit_pct: f64,
    },
}
