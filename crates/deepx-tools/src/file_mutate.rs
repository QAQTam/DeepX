//! Mutation tools: file write, edit, edit_diff, delete, move, copy.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolRisk, ToolCallCtx, ToolResult, handler};
use super::file_shared::{
    unified_diff, diff_stats, normalize_newlines, closest_line,
    disambiguate_match, apply_diff_and_format, is_binary_read_error,
};

// ── Shared helpers ──

fn format_diff_result(prefix: &str, path: &str, diff: &str, label: &str, _success: bool) -> String {
    let (added, removed, first_line) = diff_stats(diff);
    let summary = format!("[{prefix}] {path}:{first_line} +{added} -{removed} | {label}",
        added = added.max(1), removed = removed.max(1));
    // Always include the diff body — LLM context is truncated later in build_context_for_gate.
    // The frontend and audit trail need the full diff.
    format!("{}\n\n{}", summary, diff.trim_end())
}

// ── Helpers from file_edit ──

fn parse_paths(args: &str) -> Vec<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(arr) = v.get("paths").and_then(|a| a.as_array()) {
            let paths: Vec<String> = arr.iter().filter_map(|p| p.as_str().map(String::from)).collect();
            if !paths.is_empty() { return paths; }
        }
    }
    let path = parse_arg(args, "path");
    if path.is_empty() { vec![] } else { vec![path] }
}

fn parse_patterns(args: &str) -> Vec<(String, String)> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(arr) = v.get("patterns").and_then(|a| a.as_array()) {
            let patterns: Vec<(String, String)> = arr.iter().filter_map(|p| {
                let old = p.get("old").and_then(|o| o.as_str()).unwrap_or("");
                let new = p.get("new").and_then(|n| n.as_str()).unwrap_or("");
                if old.is_empty() { None } else { Some((old.to_string(), new.to_string())) }
            }).collect();
            if !patterns.is_empty() { return patterns; }
        }
    }
    let old = parse_arg(args, "old_string");
    let new = parse_arg(args, "new_string");
    if old.is_empty() { vec![] } else { vec![(old, new)] }
}

enum Match {
    Ok { msg: String },
    NoMatch { msg: String },
    Error { msg: String },
}

fn build_fuzzy_hint(content: &str, old: &str) -> String {
    if let Some((line_no, line)) = closest_line(content, old) {
        return format!(
            "\n[HINT] String not found exactly. Closest match at line {line_no}: \"{}\"\n       Retry with file_edit_diff start_line={} and old_lines set to the actual lines from the file.",
            line.chars().take(120).collect::<String>(),
            line_no
        );
    }
    "\n[HINT] String not found. Use file_read to check current file content, then retry.".to_string()
}

fn apply_one(content: &str, old: &str, new: &str, use_regex: bool, replace_all: bool, _path: &str) -> (String, Match) {
    if use_regex {
        let re = match regex::Regex::new(old) {
            Ok(r) => r,
            Err(e) => return (content.to_string(), Match::Error { msg: format!("Invalid regex: {e}") }),
        };
        let count = re.find_iter(content).count();
        if count == 0 {
            return (content.to_string(), Match::NoMatch { msg: format!("regex no matches") });
        }
        // Escape $ in replacement string — regex interprets $1, ${name} as
        // capture-group backreferences. The LLM intends literal replacement text,
        // so we double every $ ($$ → literal $).
        let escaped_new = new.replace('$', "$$");
        let new_content = if replace_all {
            re.replace_all(content, &escaped_new).to_string()
        } else {
            re.replacen(content, 1, &escaped_new).to_string()
        };
        let msg = if replace_all { format!("regex replaced {count} matches") } else { "regex replaced 1 match".into() };
        (new_content, Match::Ok { msg })
    } else if replace_all {
        if !content.contains(old) {
            let hint = build_fuzzy_hint(content, old);
            return (content.to_string(), Match::NoMatch { msg: format!("no occurrences{hint}") });
        }
        let count = content.matches(old).count();
        let new_content = content.replace(old, new);
        (new_content, Match::Ok { msg: format!("replaced {count} occurrences") })
    } else {
        match content.find(old) {
            Some(pos) => {
                let line = content[..pos].lines().count() + 1;
                let new_content = content.replacen(old, new, 1);
                (new_content, Match::Ok { msg: format!("line {line}: +{} -{}", new.len(), old.len()) })
            }
            None => {
                let hint = build_fuzzy_hint(content, old);
                (content.to_string(), Match::NoMatch { msg: format!("string not found{hint}") })
            }
        }
    }
}

// ── exec_write_file (from file_write.rs) ──

