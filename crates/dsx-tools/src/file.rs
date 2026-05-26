//! File operations: read, write, edit, search, list directory.
//!
//! Each sub-tool is a standalone `ToolHandler` registered via `register()`.
//! The old consolidated `file` dispatcher (file_def/exec_file/classify_file)
//! has been removed — callers use the individual tools directly.

use std::process::Command;
use std::time::Duration;

use super::{parse_arg, parse_arg_or, parse_opt, parse_opt_bool};
use super::{ToolManager, ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict};

// ═══════════════════════════════════════════════════════════════════════════
// Individual tool implementations (unchanged internals)
// ═══════════════════════════════════════════════════════════════════════════

pub(super) fn exec_read_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let start: Option<usize> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("start_line")?.as_u64().map(|n| (n as usize).max(1)));
    let end: Option<usize> = serde_json::from_str(args).ok()
        .and_then(|v: serde_json::Value| v.get("end_line")?.as_u64().map(|n| n as usize));

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let all_lines: Vec<&str> = content.lines().collect();
            let total = all_lines.len();
            let start_idx = start.map(|s| (s - 1).min(total)).unwrap_or(0);
            let end_idx = end.map(|e| e.min(total)).unwrap_or(total);
            let start_idx = start_idx.min(end_idx);
            let lines: Vec<&str> = all_lines[start_idx..end_idx].to_vec();
            let shown = lines.len();
            let total_lines = all_lines.len();

            if start.is_some() || end.is_some() {
                let mut result = format!("[OK] {} lines {}-{}/{} of {}\n", shown, start_idx + 1, end_idx, total_lines, path);
                for (i, l) in lines.iter().enumerate() {
                    result.push_str(&format!("{:>6}  {}\n", start_idx + i + 1, l));
                }
                result
            } else {
                let head: Vec<&str> = lines.iter().take(50).cloned().collect();
                let tail: Vec<&str> = lines.iter().rev().take(10).collect::<Vec<_>>().into_iter().rev().cloned().collect();
                let mut result = format!("[PARTIAL] {} lines, showing 1-50/{}\n", total_lines, path);
                for (i, l) in head.iter().enumerate() {
                    result.push_str(&format!("{:>6}  {}\n", i + 1, l));
                }
                if total_lines > 50 {
                    result.push_str("  ⋮\n");
                    for (i, l) in tail.iter().enumerate() {
                        result.push_str(&format!("{:>6}  {}\n", total_lines - tail.len() + i + 1, l));
                    }
                    result.push_str(&format!("[HINT] Use start_line=N end_line=N to read specific lines.\n"));
                }
                result
            }
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("valid UTF-8") || err_msg.contains("utf8") || err_msg.contains("utf-8") {
                let meta = std::fs::metadata(&path);
                let size = meta.as_ref().map(|m| format!(", {}B", m.len())).unwrap_or_default();
                format!("[OK] {} — binary file{} (cannot display as text)\n[HINT] Use exec(\"file '{}'\") to identify format, or exec(\"xxd '{}'\") for hex dump.", path, size, path, path)
            } else {
                let url_hint = if path.contains("://") || path.contains(".com") || path.contains("www.") {
                    "\n[HINT] This looks like a URL — did you mean to call web_fetch() instead of read_file()?"
                } else { "" };
                format!("[ERROR] Cannot read {}: {}\n[HINT] Use list_dir() on the parent directory to verify the file exists, or check the path spelling.{}", path, e, url_hint)
            }
        },
    }
}

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
                return format!("[OK] {} — binary file, edit_file works on text only\n[HINT] Use exec with appropriate tool for binary files.", path);
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
                let diff = build_diff(&content, &new_content, &old, &new, &path, true);
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
                        let diff = build_diff(&content, &new_content, &old, &new, &path, false);
                        format!("[OK] {}:{} +{} -{}\n\n{}", path, line, new.len(), old.len(), diff)
                    }
                    Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify the parent directory exists and is writable. Use exec(\"ls -la\") or explore() to check.", path, e),
                }
            }
            None => format!("[PARTIAL] {} — string not found\n[HINT] The old_string may have changed. Re-read the file and try again.", path),
        }
    }
}

