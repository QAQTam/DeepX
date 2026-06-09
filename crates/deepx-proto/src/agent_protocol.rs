//! UI ↔ Agent frame definitions (channel-based, mpsc in-process).
//!
//! v5: Round-based protocol. Each API call is a Round with optional
//! streaming preview. No duplication between streaming and final content.
//! Frontend appends blocks in order — no state machine required.

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════
// UI → Agent (unchanged from v4)
// ═══════════════════════════════════════════════════════════════════════════

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

    #[serde(rename = "create_session")]
    CreateSession,

    #[serde(rename = "cancel")]
    Cancel,

    #[serde(rename = "shutdown")]
    Shutdown,

    #[serde(rename = "reload_config")]
    ReloadConfig,

    #[serde(rename = "debug_cmd")]
    DebugCommand { cmd: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared types
// ═══════════════════════════════════════════════════════════════════════════

/// Tool call definition sent in RoundComplete.tool_calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDef {
    pub id: String,
    pub name: String,
    /// Human-readable args summary (e.g. "foo.rs", "search pattern")
    pub args_display: String,
    /// Raw JSON arguments string
    pub args_json: String,
}

/// Tool execution result sent in ToolResults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultDef {
    pub tool_call_id: String,
    pub output: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileSnapshotInfo>,
}

/// File metadata snapshot for rich rendering.
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

/// Document tracking entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocInfo {
    pub tag: String,
    pub path: String,
    pub turns_since_read: u32,
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: String,
}

/// One round of a turn (one API call).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundData {
    pub round_num: u32,
    pub thinking: Option<String>,
    pub answer: Option<String>,
    pub tool_calls: Vec<ToolCallDef>,
    pub tool_results: Vec<ToolResultDef>,
}

/// One full turn (user message + all rounds).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnData {
    pub turn_id: String,
    pub user_text: String,
    pub rounds: Vec<RoundData>,
}

/// One block in a round, preserving the LLM's output order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoundBlock {
    Reasoning { content: String },
    Text { content: String },
    Tool { card: ToolCallDef },
}

// ═══════════════════════════════════════════════════════════════════════════
// Agent → UI (v5 — round-based)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Agent2Ui {
    // ── Turn lifecycle ──

    /// A new turn starts. Frontend creates a user message + turn container.
    #[serde(rename = "turn_start")]
    TurnStart {
        turn_id: String,
        user_text: String,
    },

    /// Turn complete. All rounds and tool results have been sent.
    #[serde(rename = "turn_end")]
    TurnEnd {
        turn_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<deepx_types::UsageInfo>,
    },

    // ── Streaming preview (optional, additive) ──

    /// Live typing preview for the current round.
    /// Frontend shows this as a draft; RoundComplete replaces it.
    #[serde(rename = "round_delta")]
    RoundDelta {
        turn_id: String,
        round_num: u32,
        kind: RoundDeltaKind,
        delta: String,
    },

    // ── Round complete (authoritative) ──

    /// One API call finished. Contains everything the model produced.
    /// Frontend replaces any draft from RoundDelta with this content.
    #[serde(rename = "round_complete")]
    RoundComplete {
        turn_id: String,
        round_num: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        answer: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCallDef>,
        /// Ordered blocks matching LLM output sequence (preferred).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        blocks: Vec<RoundBlock>,
        /// true = this is the final round of the turn
        is_final: bool,
    },

    // ── Tool execution results ──

    /// Results from executing the tool calls in a RoundComplete.
    /// Sent after each tool finishes, before the next round or TurnEnd.
    #[serde(rename = "tool_results")]
    ToolResults {
        turn_id: String,
        round_num: u32,
        results: Vec<ToolResultDef>,
    },

    /// Real-time stdout/stderr chunk from a running exec tool.
    /// Frontend accumulates these until the corresponding ToolResult arrives.
    #[serde(rename = "tool_exec_delta")]
    ToolExecDelta {
        tool_call_id: String,
        delta: String,
    },

    // ── Session restore ──

    /// Full session history sent on resume.
    #[serde(rename = "session_restored")]
    SessionRestored {
        seed: String,
        turns: Vec<TurnData>,
        tokens_used: u32,
        #[serde(default)]
        cache_hit_pct: f64,
    },

    /// A new session was created (response to CreateSession).
    #[serde(rename = "session_created")]
    SessionCreated {
        seed: String,
    },

    // ── System events ──

    #[serde(rename = "error")]
    Error { message: String },

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

    #[serde(rename = "dashboard")]
    Dashboard {
        hp_connected: bool,
        session_seed: String,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<deepx_types::UsageInfo>,
        #[serde(default)]
        context_limit: u32,
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

/// Streaming block kind for RoundDelta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundDeltaKind {
    /// Model is reasoning (thinking phase).
    Thinking,
    /// Agent is executing tool calls — tool names follow.
    ToolCalling,
    /// Model is generating the visible answer.
    Answering,
}
