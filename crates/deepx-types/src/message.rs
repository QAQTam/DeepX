use serde::{Deserialize, Serialize};

// ── OpenAI-native content blocks ──

/// Content block within a message, matching OpenAI / DeepSeek Chat Completions API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        reasoning: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

impl ContentBlock {
    pub fn text(text: &str) -> Self {
        ContentBlock::Text { text: text.to_string() }
    }
}

// ── Messages ──

/// A conversation message using OpenAI-native content-block format.
///
/// Roles:
/// - `"user"` — contains `Text` and/or `ToolResult` blocks
/// - `"assistant"` — contains `Text`, `Reasoning`, and/or `ToolUse` blocks
/// - `"system"` — system-level context and instructions
/// - `"tool"` — tool execution results
///
/// The optional `name` field distinguishes same-role participants
/// (e.g. `name="docs"` for injected document context, `name="code"` for
/// code snippets). It maps to OpenAI's `name` parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Monotonic per-session message ID for ordering and dedup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<u64>,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn system(content: &str) -> Self {
        Self {
            msg_id: None,
            role: "system".into(),
            name: None,
            content: vec![ContentBlock::text(content)],
        }
    }
    pub fn user(content: &str) -> Self {
        Self {
            msg_id: None,
            role: "user".into(),
            name: None,
            content: vec![ContentBlock::text(content)],
        }
    }
    pub fn tool(tool_call_id: &str, result: &str) -> Self {
        Self {
            msg_id: None,
            role: "tool".into(),
            name: None,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_call_id.into(),
                content: result.into(),
            }],
        }
    }
}

// ── Tool Call (kept for IPC, XML/DSML parsing, and backward compat) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}


