//! TurnEngine: drives the gate→tools→repeat cycle.
//!
//! Owns: suspended TurnState.
//! Receives: RingContext + ToolEngine (for tool execution).
//! Returns: Outcome (ContinueTurn, YieldToUser, TurnComplete, Error).

use std::collections::HashSet;

use deepx_message::Effect;
use deepx_proto::{Agent2Ui, RoundDeltaKind};
use deepx_types::UsageInfo;

use super::types::*;
use super::engine_tool::ToolEngine;
use crate::conflict;
use crate::util;

/// Why the turn is being resumed.
pub enum ResumeReason {
    /// User answered permission dialogs — all approvals resolved.
    PermissionResolved,
    /// User answered an ask_user prompt — feed answer as tool result.
    AskUserAnswer { answer: String },
}

/// TurnEngine manages a single LLM turn lifecycle.
pub struct TurnEngine {
    /// If Some, a turn is suspended waiting for permission or ask_user.
    pub(crate) suspended: Option<TurnState>,
}

impl TurnEngine {
    pub fn new() -> Self {
        Self { suspended: None }
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.is_some()
    }

    /// Returns the reason the turn was suspended, or None if not suspended.
    pub fn suspended_reason(&self) -> Option<YieldReason> {
        self.suspended.as_ref().map(|s| s.reason)
    }

    // ── Public API ──

    /// Run one full lap around the gate→tools ring.
    /// Called initially by InputEngine after user input, and recursively
    /// by Loop::apply_outcome for ContinueTurn.
    pub fn run(
        &mut self,
        ctx: &mut RingContext,
        tool: &mut ToolEngine,
        turn_id: String,
        round_num: u32,
        last_usage: Option<UsageInfo>,
    ) -> Outcome {
        self.run_lap(ctx, tool, turn_id, round_num, last_usage)
    }

    /// Resume a suspended turn.
    pub fn resume(
        &mut self,
        ctx: &mut RingContext,
        tool: &mut ToolEngine,
        reason: ResumeReason,
    ) -> Outcome {
        let saved = match self.suspended.take() {
            Some(s) => s,
            None => return Outcome::Error("No suspended turn to resume".into()),
        };
        if saved.session_id != ctx.agent.session.seed {
            log::warn!("[TURN] refusing to resume stale turn {}", saved.turn_id);
            return Outcome::Handled;
        }

        match reason {
            ResumeReason::PermissionResolved => {
                log::info!("[TURN] resuming turn {} round {}", saved.turn_id, saved.round_num);
                self.emit_completed_tool_round(ctx, &saved.turn_id, saved.round_num);
                self.run_lap(ctx, tool, saved.turn_id, saved.round_num + 1, saved.usage)
            }
            ResumeReason::AskUserAnswer { answer } => {
                let ask_call_id = match ctx.agent.msg.find_last_step_tool_call("ask_user") {
                    Some(id) => id,
                    None => {
                        return Outcome::Error(
                            "Cannot find ask_user tool call in suspended turn".into(),
                        );
                    }
                };
                ctx.agent.msg.push_tool_result(&ask_call_id, &answer, true);
                ctx.agent.msg.flush_meta(
                    &ctx.agent.config.model,
                    &ctx.agent.config.reasoning_effort,
                );
                log::info!(
                    "[TURN] ask_user answer fed as tool result, resuming turn {}",
                    saved.turn_id
                );
                self.emit_completed_tool_round(ctx, &saved.turn_id, saved.round_num);
                self.run_lap(ctx, tool, saved.turn_id, saved.round_num + 1, saved.usage)
            }
        }
    }

    // ── Internal lap execution ──

