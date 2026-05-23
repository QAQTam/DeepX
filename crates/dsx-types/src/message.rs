use serde::{Deserialize, Serialize};

// ── Anthropic-native content blocks ──

/// Content block within a message, matching Anthropic Messages API spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        signature: String,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

impl ContentBlock {
    pub fn text(text: &str) -> Self {
        ContentBlock::Text { text: text.to_string() }
    }
}

// ── Messages ──

/// A conversation message using Anthropic-native content-block format.
///
/// Roles:
/// - `"user"` — contains `Text` and/or `ToolResult` blocks
/// - `"assistant"` — contains `Text`, `Thinking`, and/or `ToolUse` blocks
/// - `"system"` — internal only, never sent to API (handled by `build_context`)
/// - `"tool"` — internal only, converted to `role:"user"`+`ToolResult` by assembler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

#[allow(dead_code)]
impl Message {
    /// Internal system message (extracted by `build_context`, never sent to API).
    pub fn system(content: &str) -> Self {
        Self {
            role: "system".into(),
            content: vec![ContentBlock::text(content)],
        }
    }
    /// Anthropic user message with a single text block.
    pub fn user(content: &str) -> Self {
        Self {
            role: "user".into(),
            content: vec![ContentBlock::text(content)],
        }
    }
    /// Empty assistant message (blocks added after streaming).
    pub fn assistant_empty() -> Self {
        Self {
            role: "assistant".into(),
            content: vec![],
        }
    }
    /// Internal tool result (assembler merges into user role).
    pub fn tool(tool_call_id: &str, result: &str) -> Self {
        Self {
            role: "tool".into(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_call_id.into(),
                content: result.into(),
                is_error: None,
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
    pub arguments: String, // JSON string
}

#[allow(dead_code)]
impl ToolCall {
    pub fn try_parse_args<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_str(&self.function.arguments).ok()
    }
}
