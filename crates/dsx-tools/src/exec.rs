//! Command execution via shell.
//!
//! 安全检测逻辑由 safety.rs 集中管理。

use std::process::{Command, Stdio};

use crate::{ToolCallCtx, ToolResult};

// ── Compat helpers ──

pub fn exec_command(args: &str) -> String {
    let command = crate::parse_arg(args, "command");
    if command.trim().is_empty() {
        return "[ERROR] exec: empty command\n[HINT] Provide a shell command in the `cmd` or `command` parameter.".into();
    }
    let cwd = parse_opt(args, "cwd");
    let timeout_secs = parse_opt_u64(args, "timeout_secs")
        .filter(|&n| n > 0 && n <= 3600)
        .unwrap_or(30);

    let mut cmd = if cfg!(target_os = "windows") {
        // Prefer pwsh (PowerShell 7) > powershell (5.1) > cmd
        if which("pwsh.exe") {
            let encoded = format!("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;$OutputEncoding=[System.Text.UTF8Encoding]::new();{}", command);
            let mut c = Command::new("pwsh");
            c.args(["-NoLogo", "-NonInteractive", "-Command", &encoded])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c
        } else if which("powershell.exe") {
            let encoded = format!("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;$OutputEncoding=[System.Text.UTF8Encoding]::new();{}", command);
            let mut c = Command::new("powershell");
            c.args(["-NoLogo", "-NonInteractive", "-Command", &encoded])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c
        } else {
            let mut c = Command::new("cmd");
            c.args(["/C", &command])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c
        }
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &command])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    };

    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] exec '{}' failed to start\n[HINT] {}", command, e),
    };
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    use std::sync::atomic::Ordering;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let output = loop {
        if crate::CANCEL.load(Ordering::SeqCst) {
            dsx_types::platform::kill_process(pid);
            return "[CANCELLED] Command execution cancelled by user.".into();
        }
        let remaining = deadline.checked_duration_since(std::time::Instant::now()).unwrap_or_default();
        if remaining.is_zero() {
            dsx_types::platform::kill_process(pid);
            return format!("[ERROR] exec timed out after {}s\n[HINT] Increase timeout_secs or check if the command is stuck.", timeout_secs);
        }
        let poll = remaining.min(std::time::Duration::from_secs(1));
        match rx.recv_timeout(poll) {
            Ok(Ok(o)) => break o,
            Ok(Err(e)) => return format!("[ERROR] exec wait failed: {}", e),
            Err(_) => continue, // poll tick — check cancel next iteration
        }
    };

    const MAX_EXEC_OUTPUT: usize = 256 * 1024;
    let stdout = if output.stdout.len() > MAX_EXEC_OUTPUT {
        format!("{}...[TRUNCATED: {} bytes total]", String::from_utf8_lossy(&output.stdout[..MAX_EXEC_OUTPUT]), output.stdout.len())
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    let stderr = if output.stderr.len() > MAX_EXEC_OUTPUT {
        format!("{}...[TRUNCATED: {} bytes total]", String::from_utf8_lossy(&output.stderr[..MAX_EXEC_OUTPUT]), output.stderr.len())
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };
    let full_output = format!("[stdout]\n{}\n[stderr]\n{}", stdout.trim_end(), stderr.trim_end());
    let full_output = full_output.trim();
    let exit_code = output.status.code().unwrap_or(-1);
    let status = if exit_code == 0 { "OK" } else { "FAIL" };
    let mut result = format!("[{}] exec: {} (exit {})\n", status, command, exit_code);

    if full_output.is_empty() {
        result.push_str("(no output)");
        return result;
    }

    let lines: Vec<&str> = full_output.lines().collect();
    let total = lines.len();

    // Adaptive limits by command type
    let (head_count, tail_count) = if command.contains("make ") || command.contains("cargo build") || command.contains("cargo check") {
        (120, 80)
    } else if command.contains("cargo test") || command.contains("go test") || command.contains("pytest") {
        (20, 200)
    } else if command.contains("grep") || command.contains("find ") || command.contains("ls ") {
        (200, 30)
    } else {
        (80, 40)
    };

    // Error indices for context display
    let error_indices: Vec<usize> = lines.iter().enumerate()
        .filter(|(_, l)| l.contains("error") || l.contains("Error") || l.contains("ERROR")
            || l.contains("fail") || l.contains("FAIL"))
        .map(|(i, _)| i)
        .collect();
    let warning_indices: Vec<usize> = lines.iter().enumerate()
        .filter(|(_, l)| l.contains("warning") || l.contains("Warning") || l.contains("WARN"))
        .map(|(i, _)| i)
        .collect();

    if total > head_count + tail_count {
        for &l in &lines[..head_count] {
            result.push_str(l);
            result.push('\n');
        }
        if head_count + tail_count < total {
            let mid = &lines[head_count..total.saturating_sub(tail_count)];
            let compressed = compress_lines(mid);
            result.push_str(&compressed);
        }
        if tail_count > 0 {
            for &l in &lines[total.saturating_sub(tail_count)..] {
                result.push_str(l);
                result.push('\n');
            }
        }
        append_error_summary(&mut result, &lines, &error_indices, total, true);
        append_warning_summary(&mut result, &lines, &warning_indices, true);
        append_exit_footer(&mut result, exit_code);
        result
    } else {
        for &l in &lines {
            result.push_str(l);
            result.push('\n');
        }
        append_error_summary(&mut result, &lines, &error_indices, total, false);
        append_warning_summary(&mut result, &lines, &warning_indices, false);
        append_exit_footer(&mut result, exit_code);
        result
    }
}