    fn run_lap(
        &mut self,
        ctx: &mut RingContext,
        tool: &mut ToolEngine,
        turn_id: String,
        round_num: u32,
        mut last_usage: Option<UsageInfo>,
    ) -> Outcome {
        // Rebuild provider from current config
        let ep = deepx_config::registry::find_endpoint(
            &ctx.agent.config.provider_id,
            &ctx.agent.config.endpoint,
        );
        let provider = deepx_gate::ProviderConfig::openai(
            &ctx.agent.config.base_url,
            &ctx.agent.config.api_key,
            &ctx.agent.config.model,
            ep.as_ref().and_then(|e| e.user_id_mode.clone()),
            ep.as_ref().and_then(|e| e.chat_path.clone()),
            ep.as_ref().map(|e| e.thinking_mode.clone()).unwrap_or_default(),
            ep.as_ref().map(|e| e.cache_field.clone()).unwrap_or_default(),
            ep.as_ref().map(|e| e.supports_thinking).unwrap_or(true),
        )
        .with_stateful(ep.as_ref().map(|e| e.stateful).unwrap_or(false));

        loop {
            // ── Interrupt check ──
            if ctx.cancel.is_set() || deepx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                ctx.agent.msg.remove_last_step_if_incomplete();
                ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
                return Outcome::Handled;
            }
            if !ctx.pending.is_empty() {
                ctx.agent.msg.remove_last_step_if_incomplete();
                ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
                return Outcome::Handled;
            }

            let messages = ctx.agent.build_context();
            let tools = Some(ctx.agent.tool_defs.clone());
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut tool_calls_raw = serde_json::Value::Null;
            let mut had_error = false;

            *ctx.phase = LoopPhase::GateRunning;
            let cancel_arc = ctx.cancel.arc();

            // ── SSE Gate Request ──
            let result = deepx_gate::chat_stream(
                &provider, messages, tools,
                ctx.agent.config.max_tokens,
                Some(ctx.agent.config.reasoning_effort.clone()),
                Some(ctx.agent.session.seed.clone()),
                Some(&cancel_arc),
                &mut |event| {
                    match event {
                        deepx_gate::StreamEvent::ContentDelta(d) => {
                            if ctx.cancel.is_set() { return; }
                            content.push_str(&d);
                            ctx.emitter.emit_delta(Agent2Ui::RoundDelta {
                                turn_id: turn_id.clone(), round_num,
                                kind: RoundDeltaKind::Answering, delta: d,
                            });
                        }
                        deepx_gate::StreamEvent::ReasoningDelta(r) => {
                            if ctx.cancel.is_set() { return; }
                            reasoning.push_str(&r);
                            ctx.emitter.emit_delta(Agent2Ui::RoundDelta {
                                turn_id: turn_id.clone(), round_num,
                                kind: RoundDeltaKind::Thinking, delta: r,
                            });
                        }
                        deepx_gate::StreamEvent::Done { raw_message, usage, .. } => {
                            if let Some(ref u) = usage {
                                ctx.agent.session.tokens += u.total_tokens as u64;
                                last_usage = usage.clone();
                            }
                            content.clear(); reasoning.clear();
                            let mut blocks: Vec<serde_json::Value> = Vec::new();
                            for block in &raw_message.content {
                                match block {
                                    deepx_types::ContentBlock::Text { text } => content.push_str(text),
                                    deepx_types::ContentBlock::Reasoning { reasoning: r } => reasoning.push_str(r),
                                    deepx_types::ContentBlock::ToolUse { id, name, input } => {
                                        blocks.push(serde_json::json!({
                                            "id": id, "name": name, "arguments": input.to_string(),
                                        }));
                                    }
                                    _ => {}
                                }
                            }
                            if !blocks.is_empty() {
                                tool_calls_raw = serde_json::Value::Array(blocks);
                            }
                        }
                        deepx_gate::StreamEvent::ToolCallProgress { index, id, name, args_so_far } => {
                            ctx.emitter.emit_delta(Agent2Ui::ToolCallPreview {
                                turn_id: turn_id.clone(), round_num, index, id, name, args_so_far,
                            });
                        }
                        deepx_gate::StreamEvent::UsageUpdate(u) => {
                            last_usage = Some(u.clone());
                            ctx.agent.session.tokens = ctx.agent.session.tokens.max(u.total_tokens as u64);
                            ctx.emitter.emit_delta(Agent2Ui::Dashboard {
                                hp_connected: true,
                                session_seed: ctx.agent.session.seed.clone(),
                                context_limit: ctx.agent.config.context_limit,
                                tool_calls_total: 0, tool_failures: 0,
                                current_phase: "single".into(), streaming: true,
                                dsml_compat_count: ctx.agent.dsml_compat_count,
                                documents: Vec::new(), recent_edits: Vec::new(), tasks: Vec::new(),
                                session_title: None, usage: Some(u),
                                model: Some(ctx.agent.config.model.clone()),
                            });
                        }
                        deepx_gate::StreamEvent::Retrying { attempt, max_retries, delay_secs, error } => {
                            ctx.emitter.emit(Agent2Ui::Error {
                                message: format!("API error, retrying ({attempt}/{max_retries}) in {delay_secs}s: {error}"),
                            });
                        }
                        deepx_gate::StreamEvent::Error(msg) => {
                            ctx.emitter.emit(Agent2Ui::Error { message: msg });
                            had_error = true;
                        }
                    }
                },
            );

            if had_error || result.is_err() {
                ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
                return Outcome::Handled;
            }

            if ctx.cancel.is_set() {
                ctx.agent.msg.remove_last_step_if_incomplete();
                ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
                return Outcome::Handled;
            }

            // ── Parse + push assistant message ──
            let parsed = util::parse_tool_calls_from_response(&content, &reasoning, &tool_calls_raw, &ctx.agent);
            let assistant_msg = util::build_assistant_message(&content, &reasoning, &parsed);
            let effect = ctx.agent.msg.push_assistant(assistant_msg.clone());
            ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);

