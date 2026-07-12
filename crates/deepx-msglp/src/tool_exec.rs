//! Tool execution engine for the message loop.
//!
//! Handles UI-initiated tool calls and LLM-initiated tool calls through
//! a unified permission→execute→emit pipeline.

use std::collections::HashMap;

use crate::Loop;
use deepx_proto::Agent2Ui;

/// Drain tool progress channel with batched emission (at most every 50ms).
/// Returns true if cancelled during drain.
pub(crate) fn drain_tool_progress(
    loop_ref: &mut Loop,
    progress_rx: std::sync::mpsc::Receiver<(String, String)>,
) -> bool {
    log::info!("[AGENT] drain loop start");
    let mut batches: HashMap<String, String> = HashMap::new();
    let batch_interval = std::time::Duration::from_millis(50);
    loop {
        if loop_ref.cancel.is_set() || deepx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            log::info!("[AGENT] drain loop cancel");
            return true;
        }
        match progress_rx.recv_timeout(batch_interval) {
            Ok((tc_id, chunk)) => {
                batches.entry(tc_id).or_default().push_str(&chunk);
                while let Ok((tid, c)) = progress_rx.try_recv() {
                    batches.entry(tid).or_default().push_str(&c);
                }
                for (tid, merged) in batches.drain() {
                    log::info!("[AGENT] ExecProgress batch: {} {} chars", tid, merged.len());
                    loop_ref.emit_delta(Agent2Ui::ExecProgress {
                        tool_call_id: tid,
                        chunk: merged,
                    });
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !batches.is_empty() {
                    for (tid, merged) in batches.drain() {
                        loop_ref.emit_delta(Agent2Ui::ExecProgress {
                            tool_call_id: tid,
                            chunk: merged,
                        });
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("[AGENT] drain loop disconnected");
                for (tid, merged) in batches.drain() {
                    loop_ref.emit_delta(Agent2Ui::ExecProgress {
                        tool_call_id: tid,
                        chunk: merged,
                    });
                }
                return false;
            }
        }
    }
}

/// Handle a UI-initiated tool call through the full permission→execute→emit pipeline.
pub(crate) fn handle_tool_call(
    loop_ref: &mut Loop,
    id: &str,
    name: &str,
    action: &str,
    args: &serde_json::Value,
) {
    log::info!("[AGENT] handle_tool_call: name={name} action={action} id={id}");

    if loop_ref.pending_approvals.contains_key(id) {
        loop_ref.emit(Agent2Ui::Error {
            message: format!("Duplicate or replayed tool-call ID rejected: {id}"),
        });
        return;
    }

    let effective_name = crate::util::resolve_effective_name(name, action, args);
    log::info!("[AGENT] resolved effective_name={effective_name}");

    let level =
        deepx_tools::permission::PermissionLevel::from_u8(loop_ref.agent.config.permission_level);
    let ws_root = {
        let ws = deepx_tools::CURRENT_WORKSPACE
            .read()
            .expect("CURRENT_WORKSPACE lock")
            .clone();
        if ws.is_empty() || ws == "." {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            std::path::PathBuf::from(ws)
        }
    };

    // Ensure the bridge permission context is set so compatibility wrappers
    // also enforce policy.
    deepx_tools::bridge::set_runtime_context(
        &loop_ref.agent.session.seed,
        loop_ref.agent.config.permission_level,
    );

    let inv = deepx_tools::bridge::ToolInvocation {
        session_id: loop_ref.agent.session.seed.clone(),
        call_id: id.to_string(),
        tool_name: effective_name.clone(),
        action: String::new(),
        args: args.clone(),
    };

    match deepx_tools::bridge::admit(
        inv,
        loop_ref.agent.config.permission_level,
        &ws_root,
        loop_ref.trusted_folders.set(),
    ) {
        deepx_tools::bridge::Admission::Authorized(authorized) => {
            emit_tool_result(loop_ref, id, &effective_name, args, authorized);
        }
        deepx_tools::bridge::Admission::ApprovalRequired(challenge) => {
            let cat_str = match challenge.category {
                deepx_tools::permission::ToolCategory::Read => "read",
                deepx_tools::permission::ToolCategory::Write => "write",
                deepx_tools::permission::ToolCategory::Exec => "exec",
                deepx_tools::permission::ToolCategory::Net => "net",
            };
            loop_ref.emit(Agent2Ui::PermissionRequest {
                tool_call_id: challenge.call_id.clone(),
                tool_name: challenge.tool_name.clone(),
                reason: challenge.reason.clone(),
                paths: challenge
                    .resources
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
                category: cat_str.to_string(),
                level: level.to_u8(),
            });
            loop_ref.pending_approvals.insert(
                challenge.call_id.clone(),
                crate::PendingApproval {
                    challenge,
                    is_llm_tool: false,
                },
            );
        }
        deepx_tools::bridge::Admission::Denied(reason) => {
            let turn_id = format!("tc_{id}");
            loop_ref.emit(Agent2Ui::TurnStart {
                turn_id: turn_id.clone(),
                user_text: format!("tool: {name}"),
            });
            loop_ref.emit(Agent2Ui::ToolResults {
                turn_id: turn_id.clone(),
                round_num: 0,
                results: vec![deepx_proto::ToolResultDef {
                    tool_call_id: id.to_string(),
                    output: format!("[DENIED] '{name}' — {reason}"),
                    success: false,
                    file: None,
                }],
            });
            loop_ref.emit(Agent2Ui::TurnEnd {
                turn_id,
                stop_reason: None,
                usage: None,
            });
        }
    }
}

/// Execute an authorized tool and emit results. Shared by both UI-initiated
/// tools and approved LLM-initiated tools through permission responses.
pub(crate) fn emit_tool_result(
    loop_ref: &mut Loop,
    id: &str,
    name: &str,
    args: &serde_json::Value,
    authorized: deepx_tools::bridge::AuthorizedToolCall,
) {
    let turn_id = format!("tc_{id}");
    let round_num = 0u32;

    loop_ref.emit(Agent2Ui::TurnStart {
        turn_id: turn_id.clone(),
        user_text: format!("tool: {name}"),
    });
    let args_display: String = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or(name)
        .chars()
        .take(80)
        .collect();
    loop_ref.emit(Agent2Ui::RoundComplete {
        turn_id: turn_id.clone(),
        round_num,
        thinking: None,
        answer: None,
        tool_calls: vec![deepx_proto::ToolCallDef {
            id: id.to_string(),
            name: name.to_string(),
            args_display: args_display.clone(),
            args_json: args.to_string(),
        }],
        blocks: vec![deepx_proto::RoundBlock::Tool {
            card: deepx_proto::ToolCallDef {
                id: id.to_string(),
                name: name.to_string(),
                args_display,
                args_json: args.to_string(),
            },
        }],
        is_final: false,
    });

    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<(String, String)>();
    let tool_id = id.to_string();
    let tool_id_for_result = tool_id.clone();
    let tool_id_progress = tool_id.clone();
    let handle = std::thread::Builder::new()
        .stack_size(4 * 1024 * 1024)
        .spawn(move || {
            let result = deepx_tools::bridge::execute_authorized(authorized, Some(progress_tx));
            (tool_id, result.content, result.success, result.code_delta)
        })
        .expect("failed to spawn tool thread");

    let mut pending_chunk = String::new();
    loop {
        match progress_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok((tc_id, chunk)) => {
                pending_chunk.push_str(&chunk);
                while let Ok((_, c)) = progress_rx.try_recv() {
                    pending_chunk.push_str(&c);
                }
                loop_ref.emit_delta(Agent2Ui::ExecProgress {
                    tool_call_id: tc_id,
                    chunk: std::mem::take(&mut pending_chunk),
                });
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !pending_chunk.is_empty() {
                    loop_ref.emit_delta(Agent2Ui::ExecProgress {
                        tool_call_id: tool_id_progress.clone(),
                        chunk: std::mem::take(&mut pending_chunk),
                    });
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let (tid, output, success, code_delta) = handle.join().unwrap_or_else(|_| {
        (
            tool_id_for_result,
            "[ERROR] tool thread panicked".into(),
            false,
            None,
        )
    });
    if let Some(ref delta) = code_delta {
        loop_ref.code_stats.push(delta.clone());
        loop_ref.emit_delta(Agent2Ui::CodeDelta {
            lines_added: delta.lines_added,
            lines_removed: delta.lines_removed,
            files_created: delta.files_created,
            files_deleted: delta.files_deleted,
            file: delta.file.clone(),
        });
    }
    loop_ref.emit(Agent2Ui::ToolResults {
        turn_id: turn_id.clone(),
        round_num,
        results: vec![deepx_proto::ToolResultDef {
            tool_call_id: tid,
            output,
            success,
            file: None,
        }],
    });
    loop_ref.emit(Agent2Ui::TurnEnd {
        turn_id,
        stop_reason: None,
        usage: None,
    });
}
