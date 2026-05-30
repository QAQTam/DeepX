//! ask_user tool — ask the user a question mid-task.
//! Used when the model needs a decision it can't make alone
//! (e.g., library choice, configuration preference, ambiguous requirement).

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

    let mut content = format!("[ASK_USER] {}", question);
    if !options.is_empty() {
        for (i, opt) in options.iter().enumerate() {
            content.push_str(&format!("\n  {}. {}", i + 1, opt));
        }
    }
    ToolResult { success: true, content }
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
