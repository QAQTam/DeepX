//! wc tool — word/line/byte counter. Model uses `wc -l` to check file sizes.

use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_wc(args: &str) -> String {
    let path = parse_arg(args, "path");
    let lines_only = parse_opt_bool(args, "lines").unwrap_or(false);

    if path.is_empty() {
        return "[ERROR] wc: path required".into();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] wc: cannot read {path}: {e}"),
    };

    run_wc_core(&content, &path, lines_only)
}

handler!(handle_wc, exec_wc);

/// Count lines/words/chars/bytes with raw args — used by linuxmod.
pub(crate) fn run_wc_core(content: &str, label: &str, lines_only: bool) -> String {
    let line_count = content.lines().count();
    let byte_count = content.len();

    if lines_only {
        format!("[OK] wc: {label} → {} lines", line_count)
    } else {
        let word_count = content.split_whitespace().count();
        let char_count = content.chars().count();
        format!("[OK] wc: {label} → {} lines, {} words, {} chars, {} bytes", line_count, word_count, char_count, byte_count)
    }
}

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("wc", ""),
        description: "Count lines, words, and bytes in a file.\nExamples:\nLine count  →  {\"path\":\"src/main.rs\",\"lines\":true}\nAll stats  →  {\"path\":\"src/main.rs\"}",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"lines":{"type":"boolean","description":"Show only line count","default":false}},"required":["path"],"additionalProperties":false}),
        handler: handle_wc,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(10),
    });
}
