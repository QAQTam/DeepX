//! Mutation tools: file write, edit, edit_diff, delete, move, copy.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{ToolHandler, ToolRisk, ToolCallCtx, ToolResult, handler, JsonArgs};
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

fn parse_paths(args: &serde_json::Value) -> Vec<String> {
    if let Some(arr) = args.get("paths").and_then(|a| a.as_array()) {
        let paths: Vec<String> = arr.iter().filter_map(|p| p.as_str().map(String::from)).collect();
        if !paths.is_empty() { return paths; }
    }
    let path = args.s("path");
    if path.is_empty() { vec![] } else { vec![path.to_string()] }
}

fn parse_patterns(args: &serde_json::Value) -> Vec<(String, String)> {
    if let Some(arr) = args.get("patterns").and_then(|a| a.as_array()) {
        let patterns: Vec<(String, String)> = arr.iter().filter_map(|p| {
            let old = p.get("old").and_then(|o| o.as_str()).unwrap_or("");
            let new = p.get("new").and_then(|n| n.as_str()).unwrap_or("");
            if old.is_empty() { None } else { Some((old.to_string(), new.to_string())) }
        }).collect();
        if !patterns.is_empty() { return patterns; }
    }
    let old = args.s("old_string");
    let new = args.s("new_string");
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
        let escaped_new = new.replace('$', "$$");
        let new_content = if replace_all {
            re.replace_all(content, &escaped_new).to_string()
        } else {
            re.replacen(content, 1, &escaped_new).to_string()
        };
        let msg = if replace_all { format!("regex replaced {count} matches") } else { "regex replaced 1 match".into() };
        (new_content, Match::Ok { msg })
    } else if replace_all {
        // Exact match first; fallback to trim_end for trailing-whitespace tolerance.
        let matcher: &str = if content.contains(old) {
            old
        } else {
            let trimmed = old.trim_end();
            if trimmed != old && content.contains(trimmed) {
                trimmed
            } else {
                let hint = build_fuzzy_hint(content, old);
                return (content.to_string(), Match::NoMatch { msg: format!("no occurrences{hint}") });
            }
        };
        let count = content.matches(matcher).count();
        let new_content = content.replace(matcher, new);
        (new_content, Match::Ok { msg: format!("replaced {count} occurrences") })
    } else {
        match content.find(old) {
            Some(pos) => {
                let line = content[..pos].lines().count() + 1;
                let new_content = content.replacen(old, new, 1);
                (new_content, Match::Ok { msg: format!("line {line}: +{} -{}", new.len(), old.len()) })
            }
            None => {
                let trimmed = old.trim_end();
                if trimmed != old {
                    match content.find(trimmed) {
                        Some(pos) => {
                            let line = content[..pos].lines().count() + 1;
                            let new_content = content.replacen(trimmed, new, 1);
                            (new_content, Match::Ok { msg: format!("line {line} [trim-end match]: +{} -{}", new.len(), trimmed.len()) })
                        }
                        None => {
                            let hint = build_fuzzy_hint(content, old);
                            (content.to_string(), Match::NoMatch { msg: format!("string not found{hint}") })
                        }
                    }
                } else {
                    let hint = build_fuzzy_hint(content, old);
                    (content.to_string(), Match::NoMatch { msg: format!("string not found{hint}") })
                }
            }
        }
    }
}

// ── exec_write_file (from file_write.rs) ──

