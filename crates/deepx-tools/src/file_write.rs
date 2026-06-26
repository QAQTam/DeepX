use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use super::file_shared::{unified_diff, diff_stats, normalize_newlines};

pub(super) fn exec_write_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let content = parse_arg(args, "content");
    let append = parse_opt_bool(args, "append").unwrap_or(false);
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line_count = content.lines().count();

    // Read old content if file exists (for diff on overwrite)
    let old_content = std::fs::read_to_string(&path).ok();

    if append {
        use std::io::Write;
        let mut file = match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
            Ok(f) => f,
            Err(e) => return format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        };
        match file.write_all(content.as_bytes()) {
            Ok(_) => {
                if let Some(ref old) = old_content {
                    let old_line_count = old.lines().count();
                    let first_line = if old_line_count == 0 { 1u32 } else { old_line_count as u32 + 1 };
                    format!("[OK] {path}:{first_line} +{line_count} -0 | write_file\n\n+{content_trim}", path = path, first_line = first_line, line_count = line_count, content_trim = content.trim_end())
                } else {
                    format!("[OK] {} — appended {} bytes, {} lines (new file)", path, content.len(), line_count)
                }
            }
            Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        }
    } else {
        match std::fs::write(&path, &content) {
            Ok(_) => {
                if let Some(ref old) = old_content {
                    // Overwrite: show full diff
                    let (old_norm, _) = normalize_newlines(old);
                    let (new_norm, _) = normalize_newlines(&content);
                    let diff = unified_diff(&old_norm, &new_norm, &path);
                    if diff.is_empty() {
                        format!("[OK] {} — {} bytes, {} lines (no changes)", path, content.len(), line_count)
                    } else {
                        let (added, removed, first_line) = diff_stats(&diff);
                        format!("[OK] {path}:{first_line} +{added} -{removed} | write_file\n\n{diff}", path = path, first_line = first_line, added = added.max(1), removed = removed.max(1), diff = diff.trim_end())
                    }
                } else {
                    format!("[OK] {} — {} bytes, {} lines (new file)", path, content.len(), line_count)
                }
            }
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
