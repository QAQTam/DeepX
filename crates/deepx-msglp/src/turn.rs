//! LLM turn engine: drives the gate→tools→repeat loop.
//!
//! Handles SSE streaming, tool call parsing, parallel and serial tool
//! execution, permission suspension/resume, and ask_user detection.

use crate::Loop;
use crate::LoopPhase;
use crate::PendingApproval;
use crate::TurnResumeState;
use crate::conflict;
use crate::util;
use deepx_message::Effect;
use deepx_proto::{Agent2Ui, RoundDeltaKind};
use std::collections::HashSet;

/// Emit completed tool results for a round and return raw results for upstream
/// logic (e.g. ask_user detection).
pub(crate) fn emit_completed_tool_round(
    loop_ref: &mut Loop,
    turn_id: &str,
    round_num: u32,
) -> Vec<(String, String, String, bool)> {
    let results = loop_ref.agent.msg.last_step_tool_results();
    let ts = util::chrono_local_datetime();
    let tool_defs = results
        .iter()
        .map(|(tc_id, tool_name, result_content, success)| {
            let args = loop_ref
                .agent
                .msg
                .tool_call_args(tc_id)
                .map(|a| a.to_string())
                .unwrap_or_default();
            loop_ref.emit_delta(Agent2Ui::AuditRecord {
                tool_name: tool_name.clone(),
                result_summary: result_content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect(),
                success: *success,
                time: ts.clone(),
                args,
            });
            deepx_proto::ToolResultDef {
                tool_call_id: tc_id.clone(),
                output: result_content.clone(),
                success: *success,
                file: None,
            }
        })
        .collect::<Vec<_>>();

    if !tool_defs.is_empty() {
        loop_ref.emit(Agent2Ui::ToolResults {
            turn_id: turn_id.to_string(),
            round_num,
            results: tool_defs,
        });
    }
    if results.iter().any(|(_, name, _, _)| name == "plan_submit") {
        loop_ref.emit(Agent2Ui::PlanChanged);
    }
    loop_ref.emit_dashboard();
    loop_ref.flush_meta_and_stats();
    results
}

/// Resume a suspended LLM turn after all pending permission approvals have
/// been resolved. Re-enters the gate→tools loop at the saved round.
pub(crate) fn resume_saved_turn(loop_ref: &mut Loop) {
    let saved = match loop_ref.saved_turn.take() {
        Some(s) => s,
        None => return,
    };

    if saved.session_id != loop_ref.agent.session.seed {
        log::warn!(
            "[AGENT] refusing to resume turn {} from stale session {}",
            saved.turn_id,
            saved.session_id
        );
        return;
    }
    if let Err(reason) = deepx_tools::bridge::verify_active_session(&saved.session_id) {
        log::warn!(
            "[AGENT] refusing to resume turn {}: {}",
            saved.turn_id,
            reason
        );
        return;
    }

    log::info!(
        "[AGENT] resuming turn {} round {}",
        saved.turn_id,
        saved.round_num
    );

    emit_completed_tool_round(loop_ref, &saved.turn_id, saved.round_num);

    // Continue the LLM turn by re-entering the gate→tools loop
    run_llm_turn(loop_ref, saved.turn_id, saved.round_num + 1, saved.usage);
}

