//! UI ↔ Agent frame definitions (channel-based, mpsc in-process).
//!
//! v4.1: Backend-owned message structure. Agent emits Typed messages
//! with pre-rendered content and explicit boundaries. Frontend only
//! routes by type — no state machine required.

use serde::{Deserialize, Serialize};

/// UI → Agent frames (unchanged from v4.0).
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

    #[serde(rename = "cancel")]
    Cancel,

    #[serde(rename = "shutdown")]
    Shutdown,

    #[serde(rename = "reload_config")]
    ReloadConfig,

    #[serde(rename = "debug_cmd")]
    DebugCommand { cmd: String },
}

// ── Shared types ──

/// Tool call definition used in both `AssistantMsg.tool_calls` and
/// `ToolCall` events. Carries a display-ready args summary and an
/// optional structured body for rich rendering (diff, exec command).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiToolDef {
    pub id: String,
    pub name: String,
    pub args_display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// File metadata snapshot carried by ToolResult when the tool
/// operated on a file. Frontend uses this for rich rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshotInfo {
    pub path: String,
    pub lines: u32,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// Document tracking entry — shows what files are in context and their state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocInfo {
    pub tag: String,
    pub path: String,
    pub turns_since_read: u32,
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub subject: String,
    pub description: String,
    pub status: String,
}

/// Agent → UI frames. Backend owns all message structure; frontend
/// receives pre-rendered, role-annotated blocks in guaranteed order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Agent2Ui {
    // ── Structured messages ──

    /// A complete assistant message (one API call result).
    /// Sent AFTER all streaming has finished for this message.
    /// Tool calls that belong to this message arrive as separate
    /// `ToolCall` events immediately after this message.
    #[serde(rename = "assistant_msg")]
    AssistantMsg {
        id: String,
        /// Full thinking content (accumulated from all reasoning deltas).
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
        /// Full text content (accumulated from all text deltas).
        text: String,
    },

    /// A user input message (sent at the start of each turn).
    #[serde(rename = "user_msg")]
    UserMsg {
        id: String,
        text: String,
    },

    // ── Tool execution ──

    /// A tool was invoked by the model. UI should render a tool card
    /// under the parent `msg_id`.
    #[serde(rename = "tool_call")]
    ToolCall {
        /// Parent assistant message ID
        msg_id: String,
        /// Tool def with display args and optional body
        #[serde(flatten)]
        tool: UiToolDef,
    },

    /// A tool execution completed. UI should update the tool card
    /// with the result.
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_id: String,
        output: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<FileSnapshotInfo>,
    },

    // ── Streaming / animation ──

    #[serde(rename = "stream_start")]
    StreamStart {
        msg_id: String,
        kind: StreamKind,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_names: Vec<String>,
    },

    #[serde(rename = "stream_delta")]
    StreamDelta {
        msg_id: String,
        delta: String,
    },

    #[serde(rename = "stream_end")]
    StreamEnd {
        msg_id: String,
        #[serde(default)]
        is_final: bool,
    },

    // ── Turn lifecycle ──

    /// End of the current turn. All tool calls have been resolved.
    /// Agent is ready for next user input.
    #[serde(rename = "turn_end")]
    TurnEnd {
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<dsx_types::UsageInfo>,
        context_tokens: u32,
        context_limit: u32,
        session_tokens: u64,
    },

    // ── System events ──

    #[serde(rename = "ask_user")]
    AskUser {
        id: String,
        question: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<Vec<String>>,
    },

    #[serde(rename = "error")]
    Error { message: String },

    /// Tool call notice — DSML/XML compatibility warnings.
    #[serde(rename = "tool_notice")]
    ToolNotice {
        message: String,
        /// "warn" or "error"
        level: String,
    },

    #[serde(rename = "balance")]
    Balance {
        is_available: bool,
        total_balance: String,
        currency: String,
    },

    #[serde(rename = "session_restored")]
    SessionRestored {
        seed: String,
        message_count: u64,
        summary: String,
        tokens_used: u32,
        cache_hit_pct: f64,
    },

    #[serde(rename = "debug_snapshot")]
    DebugSnapshot {
        hp_connected: bool,
        session_seed: String,
        context_tokens: u32,
        tool_calls_total: u32,
        tool_failures: u32,
        current_phase: String,
        streaming: bool,
        #[serde(default)]
        dsml_compat_count: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        documents: Vec<DocInfo>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        recent_edits: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tasks: Vec<TaskInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_title: Option<String>,
        #[serde(default)]
        prompt_cache_hit_tokens: u32,
        #[serde(default)]
        prompt_cache_miss_tokens: u32,
    },

    #[serde(rename = "done")]
    Done,

    #[serde(rename = "cancelled")]
    Cancelled,

    #[serde(rename = "shutdown_ack")]
    ShutdownAck,

    #[serde(rename = "audit_record")]
    AuditRecord {
        tool_name: String,
        result_summary: String,
        success: bool,
    },
}

/// Streaming block kind — used to distinguish text from thinking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    /// Model is reasoning (thinking phase).
    Thinking,
    /// Agent is executing tool calls — tool names follow in the StreamStart.
    ToolCalling,
    /// Model is generating the visible answer.
    Answering,
}