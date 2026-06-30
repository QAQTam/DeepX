use std::process::Command;
use crate::{parse_arg, parse_opt, parse_arg_or, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use super::file_shared::rust_grep;

pub(super) fn exec_search(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let glob = parse_opt(args, "glob");
    let dir = parse_arg_or(args, "path", ".");

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

    // Phase 2: pure Rust fallback
    match rust_grep(&pattern, &dir, true, true, glob.as_deref(), 100) {
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

handler!(handle_search, exec_search);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("file", "search"),
        description: "Regex search across files. Returns file:line matches.",
        input_schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Regex pattern"},"glob":{"type":"string","description":"File glob filter (e.g. *.rs)"},"path":{"type":"string","description":"Search directory","default":"."}},"required":["pattern"],"additionalProperties":false}),
        handler: handle_search,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