pub(super) fn exec_write_file(args: &str) -> String {
    let path = crate::resolve_workspace_path(&parse_arg(args, "path"));
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
                        format_diff_result("OK", &path, &diff, "write_file", true)
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

// ── exec_edit_file (from file_edit.rs) ──

pub(super) fn exec_edit_file(args: &str) -> String {
    let paths = parse_paths(args);
    if paths.is_empty() {
        return "[ERROR] edit_file: no path specified\n[HINT] Provide 'path' (single) or 'paths' (array).".into();
    }
    let patterns = parse_patterns(args);
    if patterns.is_empty() {
        return "[ERROR] edit_file: no patterns specified\n[HINT] Provide 'old_string'/'new_string' (single) or 'patterns' (array).".into();
    }
    let replace_all = parse_opt_bool(args, "replace_all").unwrap_or(false);
    let use_regex = parse_opt_bool(args, "regex").unwrap_or(false);
    let dry_run = parse_opt_bool(args, "dry_run").unwrap_or(false);

    let mut results = Vec::new();

    for path in &paths {
        let resolved = crate::resolve_workspace_path(path);
        let raw = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => {
                if is_binary_read_error(&e.to_string()) {
                    results.push(format!("[PARTIAL] {path} — binary file, edit_file works on text only"));
                } else {
                    results.push(format!("[ERROR] Cannot read {path}: {e}"));
                }
                continue;
            }
        };

        let (orig, was_crlf) = normalize_newlines(&raw);
        if was_crlf { log::info!("edit_file: {path} had CRLF, normalized to LF"); }

        let mut content = orig.clone();
        let mut msgs: Vec<String> = Vec::new();
        let mut all_matched = true;

        for (old_raw, new_raw) in &patterns {
            let old = old_raw.replace("\r\n", "\n").replace('\r', "\n");
            let new = new_raw.replace("\r\n", "\n").replace('\r', "\n");
            let (next, m) = apply_one(&content, &old, &new, use_regex, replace_all, path);
            match m {
                Match::Ok { msg } => { msgs.push(msg); content = next; }
                Match::NoMatch { msg } => { msgs.push(format!("[ ] {msg}")); all_matched = false; }
                Match::Error { msg } => { msgs.push(format!("[ERROR] {msg}")); all_matched = false; break; }
            }
        }

        if !all_matched {
            results.push(format!("[PARTIAL] {path} — some patterns did not match\n{}", msgs.join("\n")));
            continue;
        }

        if dry_run {
            let diff = unified_diff(&orig, &content, path);
            results.push(format_diff_result("DRY RUN", path, &diff, "edit_file", false));
        } else {
            // Restore CRLF if original file had Windows line endings
            let write_content = if was_crlf {
                content.replace('\n', "\r\n")
            } else {
                content.clone()
            };
            match std::fs::write(&resolved, &write_content) {
                Ok(_) => {
                    let diff = unified_diff(&orig, &content, path);
                    results.push(format_diff_result("OK", path, &diff, "edit_file", true));
                }
                Err(e) => {
                    results.push(format!("[ERROR] Cannot write {path}: {e}"));
                }
            }
        }
    }

    results.join("\n\n")
}

handler!(handle_edit_file, exec_edit_file);

// ── exec_delete_file (from file_delete.rs) ──

fn trash_dir() -> std::path::PathBuf {
    let dir = crate::workspace::deepx_dir().join("trash");
    let _ = std::fs::create_dir_all(&dir); // ensure exists
    dir
}

