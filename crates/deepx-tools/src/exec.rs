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
    let start_time = std::time::Instant::now();
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
        Err(e) => return format!("[ERROR] {}   0s   0 bytes [SPAWN FAILED: {}]", command, e),
    };

    // ── Reader thread: PTY output → channel ──
    let reader = match proc.take_output() {
        Some(r) => r,
        None => return format!("[ERROR] {}   0s   0 bytes [NO PIPE]", command),
    };

    // Register in process registry BEFORE starting (so it's findable on timeout)
    let registry_id = crate::process_registry::ProcessRegistry::register(
        &format!("exec:{}", &command[..command.len().min(30)])
    );

    let pt_out = progress_tx.clone();
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
                    // Also write to registry
                    crate::process_registry::ProcessRegistry::append_output(registry_id, &line);
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

    let _exit_reason = loop {
        use std::sync::atomic::Ordering;
        let remaining = deadline.checked_duration_since(std::time::Instant::now()).unwrap_or_default();

        // Cancel
        if crate::CANCEL.load(Ordering::SeqCst) {
            let _ = proc.kill();
            crate::process_registry::ProcessRegistry::kill(registry_id);
            let elapsed = start_time.elapsed();
            let n = output_buf.len();
            return format!("[CANCELLED] {}   {:.1}s   {} bytes [CANCELLED]{}",
                command, elapsed.as_secs_f64(), n,
                if n > 0 { format!("\n{}", output_buf.trim()) } else { String::new() });
        }

        // Timeout — register and keep alive instead of killing
        if remaining.is_zero() {
            // Move proc to background watcher thread
            std::thread::spawn(move || {
                let exit = proc.wait(None).ok();
                if let Some(es) = exit {
                    crate::process_registry::ProcessRegistry::mark_exited(registry_id, es.code());
                } else {
                    crate::process_registry::ProcessRegistry::mark_exited(registry_id, -1);
                }
                log::info!("[EXEC] background watcher done for pid={}", registry_id);
            });

            let elapsed = start_time.elapsed();
            let n = output_buf.len();
            let output = truncate_1mb(&output_buf, MAX_EXEC_OUTPUT);
            return format!(
                "[TIMEOUT] {}   {:.1}s   {} bytes   process_id={}\n{}\n\
                 [HINT] Process still running. Use check_process({}) to inspect, \
                 wait_process({}) to wait longer, kill_process({}) to terminate.",
                command, elapsed.as_secs_f64(), n, registry_id,
                if output.is_empty() { "(no output yet)" } else { &output },
                registry_id, registry_id, registry_id,
            );
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

    // ── Normal exit: try to capture exit status, then drain ──
    // On Windows (conpty), wait() always returns code=0; on Unix, we get the real exit code.
    let exit_status = proc.wait(Some(500)).ok();
    if let Some(ref es) = exit_status {
        crate::process_registry::ProcessRegistry::mark_exited(registry_id, es.code());
    } else {
        crate::process_registry::ProcessRegistry::mark_exited(registry_id, 0);
    }
    drop(proc);

    // Final drain of any remaining lines
    while let Ok(chunk) = done_rx.try_recv() {
        output_buf.push_str(&chunk);
    }

    // Strip ANSI escape sequences — PTY output includes terminal control codes
    // that are meaningless noise to the model (e.g. \x1b[?9001h, \x1b[2J).
    let output_buf = strip_ansi(&output_buf);

    // ── Format result ──
    let elapsed = start_time.elapsed();
    let total_bytes = output_buf.len();
    let output = truncate_1mb(&output_buf, MAX_EXEC_OUTPUT);
    let shown_bytes = output.len();
    let truncated = total_bytes > MAX_EXEC_OUTPUT;

    // Build first line: [OK] command   elapsed   bytes [TAGS]
    let mut headline = format!(
        "[OK] {}   {:.1}s   {} bytes",
        command, elapsed.as_secs_f64(),
        if truncated { format!("{}/{}", shown_bytes, total_bytes) } else { shown_bytes.to_string() },
    );
    if let Some(ref es) = exit_status {
        if es.code() != 0 {
            headline.push_str(&format!(" [EXIT:{}]", es.code()));
        }
    }
    if truncated {
        headline.push_str(" [TRUNCATED]");
    }
    if output.trim().is_empty() {
        headline.push_str(" [NO OUTPUT]");
        return headline;
    }

    format!("{}\n{}", headline, output.trim())
}

/// Truncate output to MAX_EXEC_OUTPUT bytes at a char boundary, appending a truncation note.
fn truncate_1mb(buf: &str, max: usize) -> String {
    if buf.len() <= max {
        return buf.to_string();
    }
    let boundary = buf.floor_char_boundary(max);
    let mut s = buf[..boundary].to_string();
    s.push_str(&format!("\n...[TRUNCATED: {}/{} bytes shown]", boundary, buf.len()));
    s
}

/// Strip ANSI escape sequences from PTY output.
///
/// PTY spawn preserves terminal control codes for the TUI's live display,
/// but they are meaningless noise for the LLM. This strips SGR (colors),
/// cursor movement, screen clearing, DEC private modes, and other CSI sequences.
fn strip_ansi(s: &str) -> String {
    // CSI sequences: ESC [ ... final byte (0x40–0x7E)
    // Also handles OSC (ESC ]), DCS (ESC P), and stray ESC not followed by [
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'[' => {
                    // CSI: skip until final byte 0x40–0x7E
                    i += 1;
                    while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1; // skip the final byte
                    }
                }
                b']' | b'P' | b'_' | b'^' => {
                    // OSC / DCS / APC / PM: skip until ST (ESC \) or BEL
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Lone ESC or other escape — skip just this byte
                }
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
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
