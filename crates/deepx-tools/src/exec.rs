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

use crate::{ToolCallCtx, ToolResult};
use serde::Serialize;
use std::sync::mpsc;

/// Structured output from a command execution.
#[derive(Serialize, Debug, Clone)]
pub(crate) struct ExecOutput {
    status: &'static str,
    command: String,
    exit_code: Option<i32>,
    output: String,
    wall_time_seconds: f64,
    original_bytes: usize,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_id: Option<u64>,
}

impl ExecOutput {
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"status":"error","output":"serialization failed"}"#.into())
    }
}

pub(crate) fn exec_command(args: &str, tool_call_id: &str, progress_tx: Option<mpsc::Sender<(String, String)>>) -> ExecOutput {
    let max_output_tokens = parse_opt_u64(args, "max_output_tokens")
        .filter(|&n| n >= 1000 && n <= 50000)
        .unwrap_or(10000) as usize;
    // Rough estimate: 1 token ≈ 4 bytes (UTF-8 English average)
    let max_output_bytes = (max_output_tokens * 4).min(1024 * 1024);
    let start_time = std::time::Instant::now();
    let command = crate::parse_arg(args, "command");
    if command.trim().is_empty() {
        return ExecOutput {
            status: "error", command: String::new(), exit_code: None,
            output: "empty command".into(), wall_time_seconds: 0.0,
            original_bytes: 0, truncated: false, process_id: None,
        };
    }
    crate::audit::maybe_log_exec(&command);
    let cwd = parse_opt(args, "cwd");
    let timeout_secs = parse_opt_u64(args, "timeout_secs")
        .filter(|&n| n > 0 && n <= 3600)
        .unwrap_or(30);

    // ── Spawn via PTY ──
    log::info!("[EXEC] spawn start, has_progress_tx={}", progress_tx.is_some());
    let mut proc = match crate::pty::spawn(&command, cwd.as_deref()) {
        Ok(p) => p,
        Err(e) => return ExecOutput {
            status: "error", command, exit_code: None,
            output: format!("SPAWN FAILED: {}", e), wall_time_seconds: 0.0,
            original_bytes: 0, truncated: false, process_id: None,
        },
    };

    // ── Reader thread: PTY output → channel ──
    let reader = match proc.take_output() {
        Some(r) => r,
        None => return ExecOutput {
            status: "error", command, exit_code: None,
            output: "NO PIPE".into(), wall_time_seconds: 0.0,
            original_bytes: 0, truncated: false, process_id: None,
        },
    };

    // Register in process registry BEFORE starting (so it's findable on timeout)
    let registry_id = crate::process_registry::ProcessRegistry::register(
        &format!("exec:{}", &command[..command.floor_char_boundary(command.len().min(30))])
    );

    let pt_out = progress_tx.clone();
    let tc_id = tool_call_id.to_string();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let done_tx_thread = done_tx.clone();

    let _reader_handle = std::thread::spawn(move || {
        use std::io::Read;
        let mut reader = reader;
        let mut buf = vec![0u8; 4096];
        let mut pending = String::new();
        let mut partial = Vec::new();
        let mut last_flush = std::time::Instant::now();
        const FLUSH_MS: u64 = 50;
        const FLUSH_BYTES: usize = 512;

        let flush_now = |pending: &mut String, tc_id: &str,
                         pt_out: &Option<mpsc::Sender<(String, String)>>,
                         done: &mpsc::Sender<String>| {
            if !pending.is_empty() {
                if let Some(tx) = pt_out {
                    let _ = tx.send((tc_id.to_string(), pending.clone()));
                }
                crate::process_registry::ProcessRegistry::append_output(registry_id, pending);
                let _ = done.send(pending.clone());
                pending.clear();
            }
        };

        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    if !partial.is_empty() {
                        pending.push_str(&String::from_utf8_lossy(&partial));
                    }
                    if !pending.is_empty() {
                        if let Some(tx) = pt_out {
                            let _ = tx.send((tc_id.clone(), pending.clone()));
                        }
                        crate::process_registry::ProcessRegistry::append_output(registry_id, &pending);
                        let _ = done_tx_thread.send(std::mem::take(&mut pending));
                    }
                    break;
                }
                Ok(n) => {
                    let chunk_bytes: Vec<u8> = if partial.is_empty() {
                        buf[..n].to_vec()
                    } else {
                        let mut merged = std::mem::take(&mut partial);
                        merged.extend_from_slice(&buf[..n]);
                        merged
                    };
                    match String::from_utf8(chunk_bytes.clone()) {
                        Ok(clean) => pending.push_str(&clean),
                        Err(utf8_err) => {
                            let valid_len = utf8_err.utf8_error().valid_up_to();
                            partial = chunk_bytes[valid_len..].to_vec();
                            if let Ok(s) = String::from_utf8(chunk_bytes[..valid_len].to_vec()) {
                                pending.push_str(&s);
                            }
                        }
                    }
                    let elapsed = last_flush.elapsed().as_millis() as u64;
                    if elapsed >= FLUSH_MS || pending.len() >= FLUSH_BYTES {
                        flush_now(&mut pending, &tc_id, &pt_out, &done_tx_thread);
                        last_flush = std::time::Instant::now();
                    }
                }
                Err(_) => {
                    if !pending.is_empty() {
                        if let Some(tx) = pt_out {
                            let _ = tx.send((tc_id.clone(), pending.clone()));
                        }
                        crate::process_registry::ProcessRegistry::append_output(registry_id, &pending);
                        let _ = done_tx_thread.send(std::mem::take(&mut pending));
                    }
                    break;
                }
            }
        }
        log::info!("[EXEC] reader thread done");
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
            return ExecOutput {
                status: "cancelled", command, exit_code: None,
                output: if n > 0 { output_buf.trim().to_string() } else { String::new() },
                wall_time_seconds: elapsed.as_secs_f64(),
                original_bytes: n, truncated: false, process_id: None,
            };
        }

        // Timeout — register and keep alive instead of killing
        if remaining.is_zero() {
            // Final drain: capture any output already in the channel
            while let Ok(chunk) = done_rx.try_recv() {
                output_buf.push_str(&chunk);
            }
            // Take stdin writer for interactive process support
            if let Some(writer) = proc.take_input() {
                crate::process_registry::ProcessRegistry::attach_writer(registry_id, writer);
            }
            // Detach so background Drop doesn't kill the process
            proc.detach();
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
            let output = truncate_output(&output_buf, max_output_bytes);
            let truncated = output.len() < n;
            return ExecOutput {
                status: "timeout", command, exit_code: None,
                output: if output.is_empty() { "(no output yet)".into() } else { output },
                wall_time_seconds: elapsed.as_secs_f64(),
                original_bytes: n, truncated, process_id: Some(registry_id as u64),
            };
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
    let output = truncate_output(&output_buf, max_output_bytes);
    let truncated = total_bytes > max_output_bytes;

    let exit_code = exit_status.as_ref().map(|es| es.code());

    ExecOutput {
        status: "ok", command, exit_code,
        output: if output.trim().is_empty() { String::new() } else { output.trim().to_string() },
        wall_time_seconds: elapsed.as_secs_f64(),
        original_bytes: total_bytes, truncated, process_id: None,
    }
}