/// Build a diff display with 3 lines of context
pub(super) fn build_diff(before: &str, after: &str, old: &str, new: &str, path: &str, _all: bool) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let change_line = before_lines.iter()
        .position(|l| l.contains(old.lines().next().unwrap_or(old)))
        .unwrap_or(0);

    let ctx_start = change_line.saturating_sub(3);
    let _ctx_end = (change_line + 3).min(before_lines.len());

    let mut diff = String::new();
    diff.push_str(&format!("  {}  (line {})\n", path, change_line + 1));

    for i in ctx_start..change_line {
        diff.push_str(&format!("      {:>4}  {}\n", i + 1, before_lines[i]));
    }

    let before_snippet: Vec<&str> = before_lines[change_line..(change_line + old.lines().count()).min(before_lines.len())].to_vec();
    let after_snippet: Vec<&str> = after_lines[change_line..(change_line + new.lines().count()).min(after_lines.len())].to_vec();

    for bl in &before_snippet {
        diff.push_str(&format!("  -   {:>4}  {}\n", change_line + 1, bl));
    }
    for al in &after_snippet {
        diff.push_str(&format!("  +   {:>4}  {}\n", change_line + 1, al));
    }

    let after_change = change_line + new.lines().count();
    for i in after_change..(after_change + 3).min(after_lines.len()) {
        diff.push_str(&format!("      {:>4}  {}\n", i + 1, after_lines[i]));
    }

    diff
}

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

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            let err = e.to_string();
            if err.contains("UTF-8") || err.contains("utf-8") {
                return format!("[OK] {} — binary file\n[HINT] Use exec with hex dump tool.", path);
            }
            return format!("[ERROR] Cannot read {}: {}\n[HINT] Use list_dir() first.", path, e);
        }
    };
    let file_lines: Vec<&str> = content.lines().collect();
    let normalize = |s: &str| s.trim_end().to_string();
    let norm_old: Vec<String> = old_lines.iter().map(|l| normalize(l)).collect();
    let win = norm_old.len();
    if win > file_lines.len() {
        return format!("[ERROR] old_lines ({} lines) longer than file ({} lines)", win, file_lines.len());
    }

    // Phase 1: exact match
    let mut candidates: Vec<usize> = Vec::new();
    let mut was_fuzzy = false;
    for i in 0..=file_lines.len() - win {
        let window: Vec<String> = file_lines[i..i+win].iter().map(|l| normalize(l)).collect();
        if window == norm_old { candidates.push(i); }
    }
    // Phase 2: fuzzy match
    if candidates.is_empty() {
        was_fuzzy = true;
        for i in 0..=file_lines.len() - win {
            let window: Vec<String> = file_lines[i..i+win].iter().map(|l| normalize(l)).collect();
            if window.iter().zip(&norm_old).all(|(w, o)| w.trim() == o.trim()) {
                candidates.push(i);
            }
        }
    }
    if candidates.is_empty() {
        return format!("[PARTIAL] {} — old_lines not found\n[HINT] Verify current file content.", path);
    }

    // Disambiguate with context
    let match_idx = if candidates.len() == 1 {
        candidates[0]
    } else {
        let norm_before: Vec<String> = context_before.iter().map(|l| normalize(l)).collect();
        let norm_after: Vec<String> = context_after.iter().map(|l| normalize(l)).collect();
        if norm_before.is_empty() && norm_after.is_empty() {
            let locs: Vec<String> = candidates.iter().take(5).map(|&i| format!("L{}", i+1)).collect();
            return format!("[PARTIAL] {} — old_lines matches at {} locations: {}\n[HINT] Add context_before/context_after to disambiguate.", path, candidates.len(), locs.join(", "));
        }
        let mut best = candidates[0];
        let mut best_score: i32 = -1000;
        for &pos in &candidates {
            let mut score = 0i32;
            for (j, cl) in norm_before.iter().enumerate() {
                let fi = pos as i32 - norm_before.len() as i32 + j as i32;
                if fi >= 0 && (fi as usize) < file_lines.len() {
                    let fl = normalize(file_lines[fi as usize]);
                    if fl == *cl { score += 3; } else if fl.trim() == cl.trim() { score += 1; } else { score -= 1; }
                } else { score -= 2; }
            }
            for (j, cl) in norm_after.iter().enumerate() {
                let fi = pos + win + j;
                if fi < file_lines.len() {
                    let fl = normalize(file_lines[fi]);
                    if fl == *cl { score += 3; } else if fl.trim() == cl.trim() { score += 1; } else { score -= 1; }
                } else { score -= 2; }
            }
            if score > best_score { best = pos; best_score = score; }
        }
        best
    };

    // Apply: remove old, insert new
    let mut out_lines: Vec<&str> = file_lines.to_vec();
    out_lines.splice(match_idx..match_idx + win, std::iter::empty());
    for (j, line) in new_lines.iter().enumerate() {
        out_lines.insert(match_idx + j, line);
    }
    let new_content = out_lines.join("\n");
    let added = new_lines.len() as u32;
    let removed = win as u32;

    match std::fs::write(path, &new_content) {
        Ok(_) => {
            let line = match_idx + 1;
            let mut result = format!("[OK] {}:{}\n", path, line);
            if was_fuzzy {
                result.push_str("⚠ fuzzy match (indentation normalized)\n");
            }
            let ctx_start = match_idx.saturating_sub(2);
            let ctx_end = (match_idx + win + 2).min(out_lines.len()).max(match_idx + 1);
            result.push_str("── change ──\n");
            for i in ctx_start..match_idx {
                result.push_str(&format!("  {:>4}  {}\n", i+1, file_lines[i]));
            }
            for i in match_idx..match_idx + win {
                result.push_str(&format!("- {:>4}  {}\n", i+1, file_lines[i]));
            }
            for (j, l) in new_lines.iter().enumerate() {
                result.push_str(&format!("+ {:>4}  {}\n", match_idx + 1 + j, l));
            }
            for i in (match_idx + win)..ctx_end {
                if i < out_lines.len() {
                    result.push_str(&format!("  {:>4}  {}\n", i+1, out_lines[i]));
                }
            }
            let desc = if description.is_empty() { "edited" } else { description };
            result.push_str(&format!("\n[CHANGE] {}:{} +{} -{} | {}", path, line, added, removed, desc));
            result
        }
        Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify parent directory exists and is writable.", path, e),
    }
}

