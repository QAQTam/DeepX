//! UI helpers: assistant message building, tool display formatting (v5).

use dsx_proto::ToolCallDef;
use dsx_types::{ContentBlock, Message, ToolCall};

use crate::agent::AgentState;

/// Build an assistant message from LLM response parts and push to context.
pub fn build_and_push_assistant(
    agent: &mut AgentState,
    content: &str,
    reasoning_content: &Option<String>,
    parsed: &[ToolCall],
) -> Message {
    let mut blocks: Vec<ContentBlock> = Vec::new();
    if let Some(ref rc) = reasoning_content {
        if !rc.is_empty() {
            blocks.push(ContentBlock::Reasoning {
                reasoning: rc.clone(),
            });
        }
    }
    if !content.is_empty() {
        blocks.push(ContentBlock::Text {
            text: content.to_string(),
        });
    }
    for tc in parsed {
        let input: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
        blocks.push(ContentBlock::ToolUse {
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            input,
        });
    }

    if blocks.is_empty() {
        blocks.push(ContentBlock::Text {
            text: "[Empty response]".to_string(),
        });
    }
    let assistant_msg = Message {
        role: "assistant".into(),
        name: None,
        content: blocks,
    };

    agent.ctx.push_assistant(assistant_msg.clone());

    assistant_msg
}

/// Format tool args for UI display: a one-line summary and optional structured body.
pub fn format_tool_display(name: &str, args: &str) -> (String, Option<serde_json::Value>) {
    let parsed: serde_json::Value = serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
    let display = match name {
        "exec" => {
            parsed.get("command").and_then(|v| v.as_str())
                .map(|c| format!("$ {}", c))
                .unwrap_or_else(|| name.to_string())
        }
        "read_file" | "write_file" => {
            parsed.get("path").and_then(|v| v.as_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        "edit_file" | "edit_file_diff" => {
            parsed.get("path").and_then(|v| v.as_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        "explore" => {
            parsed.get("path").or(parsed.get("directory"))
                .and_then(|v| v.as_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        "search" | "grep" | "glob" => {
            parsed.get("pattern").and_then(|v| v.as_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        "task_create" | "task_update" => {
            parsed.get("subject").and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        "web_fetch" => {
            parsed.get("url").and_then(|v| v.as_str())
                .map(|u| u.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        "git_init" | "git_status" | "git_log" | "git_commit" | "git_diff" => {
            parsed.get("path").and_then(|v| v.as_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| name.to_string())
        }
        _ => name.to_string(),
    };

    let body = if name == "edit_file" || name == "edit_file_diff" {
        let old_str = parsed.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
        let new_str = parsed.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
        let file = parsed.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let old_lines: Vec<&str> = old_str.lines().collect();
        let new_lines: Vec<&str> = new_str.lines().collect();
        Some(serde_json::json!({
            "file": file,
            "old_lines": old_lines,
            "new_lines": new_lines,
        }))
    } else if name == "exec" {
        parsed.get("command").map(|c| serde_json::json!({ "command": c }))
    } else {
        None
    };

    (display, body)
}

/// Build a `ToolCallDef` from tool name and args.
pub fn make_tool_def(id: &str, name: &str, args: &str) -> ToolCallDef {
    let (display, _body) = format_tool_display(name, args);
    ToolCallDef {
        id: id.to_string(),
        name: name.to_string(),
        args_display: display,
        args_json: args.to_string(),
    }
}

/// Build a `ToolResultDef` from tool execution output.
pub fn make_tool_result(
    tool_id: &str,
    output: &str,
    success: bool,
    file: Option<dsx_proto::FileSnapshotInfo>,
) -> dsx_proto::ToolResultDef {
    dsx_proto::ToolResultDef {
        tool_call_id: tool_id.to_string(),
        output: output.to_string(),
        success,
        file,
    }
}