pub(super) fn exec_write_file(args: &serde_json::Value) -> String {
    let path = crate::resolve_workspace_path(&args.s("path"));
    let content = args.s("content");
    let append = args.opt_bool("append").unwrap_or(false);
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
                crate::file_state::record_write(&path, line_count);
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
                crate::file_state::record_write(&path, line_count);
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

pub(super) fn exec_edit_file(args: &serde_json::Value) -> String {
    let paths = parse_paths(args);
    if paths.is_empty() {
        return "[ERROR] edit_file: no path specified\n[HINT] Provide 'path' (single) or 'paths' (array).".into();
    }
    let patterns = parse_patterns(args);
    if patterns.is_empty() {
        return "[ERROR] edit_file: no patterns specified\n[HINT] Provide 'old_string'/'new_string' (single) or 'patterns' (array).".into();
    }
    let replace_all = args.opt_bool("replace_all").unwrap_or(false);
    let use_regex = args.opt_bool("regex").unwrap_or(false);
    let dry_run = args.opt_bool("dry_run").unwrap_or(false);

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
                    crate::file_state::record_edit(path, 0);
                    let diff = unified_diff(&orig, &content, path);
                    results.push(format_diff_result("OK", path, &diff, "edit_file", true));
                }
                Err(e) => {
                    results.push(format!("[ERROR] Cannot write {path}: {e}"));
                }
            }
        }
    }

    // Multi-file summary
    if paths.len() > 1 {
        let ok = results.iter().filter(|r| r.starts_with("[OK]") || r.starts_with("[DRY RUN]")).count();
        let err = results.iter().filter(|r| r.starts_with("[ERROR]") || r.starts_with("[PARTIAL]")).count();
        results.push(format!("[SUMMARY] {}/{} files ok, {} failed", ok, paths.len(), err));
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

pub(super) fn exec_delete_file(args: &serde_json::Value) -> String {
    let path = crate::resolve_workspace_path(&args.s("path"));
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "error",
            "code": "NOT_FOUND",
            "path": path,
            "message": format!("{} does not exist", path),
            "hint": "Use list_dir() to verify."
        }).to_string();
    }

    let trash_root = trash_dir();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
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
        Ok(_) => {
            crate::file_state::record_delete(&path);
            serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "ok",
            "path": path,
            "trash_path": format!(".deepx/trash/{}", trash_path.file_name().unwrap_or_default().to_string_lossy()),
            "content": format!("Moved to trash: .deepx/trash/{}", trash_path.file_name().unwrap_or_default().to_string_lossy()),
            "hint": format!("Restore with exec(\"mv {} {}\")", trash_path.display(), path),
        }).to_string()
        }
        Err(_e) => {
            if p.is_dir() {
                serde_json::json!({
                    "timeis": crate::now_utc8(),
                    "status": "error",
                    "code": "CROSS_DEVICE_DIR",
                    "path": path,
                    "message": "Cannot trash directory across devices",
                    "hint": format!("Use exec(\"rm -rf {}\") for cross-device deletion.", path),
                }).to_string()
            } else if let Err(e2) = std::fs::copy(p, &trash_path) {
                serde_json::json!({
                    "timeis": crate::now_utc8(),
                    "status": "error",
                    "code": "COPY_FAILED",
                    "path": path,
                    "message": e2.to_string(),
                    "hint": "Check permissions and disk space."
                }).to_string()
            } else {
                match std::fs::remove_file(p) {
                    Ok(_) => {
                        crate::file_state::record_delete(&path);
                        serde_json::json!({
                        "timeis": crate::now_utc8(),
                        "status": "ok",
                        "path": path,
                        "trash_path": format!(".deepx/trash/{}", trash_path.file_name().unwrap_or_default().to_string_lossy()),
                        "content": format!("Moved to trash (cross-device): .deepx/trash/{}", trash_path.file_name().unwrap_or_default().to_string_lossy()),
                        "hint": format!("Restore with exec(\"cp {} {}\")", trash_path.display(), path),
                }).to_string()
                }
                Err(e2) => serde_json::json!({
                    "timeis": crate::now_utc8(),
                    "status": "ok",
                    "path": path,
                    "warning": format!("Copied to trash but could not remove original: {}", e2),
                    "content": format!("Copied to trash, original still at {}", path),
                }).to_string(),
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

pub(super) fn exec_move_file(args: &serde_json::Value) -> String {
    let source = crate::resolve_workspace_path(&args.s("source"));
    let dest = crate::resolve_workspace_path(&args.s("dest"));
    ensure_parent_dir(&dest);
    match std::fs::rename(&source, &dest) {
        Ok(_) => {
            crate::file_state::record_move(&source, &dest);
            serde_json::json!({
                "timeis": crate::now_utc8(),
                "status": "ok",
                "source": source,
                "dest": dest,
                "content": format!("Moved {source} → {dest}"),
            }).to_string()
        }
        Err(e) => serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "error",
            "code": "MOVE_FAILED",
            "source": source,
            "dest": dest,
            "message": e.to_string(),
            "hint": "Check source exists and target directory is writable."
        }).to_string(),
    }
}

handler!(handle_move_file, exec_move_file);

pub(super) fn exec_copy_file(args: &serde_json::Value) -> String {
    let source = crate::resolve_workspace_path(&args.s("source"));
    let dest = crate::resolve_workspace_path(&args.s("dest"));
    ensure_parent_dir(&dest);
    match std::fs::copy(&source, &dest) {
        Ok(size) => serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "ok",
            "source": source,
            "dest": dest,
            "bytes": size,
            "content": format!("Copied {source} → {dest} ({size} bytes)"),
        }).to_string(),
        Err(e) => serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "error",
            "code": "COPY_FAILED",
            "source": source,
            "dest": dest,
            "message": e.to_string(),
            "hint": "Check source exists and target directory is writable."
        }).to_string(),
    }
}

