use crate::{parse_arg, parse_arg_or, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_glob(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let path = parse_arg_or(args, "path", ".");
    // Strip **/ for filename matching (walk is already recursive)
    let file_pattern = if let Some(pos) = pattern.rfind("**/") {
        &pattern[pos + 3..]
    } else if let Some(pos) = pattern.rfind("**\\") {
        &pattern[pos + 3..]
    } else {
        pattern.as_str()
    };
    let mut results = Vec::new();
    let root = std::path::Path::new(&path);
    if let Err(e) = glob_walk(root, file_pattern, &mut results) {
        return format!("[ERROR] glob failed: {}\n[HINT] Check the pattern syntax.", e);
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

handler!(handle_glob, exec_glob);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("glob", ""),
        description: "Find files matching a glob pattern recursively (e.g. *.rs, src/**/*.rs).",
        input_schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Glob pattern"},"path":{"type":"string","description":"Start directory","default":"."}},"required":["pattern"],"additionalProperties":false}),
        handler: handle_glob,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
