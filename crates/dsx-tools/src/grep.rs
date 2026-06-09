use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use std::path::PathBuf;
use std::process::Command;

fn find_binary(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(name);
            if p.exists() { return p; }
        }
    }
    PathBuf::from(name)
}

pub(super) fn exec_grep(args: &str) -> String {
    let pattern = parse_arg(args, "pattern");
    let path = parse_arg(args, "path");
    let recursive = parse_opt_bool(args, "recursive").unwrap_or(true);
    let line_numbers = parse_opt_bool(args, "line_numbers").unwrap_or(true);
    if pattern.is_empty() || path.is_empty() {
        return "[ERROR] grep: pattern and path required".into();
    }
    let grep_path = find_binary("grep.exe");
    let mut cmd = Command::new(&grep_path);
    if recursive { cmd.arg("-r"); }
    if line_numbers { cmd.arg("-n"); }
    cmd.arg(&pattern).arg(&path);
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            if !out.status.success() && stdout.is_empty() {
                format!("[OK] grep: no matches for {pattern}")
            } else {
                stdout
            }
        }
        Err(e) => format!("[ERROR] grep: {e}"),
    }
}

handler!(handle_grep, exec_grep);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("grep", ""),
        description: "Search files via grep binary. pattern: regex or literal, path: file/directory. recursive=true, line_numbers=true by default.",
        input_schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Search pattern (regex or literal)"},"path":{"type":"string","description":"File or directory path"},"recursive":{"type":"boolean","description":"Search recursively (default true)","default":true},"line_numbers":{"type":"boolean","description":"Show line numbers (default true)","default":true}},"required":["pattern","path"],"additionalProperties":false}),
        handler: handle_grep,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
