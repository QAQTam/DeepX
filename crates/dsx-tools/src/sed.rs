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

pub(super) fn exec_sed(args: &str) -> String {
    let expr = parse_arg(args, "expression");
    let path = parse_arg(args, "path");
    let in_place = parse_opt_bool(args, "in_place").unwrap_or(true);
    if expr.is_empty() || path.is_empty() {
        return "[ERROR] sed: expression and path required".into();
    }
    let sed_path = find_binary("sed.exe");
    let mut cmd = Command::new(&sed_path);
    if in_place { cmd.arg("-i").arg(""); }
    cmd.arg(&expr).arg(&path);
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if !out.status.success() && stdout.is_empty() {
                format!("[ERROR] sed: {stderr}")
            } else if stdout.is_empty() {
                format!("[OK] sed {expr} applied to {path}")
            } else {
                stdout
            }
        }
        Err(e) => format!("[ERROR] sed: {e}"),
    }
}

handler!(handle_sed, exec_sed);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("sed", ""),
        description: "Stream editor via sed binary. expression: sed script, e.g. s/old/new/g or s/a/b/; s/c/d/ for multiple. path: target file. Set in_place=false to preview.",
        input_schema: serde_json::json!({"type":"object","properties":{"expression":{"type":"string","description":"sed expression, e.g. s/old/new/g or s/a/b/; s/c/d/"},"path":{"type":"string","description":"Target file path"},"in_place":{"type":"boolean","description":"Edit file in-place (default true)","default":true}},"required":["expression","path"],"additionalProperties":false}),
        handler: handle_sed,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
