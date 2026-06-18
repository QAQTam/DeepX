//! Command execution via PTY (pseudo-terminal).
//!
//! Windows: `conpty` (CreatePseudoConsole API) via `pwsh -Command`.
//! Unix: `libc::forkpty` via `bash -c` or `sh -c`.
//!
//! Output is read by a background thread and streamed through a channel,
//! preserving cancel/timeout responsiveness. PTY provides proper terminal
//! semantics: ANSI colors, `isatty()`=true for the child process.
//!
//! 安全检测逻辑由 safety.rs 集中管理。

use std::io::{BufRead, BufReader};

use crate::{ToolCallCtx, ToolResult};
use std::sync::mpsc;

pub fn exec_command(args: &str, tool_call_id: &str, progress_tx: Option<mpsc::Sender<(String, String)>>) -> String {
    const MAX_EXEC_OUTPUT: usize = 1024 * 1024;
    let command = crate::parse_arg(args, "command");
    if command.trim().is_empty() {
        return "[ERROR] exec: empty command\n[HINT] Provide a shell command in the `cmd` or `command` parameter.".into();
    }
    let cwd = parse_opt(args, "cwd");
    let timeout_secs = parse_opt_u64(args, "timeout_secs")
        .filter(|&n| n > 0 && n <= 3600)
        .unwrap_or(30);

    // ── Spawn via PTY ──
    log::info!("[EXEC] spawn start, has_progress_tx={}", progress_tx.is_some());
    let mut proc = match crate::pty::spawn(&command, cwd.as_deref()) {
        Ok(p) => p,
        Err(e) => return format!("[ERROR] exec '{}' failed to start\n[HINT] {}", command, e),
    };
    let pid = proc.pid();

    // ── Reader thread: PTY output → channel ──
    let reader = match proc.take_output() {
        Some(r) => r,
        None => return format!("[ERROR] exec '{}' no output pipe", command),
    };

    let pt_out = progress_tx.clone();
    let has_progress = pt_out.is_some();
    log::info!("[EXEC] reader thread starting, has_progress={}", has_progress);
    let tc_id = tool_call_id.to_string();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let done_tx_thread = done_tx.clone();

    let _reader_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        let mut line_count = 0u32;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    line_count += 1;
                    if let Some(ref tx) = pt_out {
                        let _ = tx.send((tc_id.clone(), line.clone()));
                    }
                    let _ = done_tx_thread.send(line.clone());
                }
            }
        }
        log::info!("[EXEC] reader thread done, {} lines", line_count);
    });
    drop(done_tx);

    // ── Main loop: timeout + cancel + collect ──
    let mut output_buf = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    let exit_reason = loop {
        use std::sync::atomic::Ordering;
        let remaining = deadline.checked_duration_since(std::time::Instant::now()).unwrap_or_default();

        // Cancel
        if crate::CANCEL.load(Ordering::SeqCst) {
            let _ = proc.kill();
            return "[CANCELLED] Command execution cancelled by user.".into();
        }

        // Timeout
        if remaining.is_zero() {
            let _ = proc.kill();
            return format!("[ERROR] exec timed out after {}s\n[HINT] Increase timeout_secs or check if the command is stuck.", timeout_secs);
        }

        // Read output chunk
        match done_rx.recv_timeout(remaining.min(std::time::Duration::from_millis(200))) {
            Ok(chunk) => {
                output_buf.push_str(&chunk);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !proc.is_alive() {
                    break "process_exited";
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break "reader_disconnected";
            }
        }
    };
    log::info!("[EXEC] main loop exit: {}", exit_reason);

    // Ensure PTY is fully closed so the reader thread unblocks.
    // On Windows conpty, the output pipe may not close automatically when
    // the child process exits. Explicitly dropping proc triggers
    // conpty::Process::Drop → ClosePseudoConsole, which closes the pipe.
    drop(proc);
    // _reader_handle will be dropped on return (detaching the thread).
    // The reader thread will exit once the PTY pipe closes (now closed).

    // Final drain
    while let Ok(chunk) = done_rx.try_recv() {
        output_buf.push_str(&chunk);
    }

    // ── Format output ──
    let output = if output_buf.len() > MAX_EXEC_OUTPUT {
        output_buf[..output_buf.floor_char_boundary(MAX_EXEC_OUTPUT)].to_string()
            + &format!("...[TRUNCATED: {} bytes total]", output_buf.len())
    } else {
        output_buf.clone()
    };

    let output_trimmed = output.trim();
    let short_output = if output_trimmed.len() > 2000 {
        let head: String = output_trimmed.chars().take(1000).collect();
        let tail: String = output_trimmed.chars().rev().take(500).collect::<String>().chars().rev().collect();
        format!("{head}\n...({} bytes total)...\n{tail}", output_buf.len())
    } else {
        output_trimmed.to_string()
    };

    let mut result = format!("[OK] exec: {} (pid {})\n", command, pid);
    if short_output.is_empty() {
        result.push_str("(no output)");
    } else {
        result.push_str(&short_output);
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
    let result = exec_command(&args.to_string(), &ctx.id, ctx.tx_progress);
    let success = result.starts_with("[OK]");
    ToolResult { success, content: result }
}


use deepx_types::arg::{parse_opt, parse_opt_u64};

// ── 注册入口 ──

use crate::{ToolHandler, ToolKey};
use std::time::Duration;

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
