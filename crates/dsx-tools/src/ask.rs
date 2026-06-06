//! ask_user tool — ask the user a question mid-task (NEW interrupt-based design).
//!
//! Returns an `InterruptRequest` via `ToolResult::interrupt`. The agent
//! pauses the turn loop, sends the prompt to the UI, and resumes when
//! the user replies. The reply is injected as a normal tool_result.

use super::{ToolHandler, ToolKey, ToolCallCtx, ToolResult};
use std::time::Duration;

fn handle_ask_user(ctx: ToolCallCtx) -> ToolResult {
    let question = ctx.get_str("question").unwrap_or("").to_string();
    let options: Vec<String> = ctx.args.get("options")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
        .unwrap_or_default();

    let prompt = if question.is_empty() {
        "Please provide input:".to_string()
    } else {
        question
    };

    ToolResult::interrupt(prompt, options)
}

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("ask_user", ""),
        description: "Ask the user a question when you need a decision. \
            Provide a clear question and optional options (max 5). \
            The user will select an option or type a free-text answer.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user. Be specific and concise."
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional choices for the user to pick from (max 5). \
                        Each option should be a short label with optional brief explanation.",
                    "minItems": 2,
                    "maxItems": 5
                }
            },
            "required": ["question"],
            "additionalProperties": false
        }),
        handler: handle_ask_user,
        safety: |_| crate::SafetyVerdict::Allow,
        default_timeout: Duration::from_secs(300),
    });
}
