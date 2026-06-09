use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use super::file_shared::{build_diff, normalize_newlines, closest_line};

// ── Argument parsers ──

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

// ── Single-pattern applicator ──

enum Match {
    Ok { msg: String },
    NoMatch { msg: String },
    Error { msg: String },
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
        let new_content = if replace_all {
            re.replace_all(content, new).to_string()
        } else {
            re.replacen(content, 1, new).to_string()
        };
        let msg = if replace_all { format!("regex replaced {count} matches") } else { "regex replaced 1 match".into() };
        (new_content, Match::Ok { msg })
    } else if replace_all {
        if !content.contains(old) {
            let hint = match closest_line(content, old) {
                Some((line_no, line)) => format!("\n[HINT] Closest match at line {line_no}: {}", line.chars().take(80).collect::<String>()),
                None => String::new(),
            };
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
                let hint = match closest_line(content, old) {
                    Some((line_no, line)) => format!("\n[HINT] Closest match at line {line_no}: {}", line.chars().take(80).collect::<String>()),
                    None => String::new(),
                };
                (content.to_string(), Match::NoMatch { msg: format!("string not found{hint}") })
            }
        }
    }
}

// ── Main ──

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
        let raw = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                let em = e.to_string();
                if em.contains("valid UTF-8") || em.contains("utf8") || em.contains("utf-8") {
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
            let diff = build_diff(&orig, &content, "", "", path);
            results.push(format!("[DRY RUN] {path} — {n} pattern(s), no changes written\n\n{diff}", n = patterns.len()));
        } else {
            match std::fs::write(path, &content) {
                Ok(_) => {
                    let diff = build_diff(&orig, &content, "", "", path);
                    results.push(format!("[OK] {path} — {n} pattern(s): {summary}\n\n{diff}", n = patterns.len(), summary = msgs.join("; ")));
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


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("edit_file", ""),
        description: "Surgical find-and-replace in files. Single: {path, old_string, new_string}. Multi-pattern: {path, patterns: [{old,new}]}. Multi-file: {paths: [a,b], patterns: [{old,new}]}. Dry-run: dry_run=true. Regex: regex=true. Supports replace_all.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path (single)"},"paths":{"type":"array","items":{"type":"string"},"description":"File paths (multiple)"},"old_string":{"type":"string","description":"Text to find (single pattern)"},"new_string":{"type":"string","description":"Replacement text (single pattern)"},"patterns":{"type":"array","items":{"type":"object","properties":{"old":{"type":"string"},"new":{"type":"string"}},"required":["old","new"]},"description":"Array of {old, new} for batch edits"},"replace_all":{"type":"boolean","description":"Replace all occurrences","default":false},"regex":{"type":"boolean","description":"Treat old_string as regex","default":false},"dry_run":{"type":"boolean","description":"Preview diff only, do not write file","default":false},"reason":{"type":"string","description":"Why this change is needed (optional)"}},"required":[],"additionalProperties":false}),
        handler: handle_edit_file,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(60),
    });
}