pub(super) fn exec_delete_file(args: &str) -> String {
    let path = crate::resolve_workspace_path(&parse_arg(args, "path"));
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return format!("[ERROR] {} does not exist.", path);
    }

    let trash_root = trash_dir();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    // Build a safe relative name: relative to workspace root, replace path separators
    let ws = crate::CURRENT_WORKSPACE.read().expect("CURRENT_WORKSPACE lock").clone();
    let project_root = if !ws.is_empty() && ws != "." { Path::new(&ws) } else { Path::new(".") };
    let rel = if let Ok(stripped) = p.strip_prefix(project_root) {
        stripped.to_string_lossy().to_string()
    } else if let Some(name) = p.file_name() {
        name.to_string_lossy().to_string()
    } else {
        path.replace(['/', '\\', ':'], "__")
    };
    let safe_name = rel.replace(['/', '\\', ':'], "__");
    let trash_path = trash_root.join(format!("{}.{}", safe_name, ts));

    if let Some(parent) = trash_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::rename(p, &trash_path) {
        Ok(_) => format!(
            "[OK] Moved to trash: .deepx/trash/{}\n[HINT] Restore with exec(\"mv {}\" \"{}\") or exec(\"ls .deepx/trash/\") to list trash.",
            trash_path.file_name().unwrap_or_default().to_string_lossy(),
            trash_path.display(), path
        ),
        Err(_e) => {
            // Cross-device rename fails — for files: copy+delete; for dirs: not supported
            if p.is_dir() {
                format!("[ERROR] Cannot trash directory across devices: {}\n[HINT] Use exec(\"rm -rf {}\") for cross-device deletion.", path, path)
            } else if let Err(e2) = std::fs::copy(p, &trash_path) {
                format!("[ERROR] Cannot trash {}: copy failed: {}\n[HINT] Check permissions and disk space.", path, e2)
            } else {
                match std::fs::remove_file(p) {
                    Ok(_) => format!(
                        "[OK] Moved to trash (cross-device): .deepx/trash/{}\n[HINT] Restore with exec(\"cp {}\" \"{}\").",
                        trash_path.file_name().unwrap_or_default().to_string_lossy(),
                        trash_path.display(), path
                    ),
                    Err(e2) => format!(
                        "[OK] Copied to trash but could not remove original: {}\n[HINT] The original file still exists at {}.", e2, path
                    ),
                }
            }
        }
    }
}

handler!(handle_delete_file, exec_delete_file);

// ── exec_move_file & exec_copy_file (from file_move.rs) ──

fn ensure_parent_dir(dest: &str) {
    if let Some(parent) = std::path::Path::new(dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

pub(super) fn exec_move_file(args: &str) -> String {
    let source = crate::resolve_workspace_path(&parse_arg(args, "source"));
    let dest = crate::resolve_workspace_path(&parse_arg(args, "dest"));
    ensure_parent_dir(&dest);
    match std::fs::rename(&source, &dest) {
        Ok(_) => format!("[OK] Moved {} → {}", source, dest),
        Err(e) => format!("[ERROR] Cannot move {}: {}\n[HINT] Check source exists and target directory is writable.", source, e),
    }
}

handler!(handle_move_file, exec_move_file);

pub(super) fn exec_copy_file(args: &str) -> String {
    let source = crate::resolve_workspace_path(&parse_arg(args, "source"));
    let dest = crate::resolve_workspace_path(&parse_arg(args, "dest"));
    ensure_parent_dir(&dest);
    match std::fs::copy(&source, &dest) {
        Ok(size) => format!("[OK] Copied {} → {} ({} bytes)", source, dest, size),
        Err(e) => format!("[ERROR] Cannot copy {}: {}\n[HINT] Check source exists and target directory is writable.", source, e),
    }
}

handler!(handle_copy_file, exec_copy_file);

// ── exec_edit_file_diff (from file_edit_diff.rs) ──

pub(super) fn exec_edit_file_diff(args: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v, Err(_) => return "[ERROR] Invalid JSON arguments".to_string(),
    };
    let path = crate::resolve_workspace_path(
        v.get("path").and_then(|v| v.as_str()).unwrap_or("")
    );
    if path.is_empty() { return "[ERROR] Missing required field: path".to_string(); }
    let old_lines: Vec<String> = v.get("old_lines").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let new_lines: Vec<String> = v.get("new_lines").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let context_before: Vec<String> = v.get("context_before").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let context_after: Vec<String> = v.get("context_after").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let description = v.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let dry_run = v.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(true);
    // Line-number addressing: when provided, bypass content matching entirely.
    let start_line: Option<usize> = v.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
    let end_line: Option<usize> = v.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);

    // Require either old_lines (content match) or start_line (line addressing)
    if old_lines.is_empty() && start_line.is_none() {
        return "[ERROR] Missing required field: old_lines or start_line".to_string();
    }
    if old_lines.len() > 100 && start_line.is_none() {
        return format!("[ERROR] old_lines too large ({} lines, max 100)\n[HINT] Reduce the diff scope or use write_file for full rewrites.", old_lines.len());
    }

    let raw = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if is_binary_read_error(&e.to_string()) {
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
    // ── Line-number fast path ──
    if let Some(start) = start_line {
        let s = start.saturating_sub(1); // 1-based → 0-based
        let e = end_line.map(|n| n.saturating_sub(1)).unwrap_or(s);
        if s >= file_lines.len() {
            return format!("[ERROR] start_line {start} past end of file ({} lines)", file_lines.len());
        }
        let e = e.min(file_lines.len().saturating_sub(1));
        if s > e {
            return format!("[ERROR] start_line {start} > end_line {}", end_line.unwrap_or(start));
        }
        let win = e - s + 1;

        // Optional validation: if old_lines provided, cross-check them
        if !old_lines.is_empty() {
            let actual: Vec<&str> = file_lines[s..=e].iter().map(|l| *l).collect();
            let norm_actual: Vec<String> = actual.iter().map(|l| l.trim_end().to_string()).collect();
            let norm_old: Vec<String> = old_lines.iter().map(|l| l.trim_end().to_string()).collect();
            if norm_actual != norm_old {
                let mut ctx = String::new();
                for (i, line) in actual.iter().enumerate() {
                    if i >= norm_old.len() || line.trim_end() != norm_old[i] {
                        ctx.push_str(&format!("  L{} actual: {}\n", s + i + 1, line));
                        if i < norm_old.len() {
                            ctx.push_str(&format!("  L{} old_lines: {}\n", s + i + 1, norm_old[i]));
                        }
                    }
                }
                log::warn!("file_edit_diff: {} line-number mismatch at L{}:\n{}", path, start, ctx);
            }
        }

        return apply_diff_and_format(&path, &file_lines, s, win, &new_lines, description, false, dry_run, was_crlf);
    }

    // ── Content matching (original path) ──
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
        // Show best-match hint using closest_line on the first line of old_lines
        let first_old = old_lines.first().map(|l| l.trim()).unwrap_or("");
        if let Some((line_num, line_text)) = closest_line(&content, first_old) {
            return format!(
                "[PARTIAL] {} — old_lines not found.\n\
                 Closest match at line {line_num}: {}\n\
                 [HINT] Use file_read first, then retry with start_line={} or corrected old_lines.",
                path,
                line_text,
                line_num
            );
        }
        return format!("[PARTIAL] {} — old_lines not found\n[HINT] Verify current file content or use start_line/end_line for line-number editing.", path);
    }

    // Disambiguate with context
    let match_idx = match disambiguate_match(&candidates, &context_before, &context_after, &file_lines, &path, win) {
        Ok(idx) => idx,
        Err(msg) => return msg,
    };

    // Apply diff and format result
    apply_diff_and_format(&path, &file_lines, match_idx, win, &new_lines, description, was_fuzzy, dry_run, was_crlf)
}

