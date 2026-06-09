//! Command execution via shell.
//!
//! 安全检测逻辑由 safety.rs 集中管理。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use crate::{ToolCallCtx, ToolResult};
use std::sync::mpsc;

// ── Compat helpers ──

pub fn exec_command(args: &str, progress_tx: Option<mpsc::Sender<String>>) -> String {
    const MAX_EXEC_OUTPUT: usize = 1024 * 1024;
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
        // Prefer bash -i (reads .bashrc for nvm/fnm/rbenv etc)
        let mut c = if which("bash") {
            let mut c = Command::new("bash");
            c.args(["-i", "-c", &command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &command]);
            c
        };
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    };

    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] exec '{}' failed to start\n[HINT] {}", command, e),
    };
    let pid = child.id();

    let stdout_reader = child.stdout.take().map(BufReader::new);
    let stderr_reader = child.stderr.take().map(BufReader::new);

    let pt_out = progress_tx.clone();
    let pt_err = progress_tx.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let mut output_buf = String::new();

    if let Some(reader) = stdout_reader {
        let done_tx = done_tx.clone();
        std::thread::spawn(move || {
            for line in reader.lines() {
                if let Ok(l) = line {
                    let text = format!("{l}\n");
                    if let Some(ref tx) = pt_out { let _ = tx.send(text.clone()); }
                    let _ = done_tx.send(text);
                }
            }
        });
    }
    if let Some(reader) = stderr_reader {
        let done_tx = done_tx.clone();
        std::thread::spawn(move || {
            for line in reader.lines() {
                if let Ok(l) = line {
                    let text = format!("[stderr] {l}\n");
                    if let Some(ref tx) = pt_err { let _ = tx.send(text.clone()); }
                    let _ = done_tx.send(text);
                }
            }
        });
    }
    // signal no more output after streams close
    drop(done_tx);

    use std::sync::atomic::Ordering;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let exit_status = loop {
        if crate::CANCEL.load(Ordering::SeqCst) {
            deepx_types::platform::kill_process(pid);
            return "[CANCELLED] Command execution cancelled by user.".into();
        }
        let remaining = deadline.checked_duration_since(std::time::Instant::now()).unwrap_or_default();
        if remaining.is_zero() {
            deepx_types::platform::kill_process(pid);
            return format!("[ERROR] exec timed out after {}s\n[HINT] Increase timeout_secs or check if the command is stuck.", timeout_secs);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // drain remaining output
                while let Ok(chunk) = done_rx.recv() { output_buf.push_str(&chunk); }
                break status;
            }
            Ok(None) => {
                match done_rx.recv_timeout(remaining.min(std::time::Duration::from_millis(200))) {
                    Ok(chunk) => output_buf.push_str(&chunk),
                    Err(_) => continue,
                }
            }
            Err(e) => return format!("[ERROR] exec wait failed: {}", e),
        }
    };

    let exit_code = exit_status.code().unwrap_or(-1);
    let status = if exit_code == 0 { "OK" } else { "FAIL" };
    let output = if output_buf.len() > MAX_EXEC_OUTPUT {
        output_buf[..MAX_EXEC_OUTPUT].to_string() + &format!("...[TRUNCATED: {} bytes total]", output_buf.len())
    } else {
        output_buf.clone()
    };
    let output = output.trim();
    let mut result = format!("[{}] exec: {} (exit {})\n", status, command, exit_code);
    if output.is_empty() {
        result.push_str("(no output)");
    } else {
        result.push_str(output);
    }
    result
}

// ── Handler ──

pub(super) fn handle_run(ctx: ToolCallCtx) -> ToolResult {
    let command = ctx.get_str("command").unwrap_or("").to_string();

    let args = serde_json::json!({
        "command": command,
        "cwd": ctx.get_str("cwd"),
        "timeout_secs": ctx.get_u64("timeout_secs"),
    });
    let result = exec_command(&args.to_string(), ctx.tx_progress);
    let success = result.starts_with("[OK]");
    ToolResult { success, content: result }
}


use deepx_types::arg::{parse_opt, parse_opt_u64};

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
