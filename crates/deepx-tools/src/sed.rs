//! sed tool — platform dispatch:
//! - Windows: deepx-sed (pure-Rust sed engine, no GNU sed available)
//! - Linux/macOS: GNU sed binary via std::process::Command
//!
//! Output format mirrors edit_file / edit_file_diff so Tauri can parse diffs:
//!   [OK] path — N change(s): summary
//!   --- a/path
//!   +++ b/path
//!   @@ -L,N +L,N @@
//!   -removed
//!   +added
//!
//!   [CHANGE] path:line +added -removed | sed <script>

use crate::{parse_arg, parse_opt_bool, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

use super::file_shared::diff_stats;
#[cfg(not(windows))]
use super::file_shared::unified_diff;

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

    // Multi-script: join with ; for deepx-sed, pass -e for GNU sed
    let effective_script = if !scripts.is_empty() {
        scripts.join("; ")
    } else {
        script
    };

    #[cfg(windows)]
    {
        let raw = deepx_sed::deepx_run_sed(&effective_script, &path, in_place, quiet);
        // Post-process: if the result contains a unified diff, add [CHANGE] trailer
        post_process_output(&raw, &effective_script, &path)
    }
    #[cfg(not(windows))]
    {
        exec_gnu_sed(&effective_script, &scripts, &path, in_place, quiet)
    }
}

/// Enrich output: if a unified diff is present, build a clean [OK] one-liner + diff.
fn post_process_output(raw: &str, script: &str, path: &str) -> String {
    if raw.starts_with("[ERROR]") || raw.starts_with("[PARTIAL]") {
        return raw.to_string();
    }
    if let Some(diff_start) = raw.find("--- a/") {
        let diff = &raw[diff_start..];
        let (added, removed, first_line) = diff_stats(diff);
        if added > 0 || removed > 0 {
            return format!(
                "[OK] {path}:{first_line} +{added} -{removed} | sed {script}\n\n{diff}",
                path = path, first_line = first_line, added = added, removed = removed, script = script, diff = diff.trim_end()
            );
        }
    }
    raw.to_string()
}

/// Platform-dispatch sed with raw args — used by linuxmod.
/// On Windows: deepx-sed (pure-Rust). On Linux: GNU sed binary.
pub(crate) fn run_sed_core(script: &str, path: &str, in_place: bool) -> String {
    #[cfg(windows)]
    {
        deepx_sed::deepx_run_sed(script, path, in_place, false)
    }
    #[cfg(not(windows))]
    {
        exec_gnu_sed(script, &[], path, in_place, false)
    }
}

/// Apply sed to piped stdin (linuxmod pipe mode).
/// Writes stdin to temp file, runs deepx-sed in-place, reads back result.
pub(crate) fn apply_sed_to_stdin(input: &str, script: &str) -> String {
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("deepx-sed-{}.tmp", std::process::id()));
    if let Err(e) = std::fs::write(&tmp_path, input) {
        return format!("[ERROR] sed: cannot write temp file: {e}");
    }
    let path_str = tmp_path.to_string_lossy();
    let result = deepx_sed::deepx_run_sed(script, &path_str, true, false);
    // For in-place, deepx_run_sed already modified the file. Read it back.
    let content = std::fs::read_to_string(&tmp_path);
    let _ = std::fs::remove_file(&tmp_path);
    if result.starts_with("[ERROR]") {
        return result;
    }
    match content {
        Ok(c) => c,
        Err(e) => format!("[ERROR] sed: cannot read temp file: {e}"),
    }
}

/// Run GNU sed binary on Linux/macOS.
#[cfg(not(windows))]
pub(crate) fn exec_gnu_sed(
    effective_script: &str,
    scripts: &[String],
    path: &str,
    in_place: bool,
    quiet: bool,
) -> String {
    use std::process::Command;

    let desc = effective_script.to_string();
    let before = if in_place {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut cmd = Command::new("sed");
    if in_place {
        cmd.arg("-i");
    }
    if quiet {
        cmd.arg("-n");
    }
    if !scripts.is_empty() {
        for s in scripts {
            cmd.arg("-e").arg(s);
        }
    } else {
        cmd.arg("-e").arg(effective_script);
    }
    cmd.arg(path);

    if !in_place {
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
    }

    match cmd.output() {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return format!("[ERROR] sed: {}\n{}", desc, stderr.trim());
            }
            if in_place {
                let after = std::fs::read_to_string(path).unwrap_or_default();
                let diff = unified_diff(&before, &after, path);
                if diff.is_empty() {
                    format!("[OK] {} — sed {}: no changes", path, desc)
                } else {
                    let (added, removed, first_line) = diff_stats(&diff);
                    let added_count = added.max(1);
                    let removed_count = removed.max(1);
                    format!(
                        "[OK] {path}:{first_line} +{added_count} -{removed_count} | sed {desc}\n\n{diff}",
                        path = path, first_line = first_line, added_count = added_count, removed_count = removed_count, desc = desc, diff = diff.trim_end()
                    )
                }
            } else {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.is_empty() {
                    format!("[OK] {} — sed {}: no output", path, desc)
                } else {
                    stdout.into_owned()
                }
            }
        }
        Err(e) => format!("[ERROR] sed: GNU sed not found or failed to run: {e}"),
    }
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

handler!(handle_sed, exec_sed);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("sed", ""),
        description: "Stream editor (sed). Edits files in-place by default.\nSupports: s (substitute), d (delete), a\\ (append), i\\ (insert), c\\ (change), p (print with -n), = (line numbers), q (quit), N (append next line), h/H/g/G/x (hold space), y (transliterate), ! (negate), address ranges.\nExamples:\ns/old/new/g  →  {\"script\":\"s/old/new/g\",\"path\":\"file.txt\"}\nMultiple commands  →  {\"scripts\":[\"s/a/b/\",\"/x/d\"],\"path\":\"f\"}\nPreview (no write)  →  {\"script\":\"s/old/new/\",\"path\":\"f\",\"in_place\":false}\nQuiet mode (only print matches)  →  {\"script\":\"/err/p\",\"path\":\"log.txt\",\"quiet\":true}\nReturns unified diff with [CHANGE] trailer on in-place edits.",
        input_schema: serde_json::json!({"type":"object","properties":{"script":{"type":"string","description":"sed script, e.g. s/old/new/g. Use ; to chain commands."},"scripts":{"type":"array","items":{"type":"string"},"description":"Multiple sed scripts (equivalent to -e)."},"path":{"type":"string","description":"Target file path"},"in_place":{"type":"boolean","description":"Edit file in-place (default true). Set false to preview only.","default":true},"quiet":{"type":"boolean","description":"Suppress automatic printing. Use with p or = commands.","default":false}},"required":["path"],"additionalProperties":false}),
        handler: handle_sed,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
