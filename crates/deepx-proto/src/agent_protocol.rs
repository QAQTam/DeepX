//! UI ↔ Agent frame definitions (JSON-LP over stdin/stdout, child process).
//!
//! v5: Round-based protocol. Each API call is a Round with optional
//! streaming preview. No duplication between streaming and final content.
//! Frontend appends blocks in order — no state machine required.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ═══════════════════════════════════════════════════════════════════════════
// UI → Agent
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

    #[serde(rename = "set_mode")]
    SetMode { mode: String },

    /// Response to a permission request dialog.
    #[serde(rename = "permission_response")]
    PermissionResponse {
        tool_call_id: String,
        approved: bool,
        /// If true, trust the target folder permanently.
        #[serde(default)]
        trust_folder: bool,
    },

    /// User's answers to an ask_user prompt. Resumes a suspended turn.
    /// `answers` contains one entry for Single mode, N entries for Batch mode.
    #[serde(rename = "ask_response")]
    AskResponse {
        /// Matches the ask_id from Agent2Ui::AskUser.
        ask_id: String,
        answers: Vec<AskAnswer>,
    },

    /// User dismissed the ask_user dialog without answering.
    /// Agent should abort the suspended turn.
    #[serde(rename = "ask_dismiss")]
    AskDismiss { ask_id: String },

    /// Unload an explicitly-activated skill ($name mention) from context.
    #[serde(rename = "unload_skill")]
    UnloadSkill {
        /// Skill name (must match the name field in SKILL.md frontmatter).
        name: String,
    },

    /// Explicitly activate a skill by name (equivalent to $skill-name mention).
    /// Triggers SkillsChanged emission on success.
    #[serde(rename = "activate_skill")]
    ActivateSkill {
        /// Skill name (must match the name field in SKILL.md frontmatter).
        name: String,
    },

    /// Reload the skill catalog from disk and refresh the catalog system message.
    #[serde(rename = "reload_skills")]
    ReloadSkills,
}

// ═══════════════════════════════════════════════════════════════════════════
// Shared types
// ═══════════════════════════════════════════════════════════════════════════

/// Information about an available skill for the frontend skills panel.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// "project" or "user"
    pub scope: String,
    /// Display path relative to workspace (e.g. "skills/deepx/deepx-debug")
    pub source: String,
}

/// Payload for Agent2Ui::SkillsChanged.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillsStatus {
    /// All discoverable skills in the workspace.
    pub available: Vec<SkillInfo>,
    /// Names of currently loaded (explicit, $mention-activated) skills.
    pub active: Vec<String>,
}

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
    #[ts(optional)]
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
    #[ts(optional)]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
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
    #[serde(default)]
    pub is_final: bool,
    pub thinking: Option<String>,
    pub answer: Option<String>,
    pub tool_calls: Vec<ToolCallDef>,
    pub tool_results: Vec<ToolResultDef>,
}

/// Backend-owned intrinsic impact of a permission-gated action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionRisk {
    Low,
    Medium,
    High,
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
// Ask-user types (v6: multi-question support)
// ═══════════════════════════════════════════════════════════════════════════

/// Display mode for an ask_user prompt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AskMode {
    /// One question — shown in a modal dialog. Answer is sent immediately.
    #[default]
    Single,
    /// Multiple questions — shown as a form. All answers submitted together.
    Batch,
}

/// How an ask_user prompt left the active queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AskResolution {
    Answered,
    Dismissed,
}

/// One question in an ask_user prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AskQuestion {
    /// Unique ID within this ask (e.g. "q1", "q2").
    pub id: String,
    /// Question text (supports Markdown).
    pub question: String,
    /// Preset choices. Empty = free-text only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Allow the user to type a custom answer instead of picking an option.
    #[serde(default = "default_true")]
    pub allow_custom: bool,
}

/// One answer in a user response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AskAnswer {
    /// Matches AskQuestion.id.
    pub question_id: String,
    /// Selected option text, or custom input.
    pub answer: String,
}