handler!(handle_copy_file, exec_copy_file);

// ── exec_edit_file_diff (from file_edit_diff.rs) ──

pub(super) fn exec_edit_file_diff(args: &serde_json::Value) -> String {
    let path = crate::resolve_workspace_path(
        args.get("path").and_then(|v| v.as_str()).unwrap_or("")
    );
    if path.is_empty() {
        return serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "error",
            "code": "MISSING_PATH",
            "message": "path is required",
            "hint": "Provide a file path to edit."
        }).to_string();
    }
    let old_lines: Vec<String> = args.get("old_lines").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let new_lines: Vec<String> = args.get("new_lines").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let context_before: Vec<String> = args.get("context_before").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let context_after: Vec<String> = args.get("context_after").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(true);
    let start_line: Option<usize> = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
    let end_line: Option<usize> = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);

    let err = |code: &str, msg: &str, hint: &str| -> String {
        serde_json::json!({
            "timeis": crate::now_utc8(),
            "status": "error",
            "code": code,
            "path": path,
            "message": msg,
            "hint": hint,
        }).to_string()
    };

    if old_lines.is_empty() && start_line.is_none() {
        return err("MISSING_PARAM", "old_lines or start_line is required",
            "Provide old_lines for content matching or start_line for line-number editing.");
    }
    if old_lines.len() > 100 && start_line.is_none() {
        return err("TOO_LARGE",
            &format!("old_lines too large ({} lines, max 100)", old_lines.len()),
            "Reduce the diff scope, use file_write for full rewrites, or set start_line (no old_lines needed) for line-range replacement.");
    }

    let raw = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if is_binary_read_error(&e.to_string()) {
                return serde_json::json!({
                    "timeis": crate::now_utc8(),
                    "status": "error",
                    "code": "BINARY_FILE",
                    "path": path,
                    "message": "Binary file, cannot display as text",
                    "hint": "Use exec with hex dump tool."
                }).to_string();
            }
            return err("READ_FAILED", &e.to_string(), "Use list_dir() first.");
        }
    };
    let (content, was_crlf) = normalize_newlines(&raw);
    if was_crlf {
        log::info!("file_edit_diff: {} had CRLF, normalized to LF for matching", path);
    }
    let file_lines: Vec<&str> = content.lines().collect();

    if let Some(start) = start_line {
        let s = start.saturating_sub(1);
        let e = end_line.map(|n| n.saturating_sub(1)).unwrap_or(s);
        if s >= file_lines.len() {
            return err("LINE_OUT_OF_RANGE",
                &format!("start_line {start} past end of file ({} lines)", file_lines.len()),
                "Use file_read to check the file length.");
        }
        let e = e.min(file_lines.len().saturating_sub(1));
        if s > e {
            return err("LINE_RANGE_INVALID",
                &format!("start_line {start} > end_line {}", end_line.unwrap_or(start)),
                "end_line must be >= start_line.");
        }
        let win = e - s + 1;
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
                return crate::json_err(
                    "LINE_MISMATCH",
                    &format!("start_line={start}: old_lines do not match actual file content at lines {}-{}", s + 1, e + 1),
                    &format!("Mismatch:\n{ctx}File content has changed. Use file_read to re-read and retry with corrected old_lines.")
                );
            }
        }
        return apply_diff_and_format(&path, &file_lines, s, win, &new_lines, description, false, dry_run, was_crlf);
    }

    let norm_old: Vec<String> = old_lines.iter().map(|l| l.trim_end().to_string()).collect();
    let win = norm_old.len();
    if win > file_lines.len() {
        return err("TOO_LARGE",
            &format!("old_lines ({} lines) longer than file ({} lines)", win, file_lines.len()),
            "Check the file content with file_read first.");
    }

    let mut candidates: Vec<usize> = Vec::new();
    let mut was_fuzzy = false;
    for i in 0..=file_lines.len() - win {
        let window: Vec<String> = file_lines[i..i+win].iter().map(|l| l.trim_end().to_string()).collect();
        if window == norm_old { candidates.push(i); }
    }
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
        let first_old = old_lines.first().map(|l| l.trim()).unwrap_or("");
        if let Some((line_num, line_text)) = closest_line(&content, first_old) {
            return serde_json::json!({
                "timeis": crate::now_utc8(),
                "status": "error",
                "code": "NO_MATCH",
                "path": path,
                "message": "old_lines not found",
                "closest_line": line_num,
                "closest_text": line_text,
                "hint": format!("Use file_read first, then retry with start_line={line_num} or corrected old_lines."),
            }).to_string();
        }
        return err("NO_MATCH", "old_lines not found",
            "Verify current file content or use start_line/end_line for line-number editing.");
    }

    let match_idx = match disambiguate_match(&candidates, &context_before, &context_after, &file_lines, &path, win) {
        Ok(idx) => idx,
        Err(msg) => return msg,
    };

    apply_diff_and_format(&path, &file_lines, match_idx, win, &new_lines, description, was_fuzzy, dry_run, was_crlf)
}

