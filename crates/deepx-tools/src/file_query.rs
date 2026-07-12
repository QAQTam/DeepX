//! Query tools: file read, list_dir, search, diff.

use std::process::Command;

use crate::{ToolHandler, ToolRisk, ToolCallCtx, ToolResult, handler, JsonArgs};
use super::file_shared::{rust_grep, unified_diff, is_binary_read_error};

// ------ exec_read_file (from file_read.rs) ------

pub(super) fn exec_read_file(args: &serde_json::Value) -> String {
    // ------ Batch mode: read multiple files ------
    if let Some(paths) = args.get("paths").and_then(|v| v.as_array()) {
        let mut results = Vec::new();
        for p in paths {
            if let Some(pstr) = p.as_str() {
                let mut per = serde_json::json!({"path": pstr});
                if let Some(s) = args.get("start_line") { per["start_line"] = s.clone(); }
                if let Some(e) = args.get("end_line") { per["end_line"] = e.clone(); }
                results.push(exec_read_file(&per));
            }
        }
        return format!("[{} files]\n\n{}", paths.len(), results.join("\n\n---\n\n"));
    }

    // ------ Single file mode ------
    let path = crate::resolve_workspace_path(&args.s("path"));
    let start: Option<usize> = args.get("start_line").and_then(|v| v.as_u64()).map(|n| (n as usize).max(1));
    let end: Option<usize> = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);

    const MAX_READ_LINES: usize = 300;
    if let (Some(s), Some(e)) = (start, end) {
        if e > s && e - s > MAX_READ_LINES {
            return serde_json::json!({
                "timeis": crate::now_utc8(),
                "status": "error",
                "path": path,
                "code": "RANGE_TOO_LARGE",
                "message": format!("Requested range too large ({} lines > {} max)", e - s, MAX_READ_LINES),
                "hint": "Use smaller range."
            }).to_string();
        }
    }
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            let content = raw.replace("\r\n", "\n").replace('\r', "\n");

            // ------ Cache check: return "unchanged" if content matches previous read ------
            if start.is_none() && end.is_none() {
                if let Some(cached) = crate::file_cache::check(&path, &content) {
                    return cached;
                }
            }

            let all_lines: Vec<&str> = content.lines().collect();
            let total = all_lines.len();
            let start_idx = start.map(|s| (s - 1).min(total)).unwrap_or(0);
            let end_idx = end.map(|e| e.min(total)).unwrap_or(total);
            let start_idx = start_idx.min(end_idx);
            let shown = end_idx - start_idx;
            let truncated = start.is_some() || end.is_some() || total > 200;

            let body: String = if total <= 200 && start.is_none() && end.is_none() {
                // Small file, full output ---no line numbers in body
                all_lines.join("\n")
            } else if start.is_some() || end.is_some() {
                // Range read
                all_lines[start_idx..end_idx].join("\n")
            } else {
                // Large file: head + tail, no line numbers
                let head = 50.min(total);
                let tail = 30.min(total - head);
                let mut s = all_lines[..head].join("\n");
                if total > head + tail {
                    s.push_str(&format!(
                        "\n--?[{} lines omitted --?use start_line to read specific range]",
                        total - head - tail
                    ));
                }
                if tail > 0 {
                    s.push('\n');
                    s.push_str(&all_lines[total - tail..].join("\n"));
                }
                s
            };

            // ------ Cache: store full-file reads for future deduplication ------
            if start.is_none() && end.is_none() {
                crate::file_state::record_read(&path, &content, total);
            }

            serde_json::json!({
                "timeis": crate::now_utc8(),
                "status": "ok",
                "path": path,
                "start_line": start_idx + 1,
                "end_line": start_idx + shown,
                "total_lines": total,
                "truncated": truncated,
                "content": body,
            }).to_string()
        }
        Err(e) => {
            if is_binary_read_error(&e.to_string()) {
                let meta = std::fs::metadata(&path);
                let size = meta.as_ref().map(|m| format!("{}", m.len())).unwrap_or_default();
                serde_json::json!({
                    "timeis": crate::now_utc8(),
                    "status": "error",
                    "path": path,
                    "code": "BINARY_FILE",
                    "message": format!("Binary file ({}B), cannot display as text", size),
                    "hint": format!("Use exec(\"file '{}'\") to identify format, or exec(\"xxd '{}'\") for hex dump.", path, path),
                }).to_string()
            } else {
                let url_hint = if path.contains("://") || path.contains(".com") || path.contains("www.") {
                    "\n[HINT] This looks like a URL --?did you mean to call web_fetch() instead?"
                } else { "" };
                serde_json::json!({
                    "timeis": crate::now_utc8(),
                    "status": "error",
                    "path": path,
                    "code": "NOT_FOUND",
                    "message": e.to_string(),
                    "hint": format!("Use list_dir() on the parent directory to verify the file exists.{url_hint}"),
                }).to_string()
            }
        },
    }
}

