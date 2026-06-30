//! glob tool — file finder powered by `ignore` crate walker.
//! Replaces the hand-written `walk_dir` with ripgrep's gitignore-aware engine.

use crate::{parse_arg, parse_arg_or, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_glob(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let path = parse_arg_or(args, "path", ".");

    let mut results = Vec::new();
    let max_results = 500;

    let walker = ignore::WalkBuilder::new(&path)
        .hidden(false)
        .git_ignore(false)
        .require_git(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    for entry in walker {
        if results.len() >= max_results {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let Some(ft) = entry.file_type() {
            if !ft.is_file() && !ft.is_symlink() {
                continue;
            }
        } else {
            continue;
        }
        let fname = entry.file_name().to_string_lossy();
        if !super::file_shared::simple_glob_match(&pattern, &fname) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let sz = if size > 1024 * 1024 {
            format!("{:.1}M", size as f64 / 1_048_576.0)
        } else if size > 1024 {
            format!("{}K", size / 1024)
        } else {
            format!("{}B", size)
        };
        results.push(format!("{} ({})", entry.path().display(), sz));
    }

    if results.is_empty() {
        format!("No files matching '{}'", pattern)
    } else {
        results.join("\n")
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