/// Truncate output to max bytes at a char boundary, appending a truncation note.
fn truncate_output(buf: &str, max: usize) -> String {
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
            // Push a run of non-ESC bytes as a &str slice to preserve
            // multi-byte UTF-8.  Using `bytes[i] as char` would corrupt
            // CJK / emoji sequences by treating each byte as a separate
            // Unicode codepoint.
            let start = i;
            while i < bytes.len() && bytes[i] != 0x1b {
                i += 1;
            }
            out.push_str(&s[start..i]);
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
    let success = result.status == "ok";
    let json = result.to_json();
    ToolResult { success, content: json }
}


use deepx_types::arg::{parse_opt, parse_opt_u64};

// ── write_stdin handler ──

pub(super) fn handle_write_stdin(ctx: ToolCallCtx) -> ToolResult {
    let process_id = ctx.get_u64("process_id").unwrap_or(0) as u32;
    let input = ctx.get_str("input").unwrap_or("").to_string();
    let yield_ms = ctx.get_u64("yield_time_ms").unwrap_or(5000).min(30000);

    if process_id == 0 {
        return ToolResult::error("process_id is required");
    }

    // Clear accumulated output to capture fresh response
    crate::process_registry::ProcessRegistry::clear_output(process_id);

    // Write to stdin
    match crate::process_registry::ProcessRegistry::write_to(process_id, &input) {
        Ok(written) => {
            // Wait for the process to produce output
            std::thread::sleep(std::time::Duration::from_millis(yield_ms));

            let info = crate::process_registry::ProcessRegistry::get_info(process_id)
                .unwrap_or(serde_json::json!({"error": "process not found"}));

            let result = serde_json::json!({
                "status": "ok",
                "process_id": process_id,
                "bytes_written": written,
                "process": info,
            });
            ToolResult { success: true, content: result.to_string() }
        }
        Err(e) => {
            let info = crate::process_registry::ProcessRegistry::get_info(process_id)
                .unwrap_or(serde_json::json!({"error": "process not found"}));
            let result = serde_json::json!({
                "status": "error",
                "process_id": process_id,
                "error": e,
                "process": info,
            });
            ToolResult { success: false, content: result.to_string() }
        }
    }
}