/// Run a full LLM turn: gate→tools→repeat until the turn is complete,
/// a permission request suspends the turn, or an interrupt occurs.
pub(crate) fn run_llm_turn(
    loop_ref: &mut Loop,
    turn_id: String,
    mut round_num: u32,
    mut last_usage: Option<deepx_types::UsageInfo>,
) {
    // Rebuild provider from current config (not from saved state)
    let ep = deepx_config::registry::find_endpoint(
        &loop_ref.agent.config.provider_id,
        &loop_ref.agent.config.endpoint,
    );
    let provider = deepx_gate::ProviderConfig::openai(
        &loop_ref.agent.config.base_url,
        &loop_ref.agent.config.api_key,
        &loop_ref.agent.config.model,
        ep.as_ref().and_then(|e| e.user_id_mode.clone()),
        ep.as_ref().and_then(|e| e.chat_path.clone()),
        ep.as_ref()
            .map(|e| e.thinking_mode.clone())
            .unwrap_or_default(),
        ep.as_ref()
            .map(|e| e.cache_field.clone())
            .unwrap_or_default(),
        ep.as_ref().map(|e| e.supports_thinking).unwrap_or(true),
    )
    .with_stateful(ep.as_ref().map(|e| e.stateful).unwrap_or(false));

    loop {
        // ── Check for interrupt commands between rounds ──
        if loop_ref.check_interrupts() {
            loop_ref.agent.msg.remove_last_step_if_incomplete();
            loop_ref.flush_meta_and_stats();
            break;
        }

        if loop_ref.cancel.is_set() || deepx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            loop_ref.agent.msg.remove_last_step_if_incomplete();
            loop_ref.flush_meta_and_stats();
            break;
        }

        // Check for pending session switch (set by check_interrupts)
        if loop_ref.pending_session.is_some() || loop_ref.pending_new_session {
            loop_ref.agent.msg.remove_last_step_if_incomplete();
            loop_ref.flush_meta_and_stats();
            break;
        }

        let messages = loop_ref.agent.build_context();

        let tools = Some(loop_ref.agent.tool_defs.clone());
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls_raw = serde_json::Value::Null;
        let mut had_error = false;

        loop_ref.phase = LoopPhase::GateRunning;
        let cancel_arc = loop_ref.cancel.arc();
        let result = deepx_gate::chat_stream(
            &provider,
            messages,
            tools,
            loop_ref.agent.config.max_tokens,
            Some(loop_ref.agent.config.reasoning_effort.clone()),
            Some(loop_ref.agent.session.seed.clone()),
            Some(&cancel_arc),
            &mut |event| match event {
                deepx_gate::StreamEvent::ContentDelta(d) => {
                    if loop_ref.cancel.is_set() {
                        return;
                    }
                    content.push_str(&d);
                    loop_ref.emit_delta(Agent2Ui::RoundDelta {
                        turn_id: turn_id.clone(),
                        round_num,
                        kind: RoundDeltaKind::Answering,
                        delta: d,
                    });
                }
                deepx_gate::StreamEvent::ReasoningDelta(r) => {
                    if loop_ref.cancel.is_set() {
                        return;
                    }
                    reasoning.push_str(&r);
                    loop_ref.emit_delta(Agent2Ui::RoundDelta {
                        turn_id: turn_id.clone(),
                        round_num,
                        kind: RoundDeltaKind::Thinking,
                        delta: r,
                    });
                }
                deepx_gate::StreamEvent::Done {
                    raw_message, usage, ..
                } => {
                    if let Some(ref u) = usage {
                        loop_ref.agent.session.tokens += u.total_tokens as u64;
                        last_usage = usage.clone();
                    }
                    content.clear();
                    reasoning.clear();
                    let mut blocks: Vec<serde_json::Value> = Vec::new();
                    for block in &raw_message.content {
                        match block {
                            deepx_types::ContentBlock::Text { text } => content.push_str(text),
                            deepx_types::ContentBlock::Reasoning { reasoning: r } => {
                                reasoning.push_str(r)
                            }
                            deepx_types::ContentBlock::ToolUse { id, name, input } => {
                                blocks.push(serde_json::json!({
                                    "id": id,
                                    "name": name,
                                    "arguments": input.to_string(),
                                }));
                            }
                            _ => {}
                        }
                    }
                    if !blocks.is_empty() {
                        tool_calls_raw = serde_json::Value::Array(blocks);
                    }
                }
                deepx_gate::StreamEvent::ToolCallProgress {
                    index,
                    id,
                    name,
                    args_so_far,
                } => {
                    loop_ref.emit_delta(Agent2Ui::ToolCallPreview {
                        turn_id: turn_id.clone(),
                        round_num,
                        index,
                        id,
                        name,
                        args_so_far,
                    });
                }
                deepx_gate::StreamEvent::UsageUpdate(u) => {
                    last_usage = Some(u.clone());
                    loop_ref.agent.session.tokens =
                        loop_ref.agent.session.tokens.max(u.total_tokens as u64);
                    loop_ref.emit_delta(Agent2Ui::Dashboard {
                        hp_connected: true,
                        session_seed: loop_ref.agent.session.seed.clone(),
                        context_limit: loop_ref.agent.config.context_limit,
                        tool_calls_total: 0,
                        tool_failures: 0,
                        current_phase: "single".into(),
                        streaming: true,
                        dsml_compat_count: loop_ref.agent.dsml_compat_count,
                        documents: Vec::new(),
                        recent_edits: Vec::new(),
                        tasks: Vec::new(),
                        session_title: None,
                        usage: Some(u),
                        model: Some(loop_ref.agent.config.model.clone()),
                    });
                }
                deepx_gate::StreamEvent::Retrying {
                    attempt,
                    max_retries,
                    delay_secs,
                    error,
                } => {
                    let msg = format!(
                        "API error, retrying ({attempt}/{max_retries}) in {delay_secs}s: {error}"
                    );
                    loop_ref.emit(Agent2Ui::Error { message: msg });
                }
                deepx_gate::StreamEvent::Error(msg) => {
                    loop_ref.emit(Agent2Ui::Error { message: msg });
                    had_error = true;
                }
            },
        );

        if had_error || result.is_err() {
            loop_ref.flush_meta_and_stats();
            break;
        }

        if loop_ref.cancel.is_set() || deepx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            loop_ref.agent.msg.remove_last_step_if_incomplete();
            loop_ref.flush_meta_and_stats();
            break;
        }

        let parsed = util::parse_tool_calls_from_response(
            &content,
            &reasoning,
            &tool_calls_raw,
            &loop_ref.agent,
        );
        let assistant_msg = util::build_assistant_message(&content, &reasoning, &parsed);
        let effect = loop_ref.agent.msg.push_assistant(assistant_msg.clone());
        loop_ref.flush_meta_and_stats();

        util::emit_round_complete(
            &loop_ref.event_tx,
            &turn_id,
            round_num,
            &assistant_msg,
            &content,
            &reasoning,
            &parsed,
        );

        match effect {
            Effect::None => {
                loop_ref.phase = LoopPhase::ToolsRunning;

                let mut round_pending_ids = Vec::new();
                let pending = loop_ref.agent.msg.get_last_step_pending();
                if !pending.is_empty() {
                    let mut seen_call_ids = HashSet::new();
                    let duplicate_or_reused = pending.iter().any(|tool| {
                        !seen_call_ids.insert(tool.id.clone())
                            || loop_ref.pending_approvals.contains_key(&tool.id)
                    });
                    if duplicate_or_reused {
                        log::error!("[AGENT] duplicate or reused LLM tool-call id");
                        loop_ref.agent.msg.remove_last_step_if_incomplete();
                        loop_ref.emit(Agent2Ui::Error {
                            message: "Model returned a duplicate or reused tool-call ID; no tools were executed."
                                .into(),
                        });
                        break;
                    }
                    let (serial_groups, serial_after) = conflict::resolve_write_conflicts(&pending);
                    let ws_root = {
                        let ws = deepx_tools::CURRENT_WORKSPACE
                            .read()
                            .expect("CURRENT_WORKSPACE lock")
                            .clone();
                        if ws.is_empty() || ws == "." {
                            std::env::current_dir()
                                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                        } else {
                            std::path::PathBuf::from(ws)
                        }
                    };

                    let mut authorized: Vec<(
                        String,
                        String,
                        deepx_tools::bridge::AuthorizedToolCall,
                    )> = Vec::new();
                    for (i, tool) in pending.iter().enumerate() {
                        if serial_after.contains(&i) {
                            continue;
                        }

                        let inv = deepx_tools::bridge::ToolInvocation {
                            session_id: loop_ref.agent.session.seed.clone(),
                            call_id: tool.id.clone(),
                            tool_name: tool.name.clone(),
                            action: String::new(),
                            args: tool.args.clone(),
                        };
                        match deepx_tools::bridge::admit(
                            inv,
                            loop_ref.agent.config.permission_level,
                            &ws_root,
                            loop_ref.trusted_folders.set(),
                        ) {
                            deepx_tools::bridge::Admission::Authorized(auth) => {
                                authorized.push((tool.id.clone(), tool.name.clone(), auth));
                            }
                            deepx_tools::bridge::Admission::ApprovalRequired(challenge) => {
                                let cat_str = match challenge.category {
                                    deepx_tools::permission::ToolCategory::Read => "read",
                                    deepx_tools::permission::ToolCategory::Write => "write",
                                    deepx_tools::permission::ToolCategory::Exec => "exec",
                                    deepx_tools::permission::ToolCategory::Net => "net",
                                };
                                let call_id = challenge.call_id.clone();
                                loop_ref.emit(Agent2Ui::PermissionRequest {
                                    tool_call_id: call_id.clone(),
                                    tool_name: challenge.tool_name.clone(),
                                    reason: challenge.reason.clone(),
                                    paths: challenge
                                        .resources
                                        .iter()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .collect(),
                                    category: cat_str.to_string(),
                                    level: deepx_tools::permission::PermissionLevel::from_u8(
                                        loop_ref.agent.config.permission_level,
                                    )
                                    .to_u8(),
                                });
                                round_pending_ids.push(call_id.clone());
                                loop_ref.pending_approvals.insert(
                                    call_id,
                                    PendingApproval {
                                        challenge,
                                        is_llm_tool: true,
                                    },
                                );
                            }
                            deepx_tools::bridge::Admission::Denied(reason) => {
                                loop_ref.agent.msg.push_tool_result_direct(
                                    &tool.id,
                                    &format!(
                                        "[timeis: {}]\n[DENIED] {}",
                                        util::chrono_local_datetime(),
                                        reason
                                    ),
                                    false,
                                );
                            }
                        }
                    }

                    // ── Execute parallel authorized tools ──
                    let (progress_tx, progress_rx) =
                        deepx_tools::bounded_exec_progress_channel();
                    let mut handles: Vec<(
                        String,
                        std::thread::JoinHandle<(
                            String,
                            String,
                            bool,
                            Option<deepx_proto::CodeDeltaRecord>,
                            Option<deepx_skills::SkillActivation>,
                        )>,
                    )> = Vec::new();
                    let mut tool_infos = Vec::new();

                    for (tc_id, tool_name, auth_call) in authorized {
                        let tx = progress_tx.clone();
                        let tc_id_for_closure = tc_id.clone();
                        let tc_id_for_handle = tc_id.clone();
                        tool_infos.push((tc_id, tool_name));
                        let handle = std::thread::Builder::new()
                            .stack_size(4 * 1024 * 1024)
                            .spawn(move || {
                                let result = deepx_tools::bridge::execute_authorized(
                                    auth_call,
                                    Some(tx),
                                );
                                (
                                    tc_id_for_closure,
                                    result.content,
                                    result.success,
                                    result.code_delta,
                                    result.skill_activation,
                                )
                            })
                            .expect("failed to spawn tool thread");
                        handles.push((tc_id_for_handle, handle));
                    }
                    drop(progress_tx);

                    if !handles.is_empty() {
                        let cancelled = loop_ref.drain_tool_progress(progress_rx);

                        if cancelled {
                            log::info!(
                                "[AGENT] cancelled, pushing placeholder results + background reaper"
                            );
                            let ts = util::chrono_local_datetime();
                            for (tc_id, _tool_name) in &tool_infos {
                                loop_ref.agent.msg.push_tool_result_direct(
                                    tc_id,
                                    &format!("[timeis: {ts}]\n[CANCELLED]"),
                                    false,
                                );
                            }
                            std::thread::spawn(move || {
                                for (_id, h) in handles {
                                    let _ = h.join();
                                }
                            });
                        } else {
                            let ts = util::chrono_local_datetime();
                            for (tc_id, h) in handles {
                                match h.join() {
                                    Ok((_id, content, success, code_delta, skill_activation)) => {
                                        loop_ref.agent.msg.push_tool_result_direct(
                                            &tc_id,
                                            &format!("[timeis: {ts}]\n{content}"),
                                            success,
                                        );
                                        if let Some(activation) = skill_activation {
                                            loop_ref.agent.activate_skill(&tc_id, activation);
                                        }
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
                                    }
                                    Err(_) => {
                                        log::error!("[AGENT] tool thread panicked for {tc_id}");
                                        loop_ref.agent.msg.push_tool_result_direct(
                                            &tc_id,
                                            &format!(
                                                "[timeis: {ts}]\n[ERROR] tool thread panicked"
                                            ),
                                            false,
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // ── Execute serialized follow-up tools (same-file write conflicts) ──
                    if !serial_groups.is_empty() {
                        let ts = util::chrono_local_datetime();
                        for group in &serial_groups {
                            for &idx in &group[1..] {
                                let tool = &pending[idx];
                                let inv = deepx_tools::bridge::ToolInvocation {
                                    session_id: loop_ref.agent.session.seed.clone(),
                                    call_id: tool.id.clone(),
                                    tool_name: tool.name.clone(),
                                    action: String::new(),
                                    args: tool.args.clone(),
                                };
                                match deepx_tools::bridge::admit(
                                    inv,
                                    loop_ref.agent.config.permission_level,
                                    &ws_root,
                                    loop_ref.trusted_folders.set(),
                                ) {
                                    deepx_tools::bridge::Admission::Authorized(auth) => {
                                        let result =
                                            deepx_tools::bridge::execute_authorized(auth, None);
                                        loop_ref.agent.msg.push_tool_result_direct(
                                            &tool.id,
                                            &format!("[timeis: {ts}]\n{}", result.content),
                                            result.success,
                                        );
                                        if let Some(activation) = result.skill_activation.clone() {
                                            loop_ref.agent.activate_skill(&tool.id, activation);
                                        }
                                        if let Some(ref delta) = result.code_delta {
                                            loop_ref.code_stats.push(delta.clone());
                                            loop_ref.emit_delta(Agent2Ui::CodeDelta {
                                                lines_added: delta.lines_added,
                                                lines_removed: delta.lines_removed,
                                                files_created: delta.files_created,
                                                files_deleted: delta.files_deleted,
                                                file: delta.file.clone(),
                                            });
                                        }
                                    }
                                    deepx_tools::bridge::Admission::ApprovalRequired(
                                        challenge,
                                    ) => {
                                        let cat_str = match challenge.category {
                                            deepx_tools::permission::ToolCategory::Read => {
                                                "read"
                                            }
                                            deepx_tools::permission::ToolCategory::Write => {
                                                "write"
                                            }
                                            deepx_tools::permission::ToolCategory::Exec => {
                                                "exec"
                                            }
                                            deepx_tools::permission::ToolCategory::Net => "net",
                                        };
                                        let call_id = challenge.call_id.clone();
                                        loop_ref.emit(Agent2Ui::PermissionRequest {
                                            tool_call_id: call_id.clone(),
                                            tool_name: challenge.tool_name.clone(),
                                            reason: challenge.reason.clone(),
                                            paths: challenge.resources.iter().map(|p| p.to_string_lossy().to_string()).collect(),
                                            category: cat_str.to_string(),
                                            level: deepx_tools::permission::PermissionLevel::from_u8(loop_ref.agent.config.permission_level).to_u8(),
                                        });
                                        round_pending_ids.push(call_id.clone());
                                        loop_ref.pending_approvals.insert(
                                            call_id,
                                            PendingApproval {
                                                challenge,
                                                is_llm_tool: true,
                                            },
                                        );
                                    }
                                    deepx_tools::bridge::Admission::Denied(reason) => {
                                        loop_ref.agent.msg.push_tool_result_direct(
                                            &tool.id,
                                            &format!("[timeis: {ts}]\n[DENIED] {reason}"),
                                            false,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if !round_pending_ids.is_empty() {
                    loop_ref.saved_turn = Some(TurnResumeState {
                        session_id: loop_ref.agent.session.seed.clone(),
                        turn_id: turn_id.clone(),
                        round_num,
                        pending_call_ids: round_pending_ids,
                        usage: last_usage.clone(),
                    });
                    log::info!(
                        "[AGENT] suspending turn {turn_id} round {round_num} for {} pending approvals",
                        loop_ref.saved_turn.as_ref().unwrap().pending_call_ids.len()
                    );
                    break;
                }

                let results = emit_completed_tool_round(loop_ref, &turn_id, round_num);

                // ── ask_user: stop loop, wait for user response ──
                let has_user_query = results.iter().any(|(_, _, content, _)| {
                    content.starts_with("[USER_QUERY]")
                        || serde_json::from_str::<serde_json::Value>(content)
                            .ok()
                            .and_then(|v| v.get("user_query").and_then(|u| u.as_bool()))
                            .unwrap_or(false)
                });
                if has_user_query {
                    log::info!(
                        "[AGENT] ask_user detected — breaking loop to wait for user input"
                    );
                    break;
                }

                round_num += 1;
                continue;
            }
            Effect::TurnComplete => {}
            _ => {}
        }

        loop_ref.flush_meta_and_stats();

        if let Some(ref usage) = last_usage {
            util::record_token_usage(usage, &loop_ref.agent.config.model);
        }

        loop_ref.emit(Agent2Ui::TurnEnd {
            turn_id: turn_id.clone(),
            stop_reason: None,
            usage: last_usage.clone(),
        });

        break;
    }

    // If turn was suspended for pending approvals, return without Done/TurnEnd.
    if loop_ref.saved_turn.is_some() {
        return;
    }

    loop_ref.emit(Agent2Ui::Done);
}