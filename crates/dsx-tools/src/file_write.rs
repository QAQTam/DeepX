use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_write_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let content = parse_arg(args, "content");
    let append = parse_opt_bool(args, "append").unwrap_or(false);
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line_count = content.lines().count();
    if append {
        use std::io::Write;
        let mut file = match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
            Ok(f) => f,
            Err(e) => return format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        };
        match file.write_all(content.as_bytes()) {
            Ok(_) => format!("[OK] {} — appended {} bytes, {} lines", path, content.len(), line_count),
            Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        }
    } else {
        match std::fs::write(&path, &content) {
            Ok(_) => format!("[OK] {} — {} bytes, {} lines", path, content.len(), line_count),
            Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        }
    }
}

handler!(handle_write_file, exec_write_file);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("write_file", ""),
        description: "Create, overwrite, or append to a file. Creates parent dirs. For new files or full rewrites; prefer edit_file for small changes.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"If true, append to file instead of overwriting","default":false},"reason":{"type":"string","description":"Why this change is needed (optional)"}},"required":["path","content"],"additionalProperties":false}),
        handler: handle_write_file,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
