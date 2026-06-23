//! sort tool — sort lines of a file, with optional unique/dedup.

use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_sort(args: &str) -> String {
    let path = parse_arg(args, "path");
    let unique = parse_opt_bool(args, "unique").unwrap_or(false);
    let reverse = parse_opt_bool(args, "reverse").unwrap_or(false);

    if path.is_empty() {
        return "[ERROR] sort: path required".into();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] sort: cannot read {path}: {e}"),
    };

    run_sort_core_with_content(&content, &path, unique, reverse)
}

handler!(handle_sort, exec_sort);

/// Sort lines of a file with raw args — used by linuxmod.
pub(crate) fn run_sort_core(path: &str, unique: bool, reverse: bool) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] sort: cannot read {path}: {e}"),
    };
    run_sort_core_with_content(&content, path, unique, reverse)
}

/// Sort already-loaded content — used by linuxmod pipe segments.
pub(crate) fn run_sort_str(content: &str, unique: bool, reverse: bool) -> String {
    run_sort_core_with_content(content, "<stdin>", unique, reverse)
}

fn run_sort_core_with_content(content: &str, label: &str, unique: bool, reverse: bool) -> String {
    let mut lines: Vec<&str> = content.lines().collect();

    if reverse {
        lines.sort_by(|a, b| b.cmp(a));
    } else {
        lines.sort();
    }

    if unique {
        lines.dedup();
    }

    let count = lines.len();
    let result = lines.join("\n");

    if result.is_empty() {
        format!("[OK] sort: {label} → empty")
    } else {
        format!("[OK] sort: {label} → {count} lines\n\n{result}")
    }
}

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("sort", ""),
        description: "Sort lines of a file. Supports reverse and unique (dedup).\nExamples:\nSort  →  {\"path\":\"data.txt\"}\nSort unique  →  {\"path\":\"data.txt\",\"unique\":true}\nReverse sort  →  {\"path\":\"data.txt\",\"reverse\":true}",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"unique":{"type":"boolean","description":"Remove duplicate lines","default":false},"reverse":{"type":"boolean","description":"Reverse sort order","default":false}},"required":["path"],"additionalProperties":false}),
        handler: handle_sort,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}