pub(super) fn exec_list_dir(args: &str) -> String {
    let path = parse_arg_or(args, "path", ".");
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut result = String::from("Directory listing: ");
            result.push_str(&path);
            result.push('\n');
            for entry in entries.flatten() {
                let ft = entry.file_type().map(|t| if t.is_dir() { "/" } else { "" }).unwrap_or("?");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let name = entry.file_name();
                let name_s = name.to_string_lossy();
                if name_s.starts_with('.') { continue; }
                if ft == "/" {
                    result.push_str(&format!("  {:<40} <DIR>\n", name_s + "/"));
                } else {
                    let sz = if size > 1024*1024 { format!("{:.1}M", size as f64 / 1_048_576.0) }
                        else if size > 1024 { format!("{}K", size / 1024) }
                        else { format!("{}B", size) };
                    result.push_str(&format!("  {:<40} {:>6}\n", name_s, sz));
                }
            }
            result
        }
        Err(e) => format!("Error listing {}: {}", path, e),
    }
}

pub(super) fn exec_search(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let glob = parse_opt(args, "glob");
    let dir = parse_arg_or(args, "path", ".");

    // Phase 1: try ripgrep (cross-platform, fast)
    let mut cmd = Command::new("rg");
    cmd.arg("-n").arg("--no-heading");
    if let Some(ref g) = glob {
        cmd.arg("-g").arg(g);
    }
    cmd.arg(&pattern).arg(&dir);

    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            let all_lines: Vec<&str> = out.lines().collect();
            let lines: Vec<&str> = all_lines.iter().take(200).copied().collect();
            if lines.is_empty() {
                return format!("No matches for '{}'", pattern);
            }
            let truncated = if all_lines.len() > 200 {
                format!("\n... ({} more matches)", all_lines.len() - 200)
            } else {
                String::new()
            };
            return lines.join("\n") + &truncated;
        }
        _ => {} // rg not installed or errored — fall through to pure Rust
    }

    // Phase 2: pure Rust fallback (regex + manual file walking)
    match rust_search(&pattern, glob, &dir) {
        Ok(lines) => {
            if lines.is_empty() {
                format!("No matches for '{}'", pattern)
            } else {
                let result: Vec<&str> = lines.iter().take(200).map(|s| s.as_str()).collect();
                let truncated = if lines.len() > 200 {
                    format!("\n... ({} more matches)", lines.len() - 200)
                } else {
                    String::new()
                };
                result.join("\n") + &truncated
            }
        }
        Err(e) => format!("search error: {}", e),
    }
}

