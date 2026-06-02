use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict, handler};

pub(super) fn exec_read_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let start: Option<usize> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("start_line")?.as_u64().map(|n| (n as usize).max(1)));
    let end: Option<usize> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("end_line")?.as_u64().map(|n| n as usize));

    const MAX_READ_LINES: usize = 500;
    if let (Some(s), Some(e)) = (start, end) {
        if e > s && e - s > MAX_READ_LINES {
            return format!("[ERROR] Requested range too large ({} lines > {} max). Use smaller range.", e - s, MAX_READ_LINES);
        }
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let all_lines: Vec<&str> = content.lines().collect();
            let total = all_lines.len();
            let start_idx = start.map(|s| (s - 1).min(total)).unwrap_or(0);
            let end_idx = end.map(|e| e.min(total)).unwrap_or(total);
            let start_idx = start_idx.min(end_idx);
            let lines: Vec<&str> = all_lines[start_idx..end_idx].to_vec();
            let shown = lines.len();
            let total_lines = all_lines.len();

            if start.is_some() || end.is_some() {
                let mut result = format!("[OK] {} lines {}-{}/{} of {}\n", shown, start_idx + 1, end_idx, total_lines, path);
                for (i, l) in lines.iter().enumerate() {
                    result.push_str(&format!("{:>6}  {}\n", start_idx + i + 1, l));
                }
                result
            } else {
                let head: Vec<&str> = lines.iter().take(50).cloned().collect();
                let tail: Vec<&str> = lines.iter().rev().take(10).collect::<Vec<_>>().into_iter().rev().cloned().collect();
                let mut result = format!("[PARTIAL] {} lines, showing 1-50/{}\n", total_lines, path);
                for (i, l) in head.iter().enumerate() {
                    result.push_str(&format!("{:>6}  {}\n", i + 1, l));
                }
                if total_lines > 50 {
                    result.push_str("  ⋮\n");
                    for (i, l) in tail.iter().enumerate() {
                        result.push_str(&format!("{:>6}  {}\n", total_lines - tail.len() + i + 1, l));
                    }
                    result.push_str(&format!("[HINT] Use start_line=N end_line=N to read specific lines.\n"));
                }
                result
            }
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("valid UTF-8") || err_msg.contains("utf8") || err_msg.contains("utf-8") {
                let meta = std::fs::metadata(&path);
                let size = meta.as_ref().map(|m| format!(", {}B", m.len())).unwrap_or_default();
                format!("[PARTIAL] {} — binary file{} (cannot display as text)\n[HINT] Use exec(\"file '{}'\") to identify format, or exec(\"xxd '{}'\") for hex dump.", path, size, path, path)
            } else {
                let url_hint = if path.contains("://") || path.contains(".com") || path.contains("www.") {
                    "\n[HINT] This looks like a URL — did you mean to call web_fetch() instead of read_file()?"
                } else { "" };
                format!("[ERROR] Cannot read {}: {}\n[HINT] Use list_dir() on the parent directory to verify the file exists, or check the path spelling.{}", path, e, url_hint)
            }
        },
    }
}

handler!(handle_read_file, exec_read_file);

fn default_allow(_ctx: &ToolCallCtx) -> SafetyVerdict { SafetyVerdict::Allow }

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("read_file", ""),
        description: "Read file content. Default preview: first 50 lines + last 10 lines. Use start_line/end_line for precise range.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"start_line":{"type":"integer","description":"First line to read (1-based)","default":1},"end_line":{"type":"integer","description":"Last line to read (inclusive). If omitted, reads to end of file."}},"required":["path"],"additionalProperties":false}),
        handler: handle_read_file,
        safety: default_allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}