handler!(handle_edit_file_diff, exec_edit_file_diff);

// ── Registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: "file_write".to_string(),
        description: "Create, overwrite, or append to a file.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"If true, append to file instead of overwriting","default":false}},"required":["path","content"],"additionalProperties":false}),
        handler: handle_write_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: "file_edit".to_string(),
        description: "String replacement in files. Supports exact match, regex, multi-file (paths array), and batch edits (patterns array).",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"paths":{"type":"array","items":{"type":"string"},"description":"Multiple file paths"},"old_string":{"type":"string","description":"Text to find"},"new_string":{"type":"string","description":"Replacement text"},"patterns":{"type":"array","items":{"type":"object","properties":{"old":{"type":"string"},"new":{"type":"string"}},"required":["old","new"]},"description":"Array of {old, new} for batch edits"},"replace_all":{"type":"boolean","description":"Replace all occurrences","default":false},"regex":{"type":"boolean","description":"Treat old_string as regex","default":false},"dry_run":{"type":"boolean","description":"Preview diff only, do not write file","default":false}},"required":["path"],"additionalProperties":false}),
        handler: handle_edit_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(60),
    });
    mgr.register(ToolHandler {
        key: "file_edit_diff".to_string(),
        description: "Fuzzy multi-line edit via old_lines+new_lines. Supports line-number addressing with start_line/end_line.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"old_lines":{"type":"array","items":{"type":"string"},"description":"Lines to remove (not needed when start_line set)"},"new_lines":{"type":"array","items":{"type":"string"},"description":"Lines to insert in place of old_lines"},"context_before":{"type":"array","items":{"type":"string"},"description":"Lines just before the change for disambiguation"},"context_after":{"type":"array","items":{"type":"string"},"description":"Lines just after the change for disambiguation"},"start_line":{"type":"integer","description":"1-based line number to start replacement at (bypasses content matching)"},"end_line":{"type":"integer","description":"1-based line number to end replacement at (inclusive, defaults to start_line)"},"dry_run":{"type":"boolean","description":"Preview diff only, do not write file (default true)","default":true},"description":{"type":"string","description":"Why this change is needed (optional)"}},"required":["path","new_lines"],"additionalProperties":false}),
        handler: handle_edit_file_diff,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: "file_delete".to_string(),
        description: "Move file to trash (.deepx/trash/) instead of permanent deletion.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path to delete"}},"required":["path"],"additionalProperties":false}),
        handler: handle_delete_file,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "file_move".to_string(),
        description: "Move or rename a file or directory. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_move_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: "file_copy".to_string(),
        description: "Copy a file. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_copy_file,
        risk: ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
