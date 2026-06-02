use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict, handler};

pub(super) fn exec_diff(args: &str) -> String {
    let path_a = parse_arg(args, "path_a");
    let path_b = parse_arg(args, "path_b");

    let content_a = match std::fs::read_to_string(&path_a) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] Cannot read {}: {}\n[HINT] Verify the file exists. Use list_dir() to check.", path_a, e),
    };
    let content_b = match std::fs::read_to_string(&path_b) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] Cannot read {}: {}\n[HINT] Verify the file exists. Use list_dir() to check.", path_b, e),
    };

    if content_a == content_b {
        return "[OK] Files are identical".to_string();
    }

    let lines_a: Vec<&str> = content_a.lines().collect();
    let lines_b: Vec<&str> = content_b.lines().collect();

    // Find first differing line
    let mut first_diff = 0usize;
    while first_diff < lines_a.len() && first_diff < lines_b.len() && lines_a[first_diff] == lines_b[first_diff] {
        first_diff += 1;
    }

    let ctx_start = first_diff.saturating_sub(2);
    let window = 3; // lines to show on each side of the diff

    let mut result = String::new();
    let mut line_count = 0usize;
    let cap = 200usize;

    // Context before
    for i in ctx_start..first_diff {
        result.push_str(&format!("  {}\n", lines_a[i]));
        line_count += 1;
        if line_count >= cap { return result; }
    }
    // Removed lines
    for i in first_diff..(first_diff + window).min(lines_a.len()) {
        result.push_str(&format!("- {}\n", lines_a[i]));
        line_count += 1;
        if line_count >= cap { return result; }
    }
    // Added lines
    for i in first_diff..(first_diff + window).min(lines_b.len()) {
        result.push_str(&format!("+ {}\n", lines_b[i]));
        line_count += 1;
        if line_count >= cap { return result; }
    }
    // Context after
    let after_start = first_diff + window;
    let after_end = after_start + 2;
    for i in after_start..after_end.min(lines_b.len().max(lines_a.len())) {
        if i < lines_a.len() {
            result.push_str(&format!("  {}\n", lines_a[i]));
            line_count += 1;
            if line_count >= cap { return result; }
        }
    }
    result
}

handler!(handle_diff, exec_diff);

fn default_allow(_ctx: &ToolCallCtx) -> SafetyVerdict { SafetyVerdict::Allow }

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("diff", ""),
        description: "Compare two files line by line. Shows first diff region with context.",
        input_schema: serde_json::json!({"type":"object","properties":{"path_a":{"type":"string","description":"First file path"},"path_b":{"type":"string","description":"Second file path"}},"required":["path_a","path_b"],"additionalProperties":false}),
        handler: handle_diff,
        safety: default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
