//! Command execution — direct process spawn via argv array.
//!
//! No PTY, no shell, no streaming. Uses `std::process::Command`.
//! Output is read via pipes (not `output()`) to prevent OOM on large outputs,
//! and truncated by actual token count using `deepx_types::token::count_tokens`.

use crate::{ToolCallCtx, ToolResult};
use serde::Serialize;
use std::io::Read;

/// Stream read from a pipe, capped at `max_bytes`. Returns (accumulated_string, was_truncated).
fn read_stream(stream: impl Read, max_bytes: usize) -> (String, bool) {
    let mut reader = std::io::BufReader::new(stream);
    let mut buf = vec![0u8; 8192];
    let mut out = String::new();
    let mut truncated = false;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() + n > max_bytes {
                    let remaining = max_bytes - out.len();
                    out.push_str(&String::from_utf8_lossy(&buf[..remaining]));
                    truncated = true;
                    std::io::copy(&mut reader, &mut std::io::sink()).ok();
                    break;
                }
                out.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            Err(_) => break,
        }
    }
    (out, truncated)
}

/// Find byte index for `target` tokens walking forward.
fn find_token_boundary(text: &str, target_tokens: u32) -> usize {
    let target_f64 = target_tokens as f64;
    let mut char_count = 0usize;
    let mut cjk_count = 0usize;
    for (i, c) in text.char_indices() {
        let is_cjk = matches!(c,
            '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}'
            | '\u{3000}'..='\u{303f}' | '\u{ff00}'..='\u{ffef}'
            | '\u{3040}'..='\u{30ff}'
        );
        if is_cjk { cjk_count += 1; } else { char_count += 1; }
        let est = char_count as f64 / 3.3 + cjk_count as f64 / 1.67;
        if est >= target_f64 { return i; }
    }
    text.len()
}

/// Find byte index for `target` tokens walking backward from end.
fn find_token_boundary_reverse(text: &str, target_tokens: u32) -> usize {
    let target_f64 = target_tokens as f64;
    let mut char_count = 0usize;
    let mut cjk_count = 0usize;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, c) in chars.iter().rev() {
        let is_cjk = matches!(c,
            '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}'
            | '\u{3000}'..='\u{303f}' | '\u{ff00}'..='\u{ffef}'
            | '\u{3040}'..='\u{30ff}'
        );
        if is_cjk { cjk_count += 1; } else { char_count += 1; }
        let est = char_count as f64 / 3.3 + cjk_count as f64 / 1.67;
        if est >= target_f64 { return *i; }
    }
    0
}

/// Token-aware smart truncation: keeps head (70%) + tail (30%).
fn token_truncate(text: &str, max_tokens: u32) -> String {
    let total = deepx_types::token::count_tokens(text);
    if total <= max_tokens { return text.to_string(); }
    let head_tokens = (max_tokens as f64 * 0.7).max(1.0) as u32;
    let tail_tokens = (max_tokens as f64 * 0.3).max(1.0) as u32;
    let head_end = find_token_boundary(text, head_tokens);
    let tail_start = find_token_boundary_reverse(text, tail_tokens);
    if head_end >= tail_start {
        let end = find_token_boundary(text, max_tokens);
        format!("{}\n...[TRUNCATED: {}/{} tokens]", &text[..end], max_tokens, total)
    } else {
        let tail = &text[tail_start..];
        format!(
            "{}\n\n...[TRUNCATED: {}/{} tokens, {} lines dropped]\n\n{}",
            &text[..head_end], max_tokens, total,
            text[head_end..tail_start].lines().count(),
            tail.trim_start(),
        )
    }
}

