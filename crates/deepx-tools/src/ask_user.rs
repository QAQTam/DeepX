use crate::{ToolHandler, ToolKey, ToolRisk, ToolCallCtx, ToolResult, handler, JsonArgs};

pub(super) fn exec_ask_user(args: &serde_json::Value) -> String {
    let question = args.s("question");
    if question.is_empty() {
        return "[ERROR] ask_user: question required".into();
    }
    let options: Vec<String> = args.get("options")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let allow_custom = args.opt_bool("allow_custom").unwrap_or(true);

    let payload = serde_json::json!({
        "question": question,
        "options": options,
        "allow_custom": allow_custom,
    });

    format!("[USER_QUERY] {}", serde_json::to_string(&payload).unwrap_or_default())
}

handler!(handle_ask_user, exec_ask_user);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("ask_user", ""),
        description: "Ask the user a question when blocked. question: what to ask (supports **Markdown** for A/B/C sections). options: preset choices (optional array, e.g. [\"Option A\", \"Option B\", \"Other\"]). allow_custom: let user type free text instead of picking an option (default true).",
        input_schema: serde_json::json!({"type":"object","properties":{"question":{"type":"string","description":"The question to ask"},"options":{"type":"array","items":{"type":"string"},"description":"Preset answer choices"},"allow_custom":{"type":"boolean","description":"Allow custom text input","default":true}},"required":["question"],"additionalProperties":false}),
        handler: handle_ask_user,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(10),
    });
}