            util::emit_round_complete_via_emitter(
                ctx.emitter, &turn_id, round_num,
                &assistant_msg, &content, &reasoning, &parsed,
            );

            match effect {
                Effect::None => {
                    // ── Execute tools ──
                    *ctx.phase = LoopPhase::ToolsRunning;

                    let mut pending = ctx.agent.msg.get_last_step_pending();
                    if !pending.is_empty() {
                        // Duplicate tool-call ID check
                        let mut seen = HashSet::new();
                        if pending.iter().any(|t| !seen.insert(t.id.clone())) {
                            ctx.agent.msg.remove_last_step_if_incomplete();
                            ctx.emitter.emit(Agent2Ui::Error {
                                message: "Duplicate tool-call ID from model".into(),
                            });
                            return Outcome::Handled;
                        }

                        // A model response must not create unbounded work. Rejected
                        // calls still receive a result so the next round can recover.
                        const MAX_TOOL_CALLS_PER_ROUND: usize = 16;
                        if pending.len() > MAX_TOOL_CALLS_PER_ROUND {
                            let rejected = pending.split_off(MAX_TOOL_CALLS_PER_ROUND);
                            for call in rejected {
                                ctx.agent.msg.push_tool_result_direct(
                                    &call.id,
                                    "[ERROR] Tool-call limit exceeded for this round (max 16). Retry the remaining calls in a later round.",
                                    false,
                                );
                            }
                        }

                        // Admit all tools via ToolEngine, then keep later writers
                        // out of the parallel worker batches.
                        const MAX_PARALLEL_TOOL_WORKERS: usize = 4;
                        let (_serial_groups, serial_after) = conflict::resolve_write_conflicts(&pending);
                        let serial_call_ids: HashSet<String> = serial_after
                            .iter()
                            .map(|index| pending[*index].id.clone())
                            .collect();
                        let (authorized, round_pending_ids) = tool.admit_batch(ctx, &pending);
                        let (mut parallel_authorized, serial_authorized): (Vec<_>, Vec<_>) = authorized
                            .into_iter()
                            .partition(|admitted| !serial_call_ids.contains(&admitted.call_id));

                        // Execute independent tools in bounded parallel batches.
                        while !parallel_authorized.is_empty() {
                            let batch_len = parallel_authorized.len().min(MAX_PARALLEL_TOOL_WORKERS);
                            let batch: Vec<_> = parallel_authorized.drain(..batch_len).collect();
                            let (progress_tx, progress_rx) = deepx_tools::bounded_exec_progress_channel();
                            let mut handles: Vec<(String, std::thread::JoinHandle<_>)> = Vec::new();

                            for admitted in batch {
                                let tx = progress_tx.clone();
                                let call_id = admitted.call_id.clone();
                                let handle = std::thread::Builder::new()
                                    .stack_size(4 * 1024 * 1024)
                                    .spawn({
                                        let auth = admitted.auth;
                                        let cid = call_id.clone();
                                        move || {
                                            let result = deepx_tools::bridge::execute_authorized(*auth, Some(tx));
                                            (
                                                cid,
                                                result.content,
                                                result.success,
                                                result.code_delta,
                                                result.skill_activation,
                                            )
                                        }
                                    })
                                    .expect("tool thread spawn");
                                handles.push((call_id, handle));
                            }
                            drop(progress_tx);

                            // Drain progress
                            tool.drain_progress_external(ctx, progress_rx, "llm_tool");

                            // Collect results
                            let cancelled = ctx.cancel.is_set();
                            for (call_id, h) in handles {
                                if cancelled {
                                    let _ = h.join(); // reap
                                } else {
                                    match h.join() {
                                        Ok((_cid, content, success, code_delta, skill_activation)) => {
                                            ctx.agent.msg.push_tool_result_direct(
                                                &call_id, &content, success,
                                            );
                                            if let Some(activation) = skill_activation {
                                                ctx.agent.activate_skill(&call_id, activation);
                                            }
                                            if let Some(ref delta) = code_delta {
                                                ctx.stats.push_delta(delta.clone());
                                                ctx.emitter.emit_delta(Agent2Ui::CodeDelta {
                                                    lines_added: delta.lines_added,
                                                    lines_removed: delta.lines_removed,
                                                    files_created: delta.files_created,
                                                    files_deleted: delta.files_deleted,
                                                    file: delta.file.clone(),
                                                });
                                            }
                                        }
                                        Err(_) => {
                                            ctx.agent.msg.push_tool_result_direct(
                                                &call_id, "[ERROR] tool thread panicked", false,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Execute later same-file writers exactly once, after the
                        // first writer from their conflict group has completed.
                        for admitted in serial_authorized {
                            if ctx.cancel.is_set() {
                                break;
                            }
                            let call_id = admitted.call_id;
                            let (progress_tx, progress_rx) = deepx_tools::bounded_exec_progress_channel();
                            let handle = std::thread::Builder::new()
                                .stack_size(4 * 1024 * 1024)
                                .spawn({
                                    let auth = admitted.auth;
                                    move || {
                                        let result = deepx_tools::bridge::execute_authorized(*auth, Some(progress_tx));
                                        (
                                            result.content,
                                            result.success,
                                            result.code_delta,
                                            result.skill_activation,
                                        )
                                    }
                                })
                                .expect("tool thread spawn");
                            tool.drain_progress_external(ctx, progress_rx, &call_id);
                            match handle.join() {
                                Ok((content, success, code_delta, skill_activation)) => {
                                    ctx.agent.msg.push_tool_result_direct(&call_id, &content, success);
                                    if let Some(activation) = skill_activation {
                                        ctx.agent.activate_skill(&call_id, activation);
                                    }
                                    if let Some(ref delta) = code_delta {
                                        ctx.stats.push_delta(delta.clone());
                                        ctx.emitter.emit_delta(Agent2Ui::CodeDelta {
                                            lines_added: delta.lines_added,
                                            lines_removed: delta.lines_removed,
                                            files_created: delta.files_created,
                                            files_deleted: delta.files_deleted,
                                            file: delta.file.clone(),
                                        });
                                    }
                                }
                                Err(_) => ctx.agent.msg.push_tool_result_direct(
                                    &call_id,
                                    "[ERROR] tool thread panicked",
                                    false,
                                ),
                            }
                        }

                        // Check for pending approvals
                        if !round_pending_ids.is_empty() {
                            self.suspended = Some(TurnState {
                                session_id: ctx.agent.session.seed.clone(),
                                turn_id: turn_id.clone(),
                                round_num,
                                pending_call_ids: round_pending_ids,
                                usage: last_usage.clone(),
                                reason: YieldReason::PermissionPending,
                            });
                            return Outcome::YieldToUser {
                                turn_id,
                                reason: YieldReason::PermissionPending,
                            };
                        }
                    }

                    // Emit completed tool round + check ask_user
                    let results = self.emit_completed_tool_round(ctx, &turn_id, round_num);
                    let has_user_query = results.iter().any(|(_, _, content, _)| {
                        content.starts_with("[USER_QUERY]")
                            || serde_json::from_str::<serde_json::Value>(content)
                                .ok()
                                .and_then(|v| v.get("user_query").and_then(|u| u.as_bool()))
                                .unwrap_or(false)
                    });
                    if has_user_query {
                        self.suspended = Some(TurnState {
                            session_id: ctx.agent.session.seed.clone(),
                            turn_id: turn_id.clone(),
                            round_num,
                            pending_call_ids: Vec::new(),
                            usage: last_usage.clone(),
                            reason: YieldReason::AskUser,
                        });
                        return Outcome::YieldToUser {
                            turn_id,
                            reason: YieldReason::AskUser,
                        };
                    }

                    // Another lap: tools executed, back to Gate
                    return Outcome::ContinueTurn {
                        turn_id,
                        round_num: round_num + 1,
                        usage: last_usage,
                    };
                }
                Effect::TurnComplete => {}
                _ => {}
            }

            ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
            if let Some(ref usage) = last_usage {
                util::record_token_usage(usage, &ctx.agent.config.model);
            }
            ctx.emitter.emit(Agent2Ui::TurnEnd {
                turn_id: turn_id.clone(),
                stop_reason: None,
                usage: last_usage.clone(),
            });
            return Outcome::TurnComplete {
                turn_id,
                usage: last_usage,
            };
        }
    }

    fn emit_completed_tool_round(&self, ctx: &mut RingContext, turn_id: &str, round_num: u32)
        -> Vec<(String, String, String, bool)>
    {
        let results = ctx.agent.msg.last_step_tool_results();
        let ts = util::chrono_local_datetime();
        let tool_defs: Vec<_> = results.iter().map(|(tc_id, name, content, success)| {
            let args = ctx.agent.msg.tool_call_args(tc_id).map(|a| a.to_string()).unwrap_or_default();
            ctx.emitter.emit_delta(Agent2Ui::AuditRecord {
                tool_name: name.clone(),
                result_summary: content.lines().next().unwrap_or("").chars().take(120).collect(),
                success: *success, time: ts.clone(), args,
            });
            deepx_proto::ToolResultDef {
                tool_call_id: tc_id.clone(), output: content.clone(), success: *success, file: None,
            }
        }).collect();

        if !tool_defs.is_empty() {
            ctx.emitter.emit(Agent2Ui::ToolResults { turn_id: turn_id.to_string(), round_num, results: tool_defs });
        }
        if results.iter().any(|(_, name, _, _)| name == "plan_submit") {
            ctx.emitter.emit(Agent2Ui::PlanChanged);
        }
        ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
        results
    }

    /// Reset all turn state (called on Cancel / new session).
    pub fn reset(&mut self) {
        self.suspended = None;
    }
}