// ── 注册入口 ──

use crate::{ToolHandler, ToolKey, ToolRisk};
use std::time::Duration;

pub fn register(mgr: &mut crate::ToolManager) {
    // exec/run
    let desc = if cfg!(windows) {
        "Execute a shell command synchronously. Returns JSON with status/exit_code/output/wall_time_seconds/truncated/process_id.\n\
         On Windows: prefer PowerShell native cmdlets (Remove-Item, Get-Content, Select-String, Test-Path). \
         Use `rg` for text search. `sed` is available via the `sed` tool, not via shell. \
         Never mix cmd and pwsh in a single pipeline."
    } else {
        "Execute a shell command synchronously. Returns JSON with status/exit_code/output/wall_time_seconds/truncated/process_id."
    };
    mgr.register(ToolHandler {
        key: ToolKey::new("exec", "run"),
        description: desc,
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to execute"},
                "cwd": {"type": "string", "description": "Working directory for the command"},
                "timeout_secs": {"type": "integer", "description": "Max execution time in seconds (1-3600, default 30)"},
                "max_output_tokens": {"type": "integer", "description": "Max output tokens before truncation (1000-50000, default 10000)"}
            },
            "required": ["command"],
            "additionalProperties": false
        }),
        handler: handle_run,
        risk: ToolRisk::Destructive,
        default_timeout: Duration::from_secs(300),
    });

    // exec/write_stdin
    mgr.register(ToolHandler {
        key: ToolKey::new("exec", "write_stdin"),
        description: "Write input to a running process's stdin and read subsequent output. \n\
                      Use after exec/run returns a process_id (timeout status) to interact with long-running processes.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "process_id": {"type": "integer", "description": "Process ID from exec/run timeout response"},
                "input": {"type": "string", "description": "Text to write to stdin (e.g. 'y\\n' to answer yes)"},
                "yield_time_ms": {"type": "integer", "description": "Wait time in ms before reading output (default 5000, max 30000)"}
            },
            "required": ["process_id", "input"],
            "additionalProperties": false
        }),
        handler: handle_write_stdin,
        risk: ToolRisk::Destructive,
        default_timeout: Duration::from_secs(60),
    });

}
