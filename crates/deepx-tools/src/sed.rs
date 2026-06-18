use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};
use std::path::PathBuf;
use std::process::Command;
use super::file_shared::unified_diff;

fn find_binary(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("binaries").join(name);
            if p.exists() { return p; }
            // fallback: try directly next to exe
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

pub(super) fn exec_sed(args: &str) -> String {
    let script = parse_arg(args, "script");
    let scripts = parse_array(args, "scripts");
    let path = parse_arg(args, "path");
    let in_place = parse_opt_bool(args, "in_place").unwrap_or(true);
    let quiet = parse_opt_bool(args, "quiet").unwrap_or(false);

    if path.is_empty() {
        return "[ERROR] sed: path required".into();
    }
    if script.is_empty() && scripts.is_empty() {
        return "[ERROR] sed: script or scripts required".into();
    }

    let sed_path = find_binary("sed.exe");
    let mut cmd = Command::new(&sed_path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if quiet { cmd.arg("-n"); }
    if in_place { cmd.arg("-i"); }
    if !scripts.is_empty() {
        for e in &scripts {
            cmd.arg("-e").arg(e);
        }
    } else {
        cmd.arg(&script);
    }
    cmd.arg(&path);

    // Snapshot before for diff
    let before = if in_place {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).replace('\r', "");
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let desc = if !scripts.is_empty() {
                format!("-e {}", scripts.join(" -e "))
            } else {
                script.clone()
            };

            if in_place {
                if !out.status.success() {
                    return format!("[ERROR] sed: {stderr}");
                }
                let after = std::fs::read_to_string(&path).unwrap_or_default();
                let diff = unified_diff(&before, &after, &path);
                if diff.is_empty() {
                    format!("[OK] sed {} — no changes", desc)
                } else {
                    format!("[OK] sed {}\n\n{}", desc, diff)
                }
            } else {
                if !out.status.success() && stdout.is_empty() {
                    format!("[ERROR] sed: {stderr}")
                } else if stdout.is_empty() {
                    format!("[OK] sed {} (no output)", desc)
                } else {
                    stdout
                }
            }
        }
        Err(e) => format!("[ERROR] sed: {e}"),
    }
}

handler!(handle_sed, exec_sed);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("sed", ""),
        description: "Stream editor via sed binary.\nSupports: s (substitute), d (delete), a\\ (append), i\\ (insert), c\\ (change), p (print with -n), = (line numbers), q (quit), N (append next line), h/H/g/G/x (hold space), y (transliterate), ! (negate), address ranges.\nExamples: sed -i 's/old/new/g' file.txt  →  {\"script\":\"s/old/new/g\",\"path\":\"file.txt\"}\nsed -n '/err/p' log.txt  →  {\"script\":\"/err/p\",\"path\":\"log.txt\",\"quiet\":true}\nsed -e 's/a/b/' -e '/x/d' f  →  {\"scripts\":[\"s/a/b/\",\"/x/d\"],\"path\":\"f\"}\nsed 's/old/new/' f  →  {\"script\":\"s/old/new/\",\"path\":\"f\",\"in_place\":false}  (dry-run preview)",
        input_schema: serde_json::json!({"type":"object","properties":{"script":{"type":"string","description":"sed script, e.g. s/old/new/g. Use ; for multiple commands."},"scripts":{"type":"array","items":{"type":"string"},"description":"Multiple sed scripts passed via -e each."},"path":{"type":"string","description":"Target file path"},"in_place":{"type":"boolean","description":"Edit file in-place (default true). Set false to preview.","default":true},"quiet":{"type":"boolean","description":"Suppress default output (-n). Use with p or = commands.","default":false}},"required":["path"],"additionalProperties":false}),
        handler: handle_sed,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