handler!(handle_read_file, exec_read_file);

// ------ exec_list_dir (from file_list_dir.rs) ------

pub(super) fn exec_list_dir(args: &serde_json::Value) -> String {
    let path = crate::resolve_workspace_path(&args.s_or("path", "."));
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            const MAX_LIST_DIR_ENTRIES: usize = 200;
            let mut content = String::from("Directory listing: ");
            content.push_str(&path);
            content.push('\n');
            let mut count = 0usize;
            let all: Vec<_> = entries.flatten().collect();
            let total = all.len();
            for entry in &all {
                if count >= MAX_LIST_DIR_ENTRIES { break; }
                count += 1;
                let ft = entry.file_type().map(|t| if t.is_dir() { "/" } else { "" }).unwrap_or("?");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let name = entry.file_name();
                let name_s = name.to_string_lossy();
                let hidden = name_s.starts_with('.');
                if ft == "/" {
                    let tag = if hidden { " <DIR> (hidden)" } else { " <DIR>" };
                    content.push_str(&format!("  {:<40}{}\n", name_s + "/", tag));
                } else {
                    let sz = if size > 1024*1024 { format!("{:.1}M", size as f64 / 1_048_576.0) }
                        else if size > 1024 { format!("{}K", size / 1024) }
                        else { format!("{}B", size) };
                    let tag = if hidden { " (hidden)" } else { "" };
                    content.push_str(&format!("  {:<40} {:>6}{}\n", name_s, sz, tag));
                }
            }
            if total > MAX_LIST_DIR_ENTRIES {
                content.push_str(&format!("... [truncated: {} more entries]\n", total - MAX_LIST_DIR_ENTRIES));
            }
            crate::json_ok(serde_json::json!({"path": path, "content": content}))
        }
        Err(e) => crate::json_err("LIST_FAILED", &format!("Cannot list {}: {}", path, e), "Check if the directory exists and is readable."),
    }
}

handler!(handle_list_dir, exec_list_dir);

// ------ exec_search (from file_search.rs) ------

