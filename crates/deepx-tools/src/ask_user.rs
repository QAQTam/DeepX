use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_ask_user(args: &str) -> String {
    let question = parse_arg(args, "question");
    if question.is_empty() {
        return "[ERROR] ask_user: question required".into();
    }
    let options: Vec<String> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("options")?.as_array().cloned())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let allow_custom = parse_opt_bool(args, "allow_custom").unwrap_or(true);

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
        description: "Ask the user a question when blocked. question: what to ask. options: preset choices (optional array). allow_custom: let user type free text (default true).",
        input_schema: serde_json::json!({"type":"object","properties":{"question":{"type":"string","description":"The question to ask"},"options":{"type":"array","items":{"type":"string"},"description":"Preset answer choices"},"allow_custom":{"type":"boolean","description":"Allow custom text input","default":true}},"required":["question"],"additionalProperties":false}),
        handler: handle_ask_user,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(10),
    });
}
