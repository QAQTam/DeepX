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
        format!("[OK] sort: {path} → empty")
    } else {
        let truncated = if count > 200 {
            let first_200: Vec<&str> = lines.iter().take(200).copied().collect();
            format!("{}\n... ({} more lines)", first_200.join("\n"), count - 200)
        } else {
            result
        };
        format!("[OK] sort: {path} → {count} lines\n\n{truncated}")
    }
}

handler!(handle_sort, exec_sort);

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