handler!(handle_edit_file_diff, exec_edit_file_diff);

// ── Registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "write"),
        description: "Create, overwrite, or append to a file.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"If true, append to file instead of overwriting","default":false}},"required":["path","content"],"additionalProperties":false}),
        handler: handle_write_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "edit"),
        description: "String replacement in files.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"paths":{"type":"array","items":{"type":"string"},"description":"Multiple file paths"},"old_string":{"type":"string","description":"Text to find"},"new_string":{"type":"string","description":"Replacement text"},"patterns":{"type":"array","items":{"type":"object","properties":{"old":{"type":"string"},"new":{"type":"string"}},"required":["old","new"]},"description":"Array of {old, new} for batch edits"},"replace_all":{"type":"boolean","description":"Replace all occurrences","default":false},"regex":{"type":"boolean","description":"Treat old_string as regex","default":false},"dry_run":{"type":"boolean","description":"Preview diff only, do not write file","default":false}},"required":["path","old_string","new_string"],"additionalProperties":false}),
        handler: handle_edit_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(60),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "edit_diff"),
        description: "Fuzzy multi-line edit via old_lines+new_lines. Supports line-number addressing with start_line/end_line.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"old_lines":{"type":"array","items":{"type":"string"},"description":"Lines to remove (not needed when start_line set)"},"new_lines":{"type":"array","items":{"type":"string"},"description":"Lines to insert in place of old_lines"},"context_before":{"type":"array","items":{"type":"string"},"description":"Lines just before the change for disambiguation"},"context_after":{"type":"array","items":{"type":"string"},"description":"Lines just after the change for disambiguation"},"start_line":{"type":"integer","description":"1-based line number to start replacement at (bypasses content matching)"},"end_line":{"type":"integer","description":"1-based line number to end replacement at (inclusive, defaults to start_line)"},"dry_run":{"type":"boolean","description":"Preview diff only, do not write file (default true)","default":true},"description":{"type":"string","description":"Why this change is needed (optional)"}},"required":["path","new_lines"],"additionalProperties":false}),
        handler: handle_edit_file_diff,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "delete"),
        description: "Move file to trash (.deepx/trash/) instead of permanent deletion.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path to delete"}},"required":["path"],"additionalProperties":false}),
        handler: handle_delete_file,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "move"),
        description: "Move or rename a file or directory. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_move_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "copy"),
        description: "Copy a file. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_copy_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