/// Direct command execution: argv array, no shell.
/// Uses background threads for pipe reading and poll-based timeout.
fn direct_exec(argv: &[String], cwd: Option<&str>, max_output_tokens: u32, timeout_secs: u64) -> ExecOutput {
    let start_time = std::time::Instant::now();
    let display_name = if argv.len() > 1 { format!("{} ...", argv[0]) } else { argv[0].clone() };
    const HARD_BYTE_CAP: usize = 5 * 1024 * 1024;

    let mut cmd = std::process::Command::new(&argv[0]);
    if argv.len() > 1 { cmd.args(&argv[1..]); }
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(dir) = cwd { cmd.current_dir(dir); }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ExecOutput {
            status: "completed", command: display_name, exit_code: Some(-1),
            output: format!("SPAWN FAILED: {e}"), wall_time_seconds: 0.0,
            original_tokens: 0, truncated: false, timed_out: false,
        },
    };

    // Start background pipe readers
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel();
    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel();
    if let Some(p) = child.stdout.take() {
        std::thread::spawn(move || { let (s, t) = read_stream(p, HARD_BYTE_CAP); let _ = stdout_tx.send((s, t)); });
    } else { let _ = stdout_tx.send((String::new(), false)); }
    if let Some(p) = child.stderr.take() {
        std::thread::spawn(move || { let (s, t) = read_stream(p, HARD_BYTE_CAP); let _ = stderr_tx.send((s, t)); });
    } else { let _ = stderr_tx.send((String::new(), false)); }

    // Poll child with timeout
    let deadline = start_time + std::time::Duration::from_secs(timeout_secs);
    let mut exit_code: Option<i32> = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => { exit_code = status.code(); break; }
            Ok(None) => {
                if std::time::Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); timed_out = true; break; }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }

    // Collect pipe output (threads finish after child exits)
    let (stdout_out, stdout_trunc) = stdout_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_else(|_| {
        (String::from("[WARN] stdout pipe timed out\n"), true)
    });
    let (stderr_out, stderr_trunc) = stderr_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_else(|_| {
        (String::from("[WARN] stderr pipe timed out\n"), true)
    });

    let mut combined = String::new();
    if !stderr_out.is_empty() { combined.push_str(&stderr_out); if !stdout_out.is_empty() { combined.push('\n'); } }
    combined.push_str(&stdout_out);

    let hard_trunc = stderr_trunc || stdout_trunc;
    let cleaned = strip_ansi(&combined);
    let total_tokens = deepx_types::token::count_tokens(&cleaned);
    let (output_str, truncated) = if total_tokens > max_output_tokens || hard_trunc {
        (token_truncate(&cleaned, max_output_tokens), true)
    } else {
        (cleaned, false)
    };

    ExecOutput {
        status: "completed", command: display_name, exit_code,
        output: output_str, wall_time_seconds: start_time.elapsed().as_secs_f64(),
        original_tokens: total_tokens, truncated, timed_out,
    }
}

/// Structured output from a command execution.
#[derive(Serialize, Debug, Clone)]
pub(crate) struct ExecOutput {
    status: &'static str,
    command: String,
    exit_code: Option<i32>,
    output: String,
    wall_time_seconds: f64,
    original_tokens: u32,
    truncated: bool,
    timed_out: bool,
}

impl ExecOutput {
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"status":"error","output":"serialization failed"}"#.into())
    }
}

// ── Tool handler ──

pub(super) fn handle_run(ctx: ToolCallCtx) -> ToolResult {
    let argv: Vec<String> = match ctx.args.get("argv").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        None => {
            return ToolResult { success: false, content: crate::json_err("MISSING_ARGV", "exec_run requires an argv array", "Example: [\"cargo\", \"check\"]") };
        }
    };
    if argv.is_empty() {
        return ToolResult { success: false, content: crate::json_err("EMPTY_ARGV", "argv array is empty", "Provide at least one element.") };
    }
    let max_output_tokens = ctx.get_u64("max_output_tokens").filter(|&n| n >= 100 && n <= 50000).unwrap_or(10000) as u32;
    let timeout_secs = ctx.get_u64("timeout_secs").filter(|&n| n > 0 && n <= 3600).unwrap_or(30);
    // Fall back to workspace root when the caller doesn't supply cwd
    let cwd: Option<String> = ctx.get_str("cwd").map(String::from).or_else(|| {
        let ws = crate::CURRENT_WORKSPACE.read().ok()?;
        if ws.is_empty() || *ws == "." { None } else { Some(ws.clone()) }
    });
    let cwd_ref: Option<&str> = cwd.as_deref();
    let result = direct_exec(&argv, cwd_ref, max_output_tokens, timeout_secs);
    let success = match result.exit_code {
        Some(0) => true,
        Some(_) => false,
        None => !result.timed_out, // killed by timeout / spawn failed is already a signal
    };
    ToolResult { success, content: result.to_json() }
}

