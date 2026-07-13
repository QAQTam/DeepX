//! Permission response handling for the message loop.
//!
//! Processes user responses to permission requests, applies trust decisions,
//! and manages suspended LLM turns awaiting approval.

use crate::Loop;
use deepx_proto::Agent2Ui;

/// Invalidate all pending authorization challenges and clear the suspended
/// turn state. Called on Cancel, new session, and shutdown.
pub(crate) fn invalidate_pending_authorizations(loop_ref: &mut Loop) {
    loop_ref.pending_approvals.clear();
    loop_ref.saved_turn = None;
    deepx_tools::bridge::clear_runtime_context();
}

/// Handle a user's response to a permission request dialog.
/// Applies the decision (approve/deny) and resumes the suspended LLM turn
/// if all pending approvals for that turn are resolved.
pub(crate) fn handle_permission_response(
    loop_ref: &mut Loop,
    tool_call_id: &str,
    approved: bool,
    trust_folder: bool,
) {
    let pending = match loop_ref.pending_approvals.remove(tool_call_id) {
        Some(p) => p,
        None => {
            log::warn!(
                "[AGENT] PermissionResponse for unknown call_id={tool_call_id} — missing or replayed"
            );
            return;
        }
    };

    // Extract fields before consuming the challenge.
    let call_id = pending.challenge.call_id.clone();
    let tool_name = pending.challenge.tool_name.clone();
    let is_llm = pending.is_llm_tool;
    let approved_resources = pending.challenge.resources.clone();

    match pending.challenge.approve(approved) {
        Ok(authorized) => {
            if trust_folder {
                for path in &approved_resources {
                    loop_ref.trusted_folders.trust(path.parent().unwrap_or(path));
                }
                log::info!("[AGENT] trusted folders updated from approved permission response");
            }
            if is_llm {
                let result = deepx_tools::bridge::execute_authorized(authorized, None);
                loop_ref.agent.msg.push_tool_result_direct(
                    &call_id,
                    &result.content,
                    result.success,
                );
                if let Some(activation) = result.skill_activation.clone() {
                    loop_ref.agent.activate_skill(&call_id, activation);
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
            } else {
                let args = authorized.args().clone();
                crate::tool_exec::emit_tool_result(loop_ref, &call_id, &tool_name, &args, authorized);
            }
        }
        Err(deepx_tools::bridge::ApprovalError::Rejected) => {
            if is_llm {
                loop_ref.agent.msg.push_tool_result_direct(
                    &call_id,
                    &format!("[DENIED] '{tool_name}' (user denied permission)"),
                    false,
                );
            } else {
                let turn_id = format!("tc_{call_id}");
                loop_ref.emit(Agent2Ui::TurnStart {
                    turn_id: turn_id.clone(),
                    user_text: format!("tool: {tool_name}"),
                });
                loop_ref.emit(Agent2Ui::ToolResults {
                    turn_id: turn_id.clone(),
                    round_num: 0,
                    results: vec![deepx_proto::ToolResultDef {
                        tool_call_id: call_id.clone(),
                        output: format!("[DENIED] '{tool_name}' (user denied permission)"),
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
        Err(deepx_tools::bridge::ApprovalError::Expired) => {
            if is_llm {
                loop_ref.agent.msg.push_tool_result_direct(
                    &call_id,
                    &format!("[EXPIRED] Permission approval expired for '{tool_name}'."),
                    false,
                );
            } else {
                log::warn!("[AGENT] permission approval expired for call_id={call_id}");
                let turn_id = format!("tc_{call_id}");
                loop_ref.emit(Agent2Ui::TurnStart {
                    turn_id: turn_id.clone(),
                    user_text: format!("tool: {tool_name}"),
                });
                loop_ref.emit(Agent2Ui::ToolResults {
                    turn_id: turn_id.clone(),
                    round_num: 0,
                    results: vec![deepx_proto::ToolResultDef {
                        tool_call_id: call_id.clone(),
                        output: format!(
                            "[EXPIRED] Permission approval expired for '{tool_name}'."
                        ),
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
        Err(deepx_tools::bridge::ApprovalError::MissingOrReplayed) => {
            log::warn!("[AGENT] permission response for unknown or replayed call");
        }
    }

    // If this was an LLM tool approval, check if we can resume the suspended turn.
    if is_llm {
        if let Some(ref saved) = loop_ref.saved_turn {
            let all_resolved = saved
                .pending_call_ids
                .iter()
                .all(|id| !loop_ref.pending_approvals.contains_key(id));
            if all_resolved {
                log::info!(
                    "[AGENT] all pending approvals resolved for turn {}, resuming",
                    saved.turn_id
                );
                loop_ref.resume_saved_turn();
            }
        }
    }
}