pub(super) fn exec_search(args: &serde_json::Value) -> String {
    let pattern = args.s("pattern");
    let glob = args.get("glob").and_then(|v| v.as_str()).map(String::from);
    let dir = crate::resolve_workspace_path(&args.s_or("path", "."));

    // Phase 1: try ripgrep (cross-platform, fast)
    let mut cmd = Command::new("rg");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.arg("-n").arg("--no-heading");
    if let Some(ref g) = glob {
        cmd.arg("-g").arg(g);
    }
    cmd.arg(&pattern).arg(&dir);

    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            let all_lines: Vec<&str> = out.lines().collect();
            let lines: Vec<&str> = all_lines.iter().take(100).copied().collect();
            if lines.is_empty() {
                return crate::json_ok(serde_json::json!({"pattern": pattern, "content": format!("No matches for '{}'", pattern)}));
            }
            let truncated = if all_lines.len() > 100 {
                format!("\n... ({} more matches)", all_lines.len() - 100)
            } else {
                String::new()
            };
            return crate::json_ok(serde_json::json!({"pattern": pattern, "matches": all_lines.len(), "content": format!("{}", lines.join("\n")) + &truncated}));
        }
        _ => {} // rg not installed or errored --?fall through to pure Rust
    }

    // Phase 2: pure Rust fallback
    match rust_grep(&pattern, &dir, true, true, glob.as_deref(), 100) {
        Ok(lines) => {
            if lines.is_empty() {
                crate::json_ok(serde_json::json!({"pattern": pattern, "content": format!("No matches for '{}'", pattern)}))
            } else {
                let result: Vec<&str> = lines.iter().take(100).map(|s| s.as_str()).collect();
                let truncated = if lines.len() > 100 {
                    format!("\n... ({} more matches)", lines.len() - 100)
                } else {
                    String::new()
                };
                crate::json_ok(serde_json::json!({"pattern": pattern, "matches": lines.len(), "content": format!("{}", result.join("\n")) + &truncated}))
            }
        }
        Err(e) => crate::json_err("SEARCH_FAILED", &format!("search failed: {}", e), "Check the pattern or path."),
    }
}

handler!(handle_search, exec_search);

// ------ exec_diff (from file_diff.rs) ------

pub(super) fn exec_diff(args: &serde_json::Value) -> String {
    let path_a = crate::resolve_workspace_path(&args.s("path_a"));
    let path_b = crate::resolve_workspace_path(&args.s("path_b"));

    let content_a = match std::fs::read_to_string(&path_a) {
        Ok(c) => c,
        Err(e) => return crate::json_err("READ_FAILED", &format!("Cannot read {}: {}", path_a, e), "Verify the file exists. Use list_dir() to check."),
    };
    let content_b = match std::fs::read_to_string(&path_b) {
        Ok(c) => c,
        Err(e) => return crate::json_err("READ_FAILED", &format!("Cannot read {}: {}", path_b, e), "Verify the file exists. Use list_dir() to check."),
    };

    if content_a == content_b {
        return crate::json_ok(serde_json::json!({"path_a": path_a, "path_b": path_b, "identical": true, "content": "Files are identical"}));
    }

    crate::json_ok(serde_json::json!({"path_a": path_a, "path_b": path_b, "identical": false, "content": unified_diff(&content_a, &content_b, &path_a)}))
}

handler!(handle_diff, exec_diff);

// ------ Registration ------

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: "read".to_string(),
        description: "Read file contents. Fails on directories --?use file_list for that. Use start_line/end_line for range reads. Full files auto-truncate to head 50 + tail 30 lines (>200 lines). Use 'paths' array to read multiple files in one call. Returns JSON with path, total_lines, truncated flag, and content.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path, NOT a directory (use file_list for directories). Relative to workspace or absolute."},"start_line":{"type":"integer","description":"First line to read (1-based, optional)"},"end_line":{"type":"integer","description":"Last line to read, inclusive (optional). Max range: 300 lines."}},"required":["path"],"additionalProperties":false}),
        handler: handle_read_file,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "list".to_string(),
        description: "List directory contents with names and sizes.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Directory path","default":"."}},"additionalProperties":false}),
        handler: handle_list_dir,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "search".to_string(),
        description: "Regex search across files. Returns file:line matches.",
        input_schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Regex pattern"},"glob":{"type":"string","description":"File glob filter (e.g. *.rs)"},"path":{"type":"string","description":"Search directory","default":"."}},"required":["pattern"],"additionalProperties":false}),
        handler: handle_search,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: "diff".to_string(),
        description: "Compare two files line by line.",
        input_schema: serde_json::json!({"type":"object","properties":{"path_a":{"type":"string","description":"First file path"},"path_b":{"type":"string","description":"Second file path"}},"required":["path_a","path_b"],"additionalProperties":false}),
        handler: handle_diff,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
