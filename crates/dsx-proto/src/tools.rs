//! Agent ↔ Tools frame definitions (stdin/stdout JSON-LP over pipes).
//!
//! The tools process is a security boundary: agent sends tool call requests,
//! tools execute them in an isolated subprocess and return results.

use serde::{Deserialize, Serialize};

/// Agent → Tools frames (stdin pipe).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum AgentToTools {
    #[serde(rename = "tools_init")]
    Init {
        allowed_tools: Vec<String>,
        session_seed: String,
        auto_mode: bool,
    },

    #[serde(rename = "tool_call_req")]
    CallReq {
        id: String,
        name: String,
        action: String,
        args: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
    },

    #[serde(rename = "tool_cancel")]
    Cancel {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    #[serde(rename = "tools_shutdown")]
    Shutdown,
}

/// Tools → Agent frames (stdout pipe).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ToolsToAgent {
    #[serde(rename = "tools_ready")]
    Ready {
        tools: Vec<dsx_types::ToolDef>,
    },

    #[serde(rename = "tool_progress")]
    Progress {
        id: String,
        content: String,
        stream_type: String,
    },

    /// Legacy text result (backward compatible).
    #[serde(rename = "tool_result")]
    Result {
        id: String,
        success: bool,
        content: String,
    },

    /// Structured result with is_error flag.
    #[serde(rename = "tool_result_message")]
    ToolResultMessage {
        id: String,
        name: String,
        action: String,
        success: bool,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },

    #[serde(rename = "tool_error")]
    ToolError {
        id: String,
        error: String,
        /// "UNKNOWN_TOOL" | "BLOCKED" | "TIMEOUT" | "PANIC" | "FORBIDDEN"
        code: String,
    },
}