fn default_true() -> bool {
    true
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
    TurnStart { turn_id: String, user_text: String },

    /// Turn complete. All rounds and tool results have been sent.
    #[serde(rename = "turn_end")]
    TurnEnd {
        turn_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
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
        #[ts(optional)]
        thinking: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
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
    ToolExecDelta { tool_call_id: String, delta: String },

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
    SessionCreated { seed: String },

    // ── System events ──
    #[serde(rename = "error")]
    Error { message: String },

    #[serde(rename = "tool_notice")]
    ToolNotice {
        message: String,
        /// "warn" or "error"
        level: String,
    },

    /// PLAN.md changed — frontend should refresh PlanReviewPanel.
    #[serde(rename = "plan_changed")]
    PlanChanged,

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
        #[ts(optional)]
        session_title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        usage: Option<deepx_types::UsageInfo>,
        #[serde(default)]
        context_limit: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        model: Option<String>,
    },

    #[serde(rename = "done")]
    Done,

    #[serde(rename = "compact_start")]
    CompactStart {
        turns_total: u32,
        turns_keeping: u32,
    },

    #[serde(rename = "compact_end")]
    CompactEnd {
        summary_chars: usize,
        turns_compacted: u32,
    },

    /// Streaming delta from the compact LLM call — shown to the user
    /// so they can see the model's summary being built in real-time.
    #[serde(rename = "compact_delta")]
    CompactDelta { delta: String },

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
        /// ISO-8601 timestamp of the tool invocation.
        #[serde(default)]
        time: String,
        /// JSON-serialized tool arguments for formatting.
        #[serde(default)]
        args: String,
    },

    /// Structured streaming output from a running command.
    /// `seq` is monotonic per command and `stream` is either stdout or stderr.
    #[serde(rename = "exec_progress")]
    ExecProgress {
        tool_call_id: String,
        stream: String,
        seq: u64,
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
        #[ts(optional)]
        file: Option<String>,
    },

    /// Heartbeat: daemon responds to frontend ping.
    #[serde(rename = "pong")]
    Pong,

    /// Skills catalog changed — frontend should refresh the skills panel.
    /// Emitted on explicit activation ($name), deactivation (UnloadSkill),
    /// and catalog reload (ReloadSkills).
    #[serde(rename = "skills_changed")]
    SkillsChanged {
        #[serde(flatten)]
        status: SkillsStatus,
    },

    /// Permission request: agent asks user to approve/deny a tool call.
    /// Frontend shows a dialog with tool details and target paths.
    #[serde(rename = "permission_request")]
    PermissionRequest {
        tool_call_id: String,
        tool_name: String,
        /// Human-readable reason for the request.
        reason: String,
        /// Target paths affected by the tool.
        paths: Vec<String>,
        /// Tool category: "read", "write", "exec", "net".
        category: String,
        /// Current permission level (1-4).
        level: u8,
        /// Intrinsic action impact, computed by the backend.
        risk: PermissionRisk,
        /// Plain-language effect of approving the action.
        consequence: String,
    },

    /// Ask-user prompt (v6). Agent suspends turn and waits for user response.
    /// Frontend shows AskDialog (Single) or AskForm (Batch).
    #[serde(rename = "ask_user")]
    AskUser {
        /// Turn containing the original ask_user tool call.
        turn_id: String,
        /// Assistant round containing the original ask_user tool call.
        round_num: u32,
        /// Original ask_user tool-call ID.
        ask_id: String,
        /// How to present the questions.
        #[serde(default)]
        mode: AskMode,
        /// One question per entry. Single mode typically has 1; Batch has N.
        questions: Vec<AskQuestion>,
    },

    /// The active ask was accepted or dismissed by the agent.
    #[serde(rename = "ask_resolved")]
    AskResolved {
        ask_id: String,
        resolution: AskResolution,
    },

    /// The ask response was rejected without consuming the active prompt.
    #[serde(rename = "ask_rejected")]
    AskRejected { ask_id: String, message: String },
}

