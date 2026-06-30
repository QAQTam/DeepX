use crate::{ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use super::file_shared::{disambiguate_match, apply_diff_and_format, normalize_newlines};

pub(super) fn exec_edit_file_diff(args: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v, Err(_) => return "[ERROR] Invalid JSON arguments".to_string(),
    };
    let path = match v.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "[ERROR] Missing required field: path".to_string(),
    };
    let old_lines: Vec<String> = v.get("old_lines").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    if old_lines.is_empty() { return "[ERROR] Missing required field: old_lines".to_string(); }
    if old_lines.len() > 100 { return format!("[ERROR] old_lines too large ({} lines, max 100)\n[HINT] Reduce the diff scope or use write_file for full rewrites.", old_lines.len()); }
    let new_lines: Vec<String> = v.get("new_lines").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let context_before: Vec<String> = v.get("context_before").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let context_after: Vec<String> = v.get("context_after").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let description = v.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let dry_run = v.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(true);

    let raw = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let err = e.to_string();
            if err.contains("UTF-8") || err.contains("utf-8") {
                return format!("[PARTIAL] {} — binary file\n[HINT] Use exec with hex dump tool.", path);
            }
            return format!("[ERROR] Cannot read {}: {}\n[HINT] Use list_dir() first.", path, e);
        }
    };
    // Normalize CRLF → LF so line matching works
    let (content, was_crlf) = normalize_newlines(&raw);
    if was_crlf {
        log::info!("file_edit_diff: {} had CRLF, normalized to LF for matching", path);
    }
    let file_lines: Vec<&str> = content.lines().collect();
    let norm_old: Vec<String> = old_lines.iter().map(|l| l.trim_end().to_string()).collect();
    let win = norm_old.len();
    if win > file_lines.len() {
        return format!("[ERROR] old_lines ({} lines) longer than file ({} lines)", win, file_lines.len());
    }

    // Phase 1: exact match
    let mut candidates: Vec<usize> = Vec::new();
    let mut was_fuzzy = false;
    for i in 0..=file_lines.len() - win {
        let window: Vec<String> = file_lines[i..i+win].iter().map(|l| l.trim_end().to_string()).collect();
        if window == norm_old { candidates.push(i); }
    }
    // Phase 2: fuzzy match
    if candidates.is_empty() {
        was_fuzzy = true;
        for i in 0..=file_lines.len() - win {
            let window: Vec<String> = file_lines[i..i+win].iter().map(|l| l.trim_end().to_string()).collect();
            if window.iter().zip(&norm_old).all(|(w, o)| w.trim() == o.trim()) {
                candidates.push(i);
            }
        }
    }
    if candidates.is_empty() {
        return format!("[PARTIAL] {} — old_lines not found\n[HINT] Verify current file content.", path);
    }

    // Disambiguate with context
    let match_idx = match disambiguate_match(&candidates, &context_before, &context_after, &file_lines, path, win) {
        Ok(idx) => idx,
        Err(msg) => return msg,
    };

    // Apply diff and format result
    apply_diff_and_format(path, &file_lines, match_idx, win, &new_lines, description, was_fuzzy, dry_run)
}

handler!(handle_edit_file_diff, exec_edit_file_diff);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "edit_diff"),
        description: "Fuzzy multi-line edit via old_lines+new_lines.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"old_lines":{"type":"array","items":{"type":"string"},"description":"Lines to remove"},"new_lines":{"type":"array","items":{"type":"string"},"description":"Lines to insert in place of old_lines"},"context_before":{"type":"array","items":{"type":"string"},"description":"Lines just before the change for disambiguation"},"context_after":{"type":"array","items":{"type":"string"},"description":"Lines just after the change for disambiguation"},"dry_run":{"type":"boolean","description":"Preview diff only, do not write file (default true)","default":true},"reason":{"type":"string","description":"Why this change is needed (optional)"}},"required":["path","old_lines","new_lines"],"additionalProperties":false}),
        handler: handle_edit_file_diff,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
