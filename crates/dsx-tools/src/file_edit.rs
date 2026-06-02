use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict, handler};
use super::file_shared::build_diff;

pub(super) fn exec_edit_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let old = parse_arg(args, "old_string");
    let new = parse_arg(args, "new_string");
    let replace_all = parse_opt_bool(args, "replace_all").unwrap_or(false);
    let use_regex = parse_opt_bool(args, "regex").unwrap_or(false);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("valid UTF-8") || err_msg.contains("utf8") || err_msg.contains("utf-8") {
                return format!("[PARTIAL] {} — binary file, edit_file works on text only\n[HINT] Use exec with appropriate tool for binary files.", path);
            }
            return format!("[ERROR] Cannot read {}: {}\n[HINT] Use list_dir() on the parent directory to verify the file exists.", path, e);
        },
    };

    if use_regex {
        let re = match regex::Regex::new(&old) {
            Ok(r) => r,
            Err(e) => return format!("[ERROR] Invalid regex: {}\n[HINT] old_string is not a valid regex pattern.", e),
        };
        let count = re.find_iter(&content).count();
        if count == 0 {
            return format!("[PARTIAL] {} — regex no matches\n[HINT] Verify the regex pattern matches the file content.", path);
        }
        let new_content = if replace_all {
            re.replace_all(&content, &new).to_string()
        } else {
            re.replacen(&content, 1, &new).to_string()
        };
        match std::fs::write(&path, &new_content) {
            Ok(_) => {
                let r_count = if replace_all { count } else { 1 };
                format!("[OK] {} — regex replaced {} match(es)\n[HINT] Pattern: /{}/ → {}", path, r_count, old, new)
            }
            Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        }
    } else if replace_all {
        let new_content = content.replace(&old, &new);
        if new_content == content {
            return format!("[PARTIAL] {} — no occurrences found\n[HINT] Verify the old_string is correct.", path);
        }
        let count = content.matches(&old).count();
        match std::fs::write(&path, &new_content) {
            Ok(_) => {
                let diff = build_diff(&content, &new_content, &old, &new, &path);
                format!("[OK] {} — replaced {} occurrences, +{} -{}\n\n{}", path, count, new.len() * count, old.len() * count, diff)
            }
            Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
        }
    } else {
        match content.find(&old) {
            Some(pos) => {
                let new_content = content.replacen(&old, &new, 1);
                let line = content[..pos].lines().count() + 1;
                match std::fs::write(&path, &new_content) {
                    Ok(_) => {
                        let diff = build_diff(&content, &new_content, &old, &new, &path);
                        format!("[OK] {}:{} +{} -{}\n\n{}", path, line, new.len(), old.len(), diff)
                    }
                    Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
                }
            }
            None => format!("[PARTIAL] {} — string not found\n[HINT] The old_string may have changed. Re-read the file and try again.", path),
        }
    }
}

handler!(handle_edit_file, exec_edit_file);

fn default_allow(_ctx: &ToolCallCtx) -> SafetyVerdict { SafetyVerdict::Allow }

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("edit_file", ""),
        description: "Find-and-replace in a file. Supports regex with regex=true, replace_all for all occurrences. Surgical edits only.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"old_string":{"type":"string","description":"Text to find"},"new_string":{"type":"string","description":"Replacement text"},"replace_all":{"type":"boolean","description":"Replace all occurrences","default":false},"regex":{"type":"boolean","description":"Treat old_string as regex","default":false},"reason":{"type":"string","description":"Why this change is needed (optional)"}},"required":["path","old_string","new_string"],"additionalProperties":false}),
        handler: handle_edit_file,
        safety: default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