fn rust_search(pattern: &str, glob: Option<String>, dir: &str) -> Result<Vec<String>, String> {
    let re = regex::Regex::new(pattern).map_err(|e| format!("invalid regex: {}", e))?;
    let mut results = Vec::new();
    let root = std::path::Path::new(dir);
    walk_dir(root, glob.as_deref(), &re, &mut results)
        .map_err(|e| format!("{}: {}", dir, e))?;
    Ok(results)
}

fn walk_dir(
    dir: &std::path::Path,
    glob: Option<&str>,
    re: &regex::Regex,
    results: &mut Vec<String>,
) -> std::io::Result<()> {
    if results.len() >= 200 {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let fname = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();

        if path.is_dir() {
            if fname.starts_with('.') || fname == "target" || fname == "node_modules" {
                continue;
            }
            walk_dir(&path, glob, re, results)?;
        } else if path.is_file() {
            if results.len() >= 200 {
                return Ok(());
            }
            if let Some(g) = glob {
                if !simple_glob_match(g, &fname) {
                    continue;
                }
            }
            if is_binary_file(&path) {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (i, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    results.push(format!("{}:{}:{}", path.display(), i + 1, line));
                    if results.len() >= 200 {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

fn simple_glob_match(glob: &str, filename: &str) -> bool {
    if glob == "*" {
        return true;
    }
    let starts = glob.starts_with('*');
    let ends = glob.ends_with('*');
    let inner = glob.trim_matches('*');
    match (starts, ends) {
        (true, true) => filename.contains(inner),
        (true, false) => filename.ends_with(inner),
        (false, true) => filename.starts_with(inner),
        (false, false) => filename == glob,
    }
}

fn is_binary_file(path: &std::path::Path) -> bool {
    match std::fs::read(path) {
        Ok(data) => {
            let check = &data[..data.len().min(8192)];
            check.contains(&0u8)
        }
        Err(_) => false,
    }
}

pub(super) fn exec_delete_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let p = std::path::Path::new(&path);
    if p.is_dir() {
        return format!("[ERROR] {} is a directory. Use delete_dir or exec(\"rm -rf\") instead.", path);
    }
    match std::fs::remove_file(p) {
        Ok(_) => format!("[OK] Deleted {}", path),
        Err(e) => format!("[ERROR] Cannot delete {}: {}\n[HINT] Check if the file exists and is writable.", path, e),
    }
}

pub(super) fn exec_move_file(args: &str) -> String {
    let source = parse_arg(args, "source");
    let dest = parse_arg(args, "dest");
    if let Some(parent) = std::path::Path::new(&dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(&source, &dest) {
        Ok(_) => format!("[OK] Moved {} → {}", source, dest),
        Err(e) => format!("[ERROR] Cannot move {}: {}", source, e),
    }
}

pub(super) fn exec_copy_file(args: &str) -> String {
    let source = parse_arg(args, "source");
    let dest = parse_arg(args, "dest");
    if let Some(parent) = std::path::Path::new(&dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::copy(&source, &dest) {
        Ok(size) => format!("[OK] Copied {} → {} ({} bytes)", source, dest, size),
        Err(e) => format!("[ERROR] Cannot copy {}: {}", source, e),
    }
}

pub(super) fn exec_glob(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let path = parse_arg_or(args, "path", ".");
    // Strip **/ for filename matching (walk is already recursive)
    let file_pattern = if pattern.contains("**/") {
        &pattern[pattern.rfind("**/").unwrap() + 3..]
    } else if pattern.contains("**\\") {
        &pattern[pattern.rfind("**\\").unwrap() + 3..]
    } else {
        pattern.as_str()
    };
    let mut results = Vec::new();
    let root = std::path::Path::new(&path);
    if let Err(e) = glob_walk(root, file_pattern, &mut results) {
        return format!("glob error: {}", e);
    }
    if results.is_empty() {
        return format!("No files matching '{}'", pattern);
    }
    results.join("\n")
}

fn glob_walk(dir: &std::path::Path, file_pattern: &str, results: &mut Vec<String>) -> std::io::Result<()> {
    if results.len() >= 500 {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let fname = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        if path.is_dir() {
            if fname.starts_with('.') || fname == "target" || fname == "node_modules" {
                continue;
            }
            glob_walk(&path, file_pattern, results)?;
        } else if path.is_file() {
            if results.len() >= 500 {
                return Ok(());
            }
            if simple_glob_match(file_pattern, &fname) {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                let sz = if size > 1024 * 1024 {
                    format!("{:.1}M", size as f64 / 1_048_576.0)
                } else if size > 1024 {
                    format!("{}K", size / 1024)
                } else {
                    format!("{}B", size)
                };
                results.push(format!("{} ({})", path.display(), sz));
            }
        }
    }
    Ok(())
}

pub(super) fn exec_diff(args: &str) -> String {
    let path_a = parse_arg(args, "path_a");
    let path_b = parse_arg(args, "path_b");

    let content_a = match std::fs::read_to_string(&path_a) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] Cannot read {}: {}", path_a, e),
    };
    let content_b = match std::fs::read_to_string(&path_b) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] Cannot read {}: {}", path_b, e),
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

// ═══════════════════════════════════════════════════════════════════════════
// Handler wrappers (bridge ToolCallCtx → old-style exec_* string-args)
// ═══════════════════════════════════════════════════════════════════════════

fn handle_read_file(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_read_file(&args))
}

fn handle_write_file(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_write_file(&args))
}

fn handle_edit_file(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_edit_file(&args))
}

fn handle_edit_file_diff(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_edit_file_diff(&args))
}

fn handle_list_dir(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_list_dir(&args))
}