fn default_load_count() -> u32 {
    20
}

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
    #[ts(optional)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_response_single_round_trip() {
        let json = r#"{"type":"ask_response","ask_id":"a1","answers":[{"question_id":"q1","answer":"Option A"}]}"#;
        let frame: Ui2Agent = serde_json::from_str(json).expect("deserialize AskResponse");
        match &frame {
            Ui2Agent::AskResponse { ask_id, answers } => {
                assert_eq!(ask_id, "a1");
                assert_eq!(answers.len(), 1);
                assert_eq!(answers[0].question_id, "q1");
                assert_eq!(answers[0].answer, "Option A");
            }
            other => panic!(
                "expected AskResponse, got {:?}",
                std::any::type_name_of_val(other)
            ),
        }
        let back = serde_json::to_string(&frame).expect("serialize");
        assert!(back.contains("\"type\":\"ask_response\""));
        assert!(back.contains("\"ask_id\":\"a1\""));
    }

    #[test]
    fn ask_response_batch_round_trip() {
        let json = r#"{"type":"ask_response","ask_id":"a2","answers":[{"question_id":"q1","answer":"A"},{"question_id":"q2","answer":"Custom"}]}"#;
        let frame: Ui2Agent = serde_json::from_str(json).expect("deserialize batch");
        match &frame {
            Ui2Agent::AskResponse { ask_id, answers } => {
                assert_eq!(ask_id, "a2");
                assert_eq!(answers.len(), 2);
                assert_eq!(answers[0].question_id, "q1");
                assert_eq!(answers[1].answer, "Custom");
            }
            other => panic!(
                "expected AskResponse, got {:?}",
                std::any::type_name_of_val(other)
            ),
        }
    }

    #[test]
    fn ask_dismiss_round_trip() {
        let json = r#"{"type":"ask_dismiss","ask_id":"a1"}"#;
        let frame: Ui2Agent = serde_json::from_str(json).expect("deserialize AskDismiss");
        match &frame {
            Ui2Agent::AskDismiss { ask_id } => assert_eq!(ask_id, "a1"),
            other => panic!(
                "expected AskDismiss, got {:?}",
                std::any::type_name_of_val(other)
            ),
        }
    }

    #[test]
    fn ask_user_single_serialize() {
        let event = Agent2Ui::AskUser {
            turn_id: "t1".into(),
            round_num: 0,
            ask_id: "a1".into(),
            mode: AskMode::Single,
            questions: vec![AskQuestion {
                id: "q1".into(),
                question: "Choose one".into(),
                options: vec!["A".into(), "B".into()],
                allow_custom: true,
            }],
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"type\":\"ask_user\""));
        assert!(json.contains("\"ask_id\":\"a1\""));
        assert!(json.contains("\"mode\":\"single\""));
    }

    #[test]
    fn ask_user_batch_serialize() {
        let event = Agent2Ui::AskUser {
            turn_id: "t2".into(),
            round_num: 1,
            ask_id: "b1".into(),
            mode: AskMode::Batch,
            questions: vec![
                AskQuestion {
                    id: "q1".into(),
                    question: "Arch?".into(),
                    options: vec!["A".into(), "B".into()],
                    allow_custom: false,
                },
                AskQuestion {
                    id: "q2".into(),
                    question: "Strategy?".into(),
                    options: vec![],
                    allow_custom: true,
                },
            ],
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"type\":\"ask_user\""));
        assert!(json.contains("\"mode\":\"batch\""));
        let back: Agent2Ui = serde_json::from_str(&json).expect("deserialize");
        match &back {
            Agent2Ui::AskUser {
                turn_id,
                round_num,
                ask_id,
                mode,
                questions,
            } => {
                assert_eq!(turn_id, "t2");
                assert_eq!(*round_num, 1);
                assert_eq!(ask_id, "b1");
                assert!(matches!(mode, AskMode::Batch));
                assert_eq!(questions.len(), 2);
                assert!(!questions[0].allow_custom);
                assert!(questions[1].allow_custom);
            }
            other => panic!(
                "expected AskUser, got {:?}",
                std::any::type_name_of_val(other)
            ),
        }
    }

    #[test]
    fn ask_user_round_trip_preserves_turn_and_call_identity() {
        let event = Agent2Ui::AskUser {
            turn_id: "t7".into(),
            round_num: 3,
            ask_id: "call-ask-1".into(),
            mode: AskMode::Batch,
            questions: vec![AskQuestion {
                id: "q1".into(),
                question: "Choose".into(),
                options: vec!["A".into()],
                allow_custom: true,
            }],
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: Agent2Ui = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            decoded,
            Agent2Ui::AskUser {
                turn_id,
                round_num: 3,
                ask_id,
                ..
            } if turn_id == "t7" && ask_id == "call-ask-1"
        ));
    }

    #[test]
    fn ask_acknowledgements_round_trip() {
        let events = [
            Agent2Ui::AskResolved {
                ask_id: "a1".into(),
                resolution: AskResolution::Answered,
            },
            Agent2Ui::AskRejected {
                ask_id: "a1".into(),
                message: "stale ask_id".into(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            serde_json::from_str::<Agent2Ui>(&json).unwrap();
        }
    }

    #[test]
    fn legacy_scalar_ask_response_is_rejected() {
        assert!(
            serde_json::from_str::<Ui2Agent>(r#"{"type":"ask_response","answer":"A"}"#,).is_err()
        );
    }
}
