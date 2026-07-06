//! Query tools: file read, list_dir, search, diff.

use std::process::Command;

use crate::{parse_arg, parse_opt, parse_arg_or, ToolHandler, ToolKey, ToolRisk, ToolCallCtx, ToolResult, handler};
use super::file_shared::{rust_grep, unified_diff, is_binary_read_error};

// ── exec_read_file (from file_read.rs) ──

pub(super) fn exec_read_file(args: &str) -> String {
    let path = crate::resolve_workspace_path(&parse_arg(args, "path"));
    let start: Option<usize> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("start_line")?.as_u64().map(|n| (n as usize).max(1)));
    let end: Option<usize> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("end_line")?.as_u64().map(|n| n as usize));

    const MAX_READ_LINES: usize = 300;
    if let (Some(s), Some(e)) = (start, end) {
        if e > s && e - s > MAX_READ_LINES {
            return format!("[ERROR] Requested range too large ({} lines > {} max). Use smaller range.", e - s, MAX_READ_LINES);
        }
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            // Normalize CRLF → LF
            let content = content.replace("\r\n", "\n").replace('\r', "\n");
            let all_lines: Vec<&str> = content.lines().collect();
            let total = all_lines.len();
            let start_idx = start.map(|s| (s - 1).min(total)).unwrap_or(0);
            let end_idx = end.map(|e| e.min(total)).unwrap_or(total);
            let start_idx = start_idx.min(end_idx);
            let lines: Vec<&str> = all_lines[start_idx..end_idx].to_vec();
            let shown = lines.len();

            if start.is_some() || end.is_some() {
                let display = crate::display_path(&path);
                let mut result = format!("[OK] {} lines {}-{}/{} of {}\n", shown, start_idx + 1, end_idx, total, display);
                for (i, l) in lines.iter().enumerate() {
                    result.push_str(&format!("{:>4} {}\n", start_idx + i + 1, l));
                }
                result
            } else if total <= 200 {
                // Full output for files ≤200 lines (avoids AI re-read)
                let display = crate::display_path(&path);
                let mut result = format!("[OK] {} lines total ({})\n", total, display);
                for (i, l) in all_lines.iter().enumerate() {
                    result.push_str(&format!("{:>4} {}\n", i + 1, l));
                }
                result
            } else {
                // Head + tail for larger files with anchor index
                let head = 50.min(total);
                let tail = 30.min(total - head);
                let mut result = format!("[PARTIAL] {} — {} lines, showing first {} + last {}\n", path, total, head, tail);
                for (i, l) in all_lines.iter().take(head).enumerate() {
                    result.push_str(&format!("{:>4} {}\n", i + 1, l));
                }
                if total > head + tail {
                    result.push_str(&format!("  ⋮  [{} lines omitted — use start_line to read specific range]\n", total - head - tail));
                }
                for (i, l) in all_lines.iter().rev().take(tail).collect::<Vec<_>>().iter().rev().enumerate() {
                    result.push_str(&format!("{:>4} {}\n", total - tail + i + 1, l));
                }
                result
            }
        }
        Err(e) => {
            if is_binary_read_error(&e.to_string()) {
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

// ── exec_list_dir (from file_list_dir.rs) ──

pub(super) fn exec_list_dir(args: &str) -> String {
    let path = crate::resolve_workspace_path(&parse_arg_or(args, "path", "."));
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            const MAX_LIST_DIR_ENTRIES: usize = 200;
            let mut result = String::from("[OK] Directory listing: ");
            result.push_str(&path);
            result.push('\n');
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
                    result.push_str(&format!("  {:<40}{}\n", name_s + "/", tag));
                } else {
                    let sz = if size > 1024*1024 { format!("{:.1}M", size as f64 / 1_048_576.0) }
                        else if size > 1024 { format!("{}K", size / 1024) }
                        else { format!("{}B", size) };
                    let tag = if hidden { " (hidden)" } else { "" };
                    result.push_str(&format!("  {:<40} {:>6}{}\n", name_s, sz, tag));
                }
            }
            if total > MAX_LIST_DIR_ENTRIES {
                result.push_str(&format!("... [truncated: {} more entries]\n", total - MAX_LIST_DIR_ENTRIES));
            }
            result
        }
        Err(e) => format!("[ERROR] Cannot list {}: {}\n[HINT] Check if the directory exists and is readable.", path, e),
    }
}

handler!(handle_list_dir, exec_list_dir);

// ── exec_search (from file_search.rs) ──

pub(super) fn exec_search(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let glob = parse_opt(args, "glob");
    let dir = crate::resolve_workspace_path(&parse_arg_or(args, "path", "."));

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
                return format!("[OK] No matches for '{}'", pattern);
            }
            let truncated = if all_lines.len() > 100 {
                format!("\n... ({} more matches)", all_lines.len() - 100)
            } else {
                String::new()
            };
            return format!("[OK] {}", lines.join("\n")) + &truncated;
        }
        _ => {} // rg not installed or errored — fall through to pure Rust
    }

    // Phase 2: pure Rust fallback
    match rust_grep(&pattern, &dir, true, true, glob.as_deref(), 100) {
        Ok(lines) => {
            if lines.is_empty() {
                format!("[OK] No matches for '{}'", pattern)
            } else {
                let result: Vec<&str> = lines.iter().take(100).map(|s| s.as_str()).collect();
                let truncated = if lines.len() > 100 {
                    format!("\n... ({} more matches)", lines.len() - 100)
                } else {
                    String::new()
                };
                format!("[OK] {}", result.join("\n")) + &truncated
            }
        }
        Err(e) => format!("[ERROR] search failed: {}\n[HINT] Check the pattern or path.", e),
    }
}

handler!(handle_search, exec_search);

// ── exec_diff (from file_diff.rs) ──

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

// ── Registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "read"),
        description: "File operations: read, write, edit, search, list, move, copy, delete, diff.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"start_line":{"type":"integer","description":"First line to read (1-based)","default":1},"end_line":{"type":"integer","description":"Last line to read (inclusive). If omitted, reads to end of file."}},"required":["path"],"additionalProperties":false}),
        handler: handle_read_file,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "list"),
        description: "List directory contents with names and sizes.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Directory path","default":"."}},"additionalProperties":false}),
        handler: handle_list_dir,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "search"),
        description: "Regex search across files. Returns file:line matches.",
        input_schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Regex pattern"},"glob":{"type":"string","description":"File glob filter (e.g. *.rs)"},"path":{"type":"string","description":"Search directory","default":"."}},"required":["pattern"],"additionalProperties":false}),
        handler: handle_search,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "diff"),
        description: "Compare two files line by line.",
        input_schema: serde_json::json!({"type":"object","properties":{"path_a":{"type":"string","description":"First file path"},"path_b":{"type":"string","description":"Second file path"}},"required":["path_a","path_b"],"additionalProperties":false}),
        handler: handle_diff,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
