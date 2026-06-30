use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use super::file_shared::unified_diff;

pub(super) fn exec_diff(args: &str) -> String {
    let path_a = crate::resolve_workspace_path(&parse_arg(args, "path_a"));
    let path_b = crate::resolve_workspace_path(&parse_arg(args, "path_b"));

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

    format!("[OK]\n{}", unified_diff(&content_a, &content_b, &path_a))
}

handler!(handle_diff, exec_diff);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "diff"),
        description: "Compare two files line by line.",
        input_schema: serde_json::json!({"type":"object","properties":{"path_a":{"type":"string","description":"First file path"},"path_b":{"type":"string","description":"Second file path"}},"required":["path_a","path_b"],"additionalProperties":false}),
        handler: handle_diff,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