fn handle_search(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_search(&args))
}

fn handle_delete_file(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_delete_file(&args))
}

fn handle_move_file(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_move_file(&args))
}

fn handle_copy_file(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_copy_file(&args))
}

fn handle_glob(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_glob(&args))
}

fn handle_diff(ctx: ToolCallCtx) -> ToolResult {
    let args = serde_json::to_string(&ctx.args).unwrap_or_default();
    ToolResult::ok(exec_diff(&args))
}

// ═══════════════════════════════════════════════════════════════════════════
// Safety classifiers
// ═══════════════════════════════════════════════════════════════════════════

fn safety_read_file(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_write_file(ctx: &ToolCallCtx) -> SafetyVerdict {
    if let Some(path) = ctx.get_str("path") {
        if path.starts_with(std::env::temp_dir().to_string_lossy().as_ref()) {
            return SafetyVerdict::Allow;
        }
    }
    SafetyVerdict::Allow
}

fn safety_edit_file(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_edit_file_diff(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_list_dir(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_search(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_delete_file(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_move_file(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_copy_file(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_glob(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

fn safety_diff(_ctx: &ToolCallCtx) -> SafetyVerdict {
    SafetyVerdict::Allow
}

// ═══════════════════════════════════════════════════════════════════════════
// Registration entry point
// ═══════════════════════════════════════════════════════════════════════════

pub fn register(mgr: &mut ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("read_file", ""),
        description: "Read file content. Default preview: first 50 lines + last 10 lines. Use start_line/end_line for precise range.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "start_line": {"type": "integer", "description": "First line to read (1-based)", "default": 1},
                "end_line": {"type": "integer", "description": "Last line to read (inclusive). If omitted, reads to end of file."}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        handler: handle_read_file,
        safety: safety_read_file,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("write_file", ""),
        description: "Create, overwrite, or append to a file. Creates parent dirs. For new files or full rewrites; prefer edit_file for small changes.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path to write"},
                "content": {"type": "string", "description": "File content"},
                "append": {"type": "boolean", "description": "Append to file instead of overwriting", "default": false}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
        handler: handle_write_file,
        safety: safety_write_file,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("edit_file", ""),
        description: "Find-and-replace in a file. Supports regex with regex=true, replace_all for all occurrences. Surgical edits only.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file to edit"},
                "old_string": {"type": "string", "description": "Text to find (exact, or regex if regex=true)"},
                "new_string": {"type": "string", "description": "Replacement text"},
                "replace_all": {"type": "boolean", "description": "Replace all occurrences", "default": false},
                "regex": {"type": "boolean", "description": "Treat old_string as a regex pattern", "default": false}
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        }),
        handler: handle_edit_file,
        safety: safety_edit_file,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("edit_file_diff", ""),
        description: "Context/Fuzzy edit: give old_lines+new_lines+optional context. Tolerant of whitespace changes. Use INSTEAD of edit_file when exact old_string is uncertain or changing multi-line blocks.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path to edit"},
                "old_lines": {"type": "array", "items": {"type": "string"}, "description": "Lines to replace"},
                "new_lines": {"type": "array", "items": {"type": "string"}, "description": "Replacement lines (empty = delete)"},
                "context_before": {"type": "array", "items": {"type": "string"}, "description": "Lines before the change"},
                "context_after": {"type": "array", "items": {"type": "string"}, "description": "Lines after the change"},
                "description": {"type": "string", "description": "What changed and why"}
            },
            "required": ["path", "old_lines", "new_lines"],
            "additionalProperties": false
        }),
        handler: handle_edit_file_diff,
        safety: safety_edit_file_diff,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("list_dir", ""),
        description: "List files and directories with names and sizes.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path to list", "default": "."}
            },
            "required": [],
            "additionalProperties": false
        }),
        handler: handle_list_dir,
        safety: safety_list_dir,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("search", ""),
        description: "Regex search across files. Returns file:line matches. Grep for your codebase.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regex or literal pattern"},
                "glob": {"type": "string", "description": "File glob (e.g. *.rs)"},
                "path": {"type": "string", "description": "Directory to search", "default": "."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        }),
        handler: handle_search,
        safety: safety_search,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("delete_file", ""),
        description: "Delete a file permanently. Use with caution.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path to delete"}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        handler: handle_delete_file,
        safety: safety_delete_file,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("move_file", ""),
        description: "Move or rename a file or directory. Creates parent dirs of dest.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "source": {"type": "string", "description": "Source path"},
                "dest": {"type": "string", "description": "Destination path"}
            },
            "required": ["source", "dest"],
            "additionalProperties": false
        }),
        handler: handle_move_file,
        safety: safety_move_file,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("copy_file", ""),
        description: "Copy a file. Creates parent dirs of dest.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "source": {"type": "string", "description": "Source path"},
                "dest": {"type": "string", "description": "Destination path"}
            },
            "required": ["source", "dest"],
            "additionalProperties": false
        }),
        handler: handle_copy_file,
        safety: safety_copy_file,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("glob", ""),
        description: "Find files matching a glob pattern recursively (e.g. *.rs, src/**/*.rs).",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern (e.g. *.rs, src/**/*.rs)"},
                "path": {"type": "string", "description": "Directory to search", "default": "."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        }),
        handler: handle_glob,
        safety: safety_glob,
        default_timeout: Duration::from_secs(15),
    });

    mgr.register(ToolHandler {
        key: ToolKey::new("diff", ""),
        description: "Compare two files line by line. Shows first diff region with context.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path_a": {"type": "string", "description": "First file path"},
                "path_b": {"type": "string", "description": "Second file path"}
            },
            "required": ["path_a", "path_b"],
            "additionalProperties": false
        }),
        handler: handle_diff,
        safety: safety_diff,
        default_timeout: Duration::from_secs(15),
    });
}
