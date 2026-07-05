//! UI ↔ Agent frame definitions (JSON-LP over stdin/stdout, child process).
//!
//! v5: Round-based protocol. Each API call is a Round with optional
//! streaming preview. No duplication between streaming and final content.
//! Frontend appends blocks in order — no state machine required.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ═══════════════════════════════════════════════════════════════════════════
// UI → Agent (unchanged from v4)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
#[non_exhaustive]
#[ts(export)]
pub enum Ui2Agent {
    #[serde(rename = "user_input")]
    UserInput { text: String },

    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        action: String,
        #[ts(type = "any")]
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

    #[serde(rename = "undo_turn")]
    UndoTurn { turn_id: String },

    #[serde(rename = "compact")]
    Compact,

    #[serde(rename = "resume_session")]
    ResumeSession { seed: String },

    #[serde(rename = "new_session")]
    NewSession,

    #[serde(rename = "load_more_turns")]
    LoadMoreTurns {
        /// Load turns older than this turn_id.
        before_turn_id: String,
        /// How many turns to load.
        #[serde(default = "default_load_count")]
        count: u32,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared types
// ═══════════════════════════════════════════════════════════════════════════

/// Tool call definition sent in RoundComplete.tool_calls.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolCallDef {
    pub id: String,
    pub name: String,
    /// Human-readable args summary (e.g. "foo.rs", "search pattern")
    pub args_display: String,
    /// Raw JSON arguments string
    pub args_json: String,
}

/// Tool execution result sent in ToolResults.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolResultDef {
    pub tool_call_id: String,
    pub output: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileSnapshotInfo>,
}

/// File metadata snapshot for rich rendering.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DocInfo {
    pub tag: String,
    pub path: String,
    pub turns_since_read: u32,
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskInfo {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: String,
}

/// One round of a turn (one API call).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoundData {
    pub round_num: u32,
    pub thinking: Option<String>,
    pub answer: Option<String>,
    pub tool_calls: Vec<ToolCallDef>,
    pub tool_results: Vec<ToolResultDef>,
}

/// One full turn (user message + all rounds).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TurnData {
    pub turn_id: String,
    pub user_text: String,
    pub rounds: Vec<RoundData>,
}

/// One block in a round, preserving the LLM's output order.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum RoundBlock {
    Reasoning { content: String },
    Text { content: String },
    Tool { card: ToolCallDef },
}

// ═══════════════════════════════════════════════════════════════════════════
// Agent → UI (v5 — round-based)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
#[non_exhaustive]
#[ts(export)]
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
        /// Total number of turns in this session.
        #[serde(default)]
        total_turns: u32,
        /// True if there are more (older) turns beyond what's sent.
        #[serde(default)]
        has_more: bool,
    },

    /// Older turns loaded from history.
    #[serde(rename = "more_turns")]
    MoreTurns {
        turns: Vec<TurnData>,
        /// True if there are still more (older) turns available.
        has_more: bool,
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
        #[serde(default)]
        tasks: Vec<TaskInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<deepx_types::UsageInfo>,
        #[serde(default)]
        context_limit: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },

    #[serde(rename = "done")]
    Done,

    #[serde(rename = "compact_start")]
    CompactStart { turns_total: u32, turns_keeping: u32 },

    #[serde(rename = "compact_end")]
    CompactEnd { summary_chars: usize, turns_compacted: u32 },

    #[serde(rename = "cancelled")]
    Cancelled,

    #[serde(rename = "shutdown_ack")]
    ShutdownAck,

    #[serde(rename = "ready")]
    Ready,

    #[serde(rename = "audit_record")]
    AuditRecord {
        tool_name: String,
        result_summary: String,
        success: bool,
    },

    /// Streaming output chunk from a running tool (e.g. exec stdout).
    #[serde(rename = "exec_progress")]
    ExecProgress {
        tool_call_id: String,
        chunk: String,
    },

    /// Tool call detected in streaming response — preview card before execution.
    #[serde(rename = "tool_call_preview")]
    ToolCallPreview {
        turn_id: String,
        round_num: u32,
        index: usize,
        id: String,
        name: String,
        args_so_far: String,
    },

    /// Realtime code stats delta from a file operation (write/edit/delete/move).
    #[serde(rename = "code_delta")]
    CodeDelta {
        lines_added: usize,
        lines_removed: usize,
        files_created: usize,
        files_deleted: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
}

fn default_load_count() -> u32 { 20 }

/// Streaming block kind for RoundDelta.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoundDeltaKind {
    /// Model is reasoning (thinking phase).
    Thinking,
    /// Agent is executing tool calls — tool names follow.
    ToolCalling,
    /// Model is generating the visible answer.
    Answering,
}

/// A single code delta record for persistence.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CodeDeltaRecord {
    pub timestamp: u64,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub files_created: usize,
    pub files_deleted: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Daily aggregated code stats.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CodeDaily {
    pub date: String,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub files_created: usize,
    pub files_deleted: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// Daemon ↔ Frontend protocol (socket transport)
// ═══════════════════════════════════════════════════════════════════════════

/// Frontend → Daemon frame. Wraps Ui2Agent with the target session seed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendToDaemon {
    pub seed: String,
    #[serde(flatten)]
    pub frame: Ui2Agent,
}

/// Daemon → Frontend frame. Wraps Agent2Ui with the source session seed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonToFrontend {
    pub seed: String,
    #[serde(flatten)]
    pub event: Agent2Ui,
}
