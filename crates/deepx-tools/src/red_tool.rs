use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use std::path::PathBuf;
use std::process::Command;

fn find_binary(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("binaries").join(name);
            if p.exists() { return p; }
            let p2 = dir.join(name);
            if p2.exists() { return p2; }
        }
    }
    PathBuf::from(name)
}

fn parse_array(args: &str, key: &str) -> Vec<String> {
    let v: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    match v.get(key) {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    }
}

pub(super) fn exec_red(args: &str) -> String {
    let expr = parse_arg(args, "expression");
    let exprs = parse_array(args, "expressions");
    let path = parse_arg(args, "path");
    let in_place = parse_opt_bool(args, "in_place").unwrap_or(true);
    let quiet = parse_opt_bool(args, "quiet").unwrap_or(false);
    let extended = parse_opt_bool(args, "extended").unwrap_or(false);
    let posix = parse_opt_bool(args, "posix").unwrap_or(false);
    let sandbox = parse_opt_bool(args, "sandbox").unwrap_or(false);
    let null_data = parse_opt_bool(args, "null_data").unwrap_or(false);

    if path.is_empty() {
        return "[ERROR] red: path required".into();
    }
    if expr.is_empty() && exprs.is_empty() {
        return "[ERROR] red: expression or expressions required".into();
    }

    let red_path = find_binary("red.exe");
    let mut cmd = Command::new(&red_path);
    if quiet { cmd.arg("-n"); }
    if extended { cmd.arg("-E"); }
    if posix { cmd.arg("--posix"); }
    if sandbox { cmd.arg("--sandbox"); }
    if null_data { cmd.arg("-z"); }
    if in_place { cmd.arg("-i").arg(""); }
    if !exprs.is_empty() {
        for e in &exprs {
            cmd.arg("-e").arg(e);
        }
    } else {
        cmd.arg(&expr);
    }
    cmd.arg(&path);

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if !out.status.success() && stdout.is_empty() {
                format!("[ERROR] red: {stderr}")
            } else if stdout.is_empty() {
                let desc = if !exprs.is_empty() {
                    format!("-e {}", exprs.join(" -e "))
                } else {
                    expr.clone()
                };
                format!("[OK] red {desc} applied to {path}")
            } else {
                stdout
            }
        }
        Err(e) => format!("[ERROR] red: {e}"),
    }
}

handler!(handle_red, exec_red);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("red", ""),
        description: "Stream editor via red binary (experimental Rust sed). 100% GNU sed compatible. Supports: s (substitute), d (delete), a\\ (append), i\\ (insert), c\\ (change), p (print), = (line numbers), q (quit), N/n (next line), h/H/g/G/x (hold space), y (transliterate), ! (negate), address ranges, { } blocks. expression: single sed script or ;-separated commands. expressions: array for -e scripts. path: target file. extended: use extended regex (-E). posix: disable GNU extensions. sandbox: disable e/r/w commands. null_data: NUL line separator (-z). in_place: edit in-place (default true, set false to preview). quiet: suppress default output / -n.",
        input_schema: serde_json::json!({"type":"object","properties":{"expression":{"type":"string","description":"sed script, e.g. s/old/new/g. Use ; for multiple commands."},"expressions":{"type":"array","items":{"type":"string"},"description":"Multiple sed scripts passed via -e each."},"path":{"type":"string","description":"Target file path"},"in_place":{"type":"boolean","description":"Edit file in-place (default true). Set false to preview.","default":true},"quiet":{"type":"boolean","description":"Suppress default output (-n). Use with p or = commands.","default":false},"extended":{"type":"boolean","description":"Use extended regular expressions (-E).","default":false},"posix":{"type":"boolean","description":"Disable non-POSIX extensions (--posix).","default":false},"sandbox":{"type":"boolean","description":"Operate in sandbox mode, disabling e/r/w commands.","default":false},"null_data":{"type":"boolean","description":"Use NUL character as line separator (-z).","default":false}},"required":["path"],"additionalProperties":false}),
        handler: handle_red,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