// ── Handler ──

pub(super) fn handle_run(ctx: ToolCallCtx) -> ToolResult {
    let command = ctx.get_str("command").unwrap_or("").to_string();

    let args = serde_json::json!({
        "command": command,
        "cwd": ctx.get_str("cwd"),
        "timeout_secs": ctx.get_u64("timeout_secs"),
    });
    let result = exec_command(&args.to_string());
    let success = result.starts_with("[OK]");
    ToolResult { success, content: result }
}

// ── 辅助函数 ──

pub(super) fn compress_lines(lines: &[&str]) -> String {
    if lines.is_empty() { return String::new(); }
    let mut out = String::new();
    let mut group_start = 0;
    for i in 1..=lines.len() {
        let same = if i < lines.len() { same_prefix(lines[group_start], lines[i]) } else { false };
        if !same {
            let count = i - group_start;
            if count > 3 {
                let prefix = common_prefix(&lines[group_start..i]);
                out.push_str(&format!("  ... ({} similar lines) ... {}\n", count, prefix.trim()));
            } else {
                for &l in &lines[group_start..i] {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            group_start = i;
        }
    }
    out
}

fn same_prefix(a: &str, b: &str) -> bool {
    let a_trim = a.trim_start();
    let b_trim = b.trim_start();
    if a_trim.len() < 10 || b_trim.len() < 10 { return false; }
    let len = 40.min(a_trim.len()).min(b_trim.len());
    match (a_trim.get(..len), b_trim.get(..len)) {
        (Some(a_pref), Some(b_pref)) => a_pref == b_pref,
        _ => false,
    }
}

fn common_prefix(lines: &[&str]) -> String {
    if lines.is_empty() { return String::new(); }
    let first = lines[0].trim_start();
    let mut prefix_len = first.len().min(60);
    for &l in &lines[1..] {
        let trimmed = l.trim_start();
        for (i, (a, b)) in first.chars().zip(trimmed.chars()).enumerate() {
            if a != b {
                prefix_len = prefix_len.min(i);
                break;
            }
        }
    }
    first.chars().take(prefix_len).collect()
}

// ── Output summary helpers (shared by truncated and full output paths) ──

fn append_error_summary(result: &mut String, lines: &[&str], error_indices: &[usize], total: usize, truncated: bool) {
    if error_indices.is_empty() {
        return;
    }
    result.push_str("\u{2500}\u{2500} errors \u{2500}\u{2500}\n");
    let mut last = 0usize;
    for &ei in error_indices.iter().take(20) {
        let start = ei.saturating_sub(2);
        if start <= last { continue; }
        if truncated && ei > 2 && ei != error_indices[0] {
            result.push_str("\u{250a}\n");
        }
        for li in start..=ei.saturating_add(2).min(total - 1) {
            result.push_str(lines[li]);
            result.push('\n');
        }
        last = ei.saturating_add(2);
    }
    if truncated && error_indices.len() > 20 {
        result.push_str(&format!("... ({} more errors)\n", error_indices.len() - 20));
    }
}

fn append_warning_summary(result: &mut String, lines: &[&str], warning_indices: &[usize], truncated: bool) {
    if warning_indices.is_empty() {
        return;
    }
    result.push_str("\u{2500}\u{2500} warnings \u{2500}\u{2500}\n");
    for &wi in warning_indices.iter().take(10) {
        result.push_str(lines[wi]);
        result.push('\n');
    }
    if truncated && warning_indices.len() > 10 {
        result.push_str(&format!("... ({} more warnings)\n", warning_indices.len() - 10));
    }
}

fn append_exit_footer(result: &mut String, exit_code: i32) {
    if exit_code != 0 {
        result.push_str(&format!("\u{2500}\u{2500} exit: {} \u{2500}\u{2500}\n", exit_code));
    }
}

// ── 参数解析（委托至 dsx-types）──

use dsx_types::arg::{parse_opt, parse_opt_u64};

// ── 注册入口 ──

use crate::{ToolHandler, ToolKey};
use std::time::Duration;

fn which(name: &str) -> bool {
    if cfg!(target_os = "windows") {
        Command::new("where")
            .args([name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("which")
            .args([name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub fn register(mgr: &mut crate::ToolManager) {
    // exec/run
    mgr.register(ToolHandler {
        key: ToolKey::new("exec", "run"),
        description: "Execute a shell command synchronously. Supports timeout_secs and cwd.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command"},
                "cwd": {"type": "string", "description": "Working directory for the command"},
                "timeout_secs": {"type": "integer", "description": "Max execution time in seconds (1-3600, default 30)"}
            },
            "required": ["command"],
            "additionalProperties": false
        }),
        handler: handle_run,
        safety: |ctx| {
            let cmd = ctx.get_str("command").unwrap_or("");
            crate::safety::classify_execution(cmd)
        },
        default_timeout: Duration::from_secs(300),
    });

}
