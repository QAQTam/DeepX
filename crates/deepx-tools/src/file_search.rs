use std::process::Command;
use crate::{parse_arg, parse_opt, parse_arg_or, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

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
            let lines: Vec<&str> = all_lines.iter().take(100).copied().collect();
            if lines.is_empty() {
                return format!("No matches for '{}'", pattern);
            }
            let truncated = if all_lines.len() > 100 {
                format!("\n... ({} more matches)", all_lines.len() - 100)
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
                let result: Vec<&str> = lines.iter().take(100).map(|s| s.as_str()).collect();
                let truncated = if lines.len() > 100 {
                    format!("\n... ({} more matches)", lines.len() - 100)
                } else {
                    String::new()
                };
                result.join("\n") + &truncated
            }
        }
        Err(e) => format!("[ERROR] search failed: {}\n[HINT] Check the pattern or path.", e),
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
    if results.len() >= 100 {
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
            if results.len() >= 100 {
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
                    if results.len() >= 100 {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

fn simple_glob_match(glob: &str, filename: &str) -> bool {
    if glob == "*" || glob == "**" {
        return true;
    }
    let starts = glob.starts_with('*');
    let ends = glob.ends_with('*');
    let inner = glob.trim_matches('*');
    if inner.is_empty() {
        return true;
    }
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
            let check = &data[..data.len().min(16384)];
            if check.contains(&0u8) {
                return true;
            }
            let non_printable = check.iter()
                .filter(|&&b| b != 0x09 && b != 0x0A && b != 0x0D && (b < 0x20 || b > 0x7E))
                .count();
            non_printable as f64 / check.len().max(1) as f64 > 0.30
        }
        Err(_) => false,
    }
}

handler!(handle_search, exec_search);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("search", ""),
        description: "Regex search across files. Returns file:line matches. Grep for your codebase.",
        input_schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Regex pattern"},"glob":{"type":"string","description":"File glob filter (e.g. *.rs)"},"path":{"type":"string","description":"Search directory","default":"."}},"required":["pattern"],"additionalProperties":false}),
        handler: handle_search,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