// ── Output helpers ──

/// Strip ANSI escape sequences from output.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i += 1;
            if i >= bytes.len() { break; }
            match bytes[i] {
                b'[' => { i += 1; while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) { i += 1; } if i < bytes.len() { i += 1; } }
                b']' | b'P' | b'_' | b'^' => { i += 1; while i < bytes.len() { if bytes[i] == 0x07 { i += 1; break; } if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' { i += 2; break; } i += 1; } }
                _ => {}
            }
        } else { out.push(bytes[i] as char); i += 1; }
    }
    out
}

// ── Registration ──

use crate::{ToolHandler, ToolRisk};
use std::time::Duration;

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: "exec_run".to_string(),
        description: "Execute a command. Pass {\"argv\": [\"program\", \"arg1\", \"arg2\", ...]}. The first element is the executable, the rest are arguments. No shell — for shell builtins use cmd /c or pwsh -Command as the executable. For pipes or redirects, write a script file and run it. Returns {\"status\": \"completed\", \"exit_code\": 0, \"output\": \"...\", \"wall_time_seconds\": 0.5, \"timed_out\": false}",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "argv": { "type": "array", "items": {"type": "string"}, "description": "Command as array of strings. argv[0]=executable, argv[1..]=args. Example: [\"cargo\",\"check\"]" },
                "cwd": {"type": "string", "description": "Working directory (optional). Defaults to workspace root."},
                "timeout_secs": {"type": "integer", "description": "Timeout in seconds (1-3600, default 30)"},
                "max_output_tokens": { "type": "integer", "description": "Max tokens of output before smart truncation (head 70% + tail 30%). Default 10000, min 100, max 50000." }
            },
            "required": ["argv"],
            "additionalProperties": false
        }),
        handler: handle_run,
        risk: ToolRisk::Destructive,
        default_timeout: Duration::from_secs(300),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_status_returns_output() {
        let argv = vec!["git".to_string(), "status".to_string()];
        let result = direct_exec(&argv, None, 10000, 10);
        eprintln!("exit_code={:?} timed_out={} time={:.3}s tokens={}",
            result.exit_code, result.timed_out, result.wall_time_seconds, result.original_tokens);
        assert!(!result.timed_out, "timed out");
        assert!(!result.output.is_empty(), "no output");
    }

    #[test]
    fn test_git_diff_returns_output() {
        let argv = vec!["git".to_string(), "diff".to_string(), "--stat".to_string()];
        let result = direct_exec(&argv, None, 10000, 10);
        eprintln!("exit_code={:?} timed_out={} time={:.3}s tokens={}",
            result.exit_code, result.timed_out, result.wall_time_seconds, result.original_tokens);
        assert!(!result.timed_out, "timed out");
    }

    #[test]
    fn test_cargo_check_returns_output() {
        let argv = vec!["cargo".to_string(), "check".to_string(), "-p".to_string(), "deepx-types".to_string()];
        let result = direct_exec(&argv, None, 10000, 60);
        eprintln!("exit_code={:?} timed_out={} time={:.3}s tokens={}",
            result.exit_code, result.timed_out, result.wall_time_seconds, result.original_tokens);
        assert!(!result.timed_out, "timed out");
        assert!(!result.output.is_empty(), "no output");
    }
}
