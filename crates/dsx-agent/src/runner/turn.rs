//! Turn processing: context building, API chat, tool execution loop.
//!
//! This is the core of the agent — one turn = push user message → build
//! context → HP API call → parse tool calls → execute tools → repeat
//! until no more tool calls or max rounds reached.

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, AgentToHp, AgentToTui};
use dsx_types::{ContentBlock, Message, ToolCall};

use crate::agent::{AgentState, ToolResultAppender};
use crate::assembly::AssemblerError;
use crate::orchestrator::{gates, learning, phase_detector, tracker};
use crate::router;
use crate::session;
use crate::tokenizer;
use crate::tool_parser;

use super::hp_bridge::{emit_tool_result, read_hp_stream_response, HpStreamResponse};
use super::lifecycle::{apply_phase_config, health_status};

/// Outcome of `execute_single_tool` for the caller's loop control.
pub enum ToolOutcome {
    Continue,
    Executed,
    Break,
}

/// Build an assistant message from LLM response parts and push to context.
pub fn build_and_push_assistant(
    agent: &mut AgentState,
    content: &str,
    reasoning_content: &Option<String>,
    thinking_signature: &Option<String>,
    parsed: &[ToolCall],
) -> Message {
    let mut blocks: Vec<ContentBlock> = Vec::new();
    if !content.is_empty() || parsed.is_empty() {
        blocks.push(ContentBlock::Text {
            text: content.to_string(),
        });
    }
    if let Some(ref rc) = reasoning_content {
        if !rc.is_empty() {
            blocks.push(ContentBlock::Thinking {
                thinking: rc.clone(),
                signature: thinking_signature.clone().unwrap_or_default(),
            });
        }
    }
    for tc in parsed {
        let input: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
        blocks.push(ContentBlock::ToolUse {
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            input,
        });
    }
    let assistant_msg = Message {
        role: "assistant".into(),
        content: blocks,
    };

    if let Err(e) = agent.ctx.push_assistant(assistant_msg.clone()) {
        log::error!("push_assistant failed: {:?} — repairing", e);
        agent.ctx.push_assistant_restore(assistant_msg.clone());
    }

    assistant_msg
}

/// Execute one tool call: gates → intercepts → cancel check → IPC → tracking.
pub fn execute_single_tool(
    agent: &mut AgentState,
    tc: &ToolCall,
    agent_tx: &mpsc::Sender<AgentToTui>,
) -> ToolOutcome {
    let name = &tc.function.name;
    let id = &tc.id;
    let args = &tc.function.arguments;

    if gates::phase_check_tool(agent, name, id) {
        emit_tool_result(
            agent_tx,
            id,
            name,
            "[BLOCKED] Phase gate prevented this tool.",
            false,
        );
        return ToolOutcome::Continue;
    }
    if gates::explore_gate(agent, name, id, args) {
        emit_tool_result(
            agent_tx,
            id,
            name,
            "[BLOCKED] Explore gate prevented this tool.",
            false,
        );
        return ToolOutcome::Continue;
    }
    if gates::re_read_gate(agent, name, id, args) {
        emit_tool_result(
            agent_tx,
            id,
            name,
            "[BLOCKED] Re-read gate prevented this tool.",
            false,
        );
        return ToolOutcome::Continue;
    }

    if name == "status" {
        let args_val: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
        let state = args_val
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("coding");
        if state == "explore" || state == "chat" {
            let err =
                format!("[ERROR] Mode '{state}' no longer exists. Use: plan, coding, debug");
            let _ = agent.ctx.push_tool_result(id, &err);
            agent.tool_results.push((id.to_string(), err.clone()));
            emit_tool_result(agent_tx, id, name, &err, false);
            return ToolOutcome::Continue;
        }
        let tp = match state {
            "plan" => dsx_types::TaskPhase::Plan,
            "coding" => dsx_types::TaskPhase::Coding,
            "debug" => dsx_types::TaskPhase::Debug,
            _ => dsx_types::TaskPhase::Coding,
        };
        let level = dsx_types::DebugLevel::Medium;
        agent.current_task_phase = tp;
        router::set_phase(tp, level);
        if agent.auto_mode {
            apply_phase_config(agent, tp, level);
        }
        let phase_name = format!("{:?}", tp).to_lowercase();
        let _ = agent_tx.send(AgentToTui::PhaseChanged {
            phase: phase_name,
        });
        let result = format!("[OK] Switched to {} mode", state);
        let _ = agent.ctx.push_tool_result(id, &result);
        agent.tool_results.push((id.to_string(), result.clone()));
        emit_tool_result(agent_tx, id, name, &result, true);
        return ToolOutcome::Continue;
    }

    if name == "ask_user" || name == "ask" {
        let args_val: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
        let question = args_val
            .get("question")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let options = args_val
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());
        let frame = AgentToTui::AskUser {
            id: id.to_string(),
            question,
            options,
        };
        let _ = agent_tx.send(frame);
        agent.pending_ask_user = Some(id.to_string());
        agent.tool_results.push((
            id.to_string(),
            "[ASK_USER] Awaiting your response.".to_string(),
        ));
        emit_tool_result(
            agent_tx,
            id,
            name,
            "[ASK_USER] Awaiting your response.",
            false,
        );
        return ToolOutcome::Continue;
    }

    if crate::tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
        crate::tools::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = agent.ctx.push_tool_result(
            &tc.id,
            "[CANCELLED] Tool execution cancelled by user.\n[HINT] This tool was not executed.",
        );
        emit_tool_result(
            agent_tx,
            id,
            name,
            "[CANCELLED] Tool execution cancelled by user.\n[HINT] This tool was not executed.",
            false,
        );
        return ToolOutcome::Continue;
    }

    let tr_content = crate::tools::execute_tool_with_id(name, "", args, id);
    if tr_content.contains("tools IPC") || tr_content.contains("not initialised") {
        let mut appender = ToolResultAppender::new(agent);
        appender.append(name, &tc.id, args, &tr_content);
        emit_tool_result(agent_tx, id, name, &tr_content, false);
        return ToolOutcome::Break;
    }

    let tr_success =
        !tr_content.starts_with("[ERROR]") && !tr_content.starts_with("[FAIL]");
    let failed = !tr_success;

    agent.health.record_tool_call(name);
    if failed {
        agent.tool_failures += 1;
    }

    {
        let mut appender = ToolResultAppender::new(agent);
        appender.append(name, &tc.id, args, &tr_content);
    }
    emit_tool_result(agent_tx, id, name, &tr_content, tr_success);

    if !failed && name == "file" && dsx_types::arg::tool_action(args) == "write" {
        tracker::track_file_written(agent, args);
    }
    if name == "exec" && dsx_types::arg::tool_action(args) == "explore" {
        agent.has_explored = true;
    }
    if name == "file" && dsx_types::arg::tool_action(args) == "read" {
        agent.turns_since_last_read = 0;
    }

    ToolOutcome::Executed
}

/// Handle a user input message with full module integration.
/// Sends AgentToTui events via the provided channel sender.
pub fn handle_user_input(
    agent: &mut AgentState,
    text: &str,
    hp: &mut BufReader<TcpStream>,
    agent_tx: &mpsc::Sender<AgentToTui>,
) {
    // ── Handle ask_user response ──
    if let Some(tool_call_id) = agent.pending_ask_user.take() {
        if let Err(e) = agent.ctx.push_tool_result(&tool_call_id, text) {
            log::error!("push_tool_result for ask_user failed: {:?} — text dropped", e);
        }
    } else {
        if text.is_empty() {
            return;
        }

        // ── Session init on first message ──
        if agent.session_seed.is_empty() {
            let seed = agent.resume_seed.clone();
            super::lifecycle::init_session(agent, seed.as_deref());
            if seed.is_some() {
                let msg_count = agent.ctx.message_count();
                let summary = agent
                    .ctx
                    .turns()
                    .last()
                    .and_then(|t| t.steps.last())
                    .and_then(|s| {
                        s.assistant.content.iter().find_map(|b| {
                            if let ContentBlock::Text { text } = b {
                                Some(text.chars().take(100).collect::<String>())
                            } else {
                                None
                            }
                        })
                    })
                    .unwrap_or_default();
                let tokens_used = agent.token_estimate;
                let cache_hit_pct = agent.cache_hit_pct;
                let _ = agent_tx.send(AgentToTui::SessionRestored {
                    seed: agent.session_seed.clone(),
                    message_count: msg_count as u64,
                    summary,
                    tokens_used,
                    cache_hit_pct,
                });
            }
            if agent.auto_mode {
                let phase = router::detect_initial_phase(text);
                let level = dsx_types::DebugLevel::Medium;
                router::set_phase(phase, level);
                agent.current_task_phase = phase;
                apply_phase_config(agent, phase, level);
                let phase_name = format!("{:?}", phase).to_lowercase();
                let _ = agent_tx.send(AgentToTui::PhaseChanged {
                    phase: phase_name,
                });
                log::info!(
                    "auto initial phase: {:?} model={} effort={:?}",
                    phase,
                    agent.config.model,
                    agent.config.effort
                );
            }
        }

        // ── Skill matching ──
        let matched = agent.skill_index.match_skills(text);
        agent.active_skill_bodies.clear();
        for skill in &matched {
            if let Some(body) = agent.skill_index.load_skill_body(&skill.name) {
                agent.active_skill_bodies.push((skill.name.clone(), body));
            }
        }

        // ── Push user message to ContextAssembler ──
        if let Err(e) = agent.ctx.push_user(text) {
            match e {
                AssemblerError::TurnIncomplete { .. } => {
                    log::warn!("push_user TurnIncomplete — repairing (cancellation deadlock)");
                    agent.ctx.remove_last_step_if_incomplete();
                    agent.ctx.push_user_restore(text);
                }
                _ => {
                    log::error!("push_user failed: {:?}", e);
                    return;
                }
            }
        }
    }

    // ── Reset per-turn state ──
    agent.tool_results.clear();
    agent.tool_failures = 0;
    agent.tool_calls_this_turn = 0;
    agent.files_written_this_turn.clear();

    // Live snapshot after user message
    session::save_live_snapshot(
        &agent.session_seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        agent.config.effort.as_deref(),
        None,
    );

    // ── Tool-calling loop ──
    for _round in 0..agent.max_tool_rounds {
        if agent.stream_cancelled {
            agent.stream_cancelled = false;
            agent.system_note("system", "用户终止了当前操作。".to_string());
            break;
        }

        let (system, messages, breakdown) = crate::assembly::build_context(agent);

        let _ = agent_tx.send(AgentToTui::CachePrediction {
            hit_rate: agent.predicted_cache_hit_pct,
        });

        let msgs_no_system: Vec<&Message> =
            messages.iter().filter(|m| m.role != "system").collect();
        log::debug!(
            "turn round={} messages={} tokens={}",
            _round,
            messages.len(),
            tokenizer::estimate_messages_tokens(&messages)
        );

        let chat = AgentToHp::ApiChat {
            model: agent.config.model.clone(),
            system: Some(system),
            messages: serde_json::to_value(&msgs_no_system).unwrap_or_default(),
            effort: agent.config.effort.clone(),
            max_tokens: Some(agent.config.max_tokens),
            tools: Some(serde_json::to_value(&agent.tool_defs).unwrap_or_default()),
            user_id: Some(agent.session_seed.clone()),
            api_key: Some(dsx_proto::Redacted(agent.config.api_key.clone())),
        };
        let _ = dsx_proto::write_frame(hp.get_mut(), &chat);

        let HpStreamResponse {
            mut content,
            reasoning_content,
            thinking_signature,
            usage,
            tool_calls_raw,
        } = match read_hp_stream_response(hp, agent, agent_tx, _round) {
            Ok(r) => r,
            Err(()) => {
                agent.stream_cancelled = false;
                return;
            }
        };


        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);

        if parsed.is_empty()
            && (content.contains("<tool_use>")
                || content.contains("<read>")
                || content.contains("<exec>")
                || content.contains("<write>")
                || content.contains("<search>"))
        {
            let tool_names: Vec<String> =
                agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let (cleaned, xml_tcs) =
                tool_parser::parse_xml_tool_calls(&content, &tool_names);
            content = cleaned;
            parsed = xml_tcs;
        }

        let has_tools = !parsed.is_empty();

        if let Some(ref u) = usage {
            agent.api_usage = Some(u.clone());
            agent.session_tokens += u.total_tokens as u64;
        }
        agent.token_estimate = breakdown.total;
        agent.token_breakdown = Some(breakdown);
        agent.health.context_tokens = agent.tokens_used();
        agent.health.context_tier = crate::health::ContextTier::from_tokens(
            agent.health.context_tokens,
            agent.config.context_limit,
        );

        if let Some(ref r) = reasoning_content {
            if agent.auto_mode {
                let tp = phase_detector::detect_task_phase_from_reasoning(r);
                if tp != agent.current_task_phase {
                    agent.current_task_phase = tp;
                    router::set_phase(tp, router::read_debug_level());
                    let level = dsx_types::DebugLevel::Medium;
                    apply_phase_config(agent, tp, level);
                    let phase_name = format!("{:?}", tp).to_lowercase();
                    let _ = agent_tx.send(AgentToTui::PhaseChanged {
                        phase: phase_name,
                    });
                }
            }
        }

        let assistant_msg = build_and_push_assistant(
            agent,
            &content,
            &reasoning_content,
            &thinking_signature,
            &parsed,
        );

        if !has_tools {
            let final_reasoning = (!agent.stream_reasoning.is_empty())
                .then(|| agent.stream_reasoning.clone())
                .or_else(|| reasoning_content.clone());
            agent.stream_content.clear();
            agent.stream_reasoning.clear();

            learning::auto_extract_memory(agent, &assistant_msg);

            agent.health.record_turn(false);
            agent.health_status_line = health_status(agent);
            agent.health.reset_turn();

            session::save_live_snapshot(
                &agent.session_seed,
                &agent.ctx.to_vec(),
                &agent.config.model,
                agent.config.effort.as_deref(),
                None,
            );
            crate::orchestrator::maybe_save_session(agent);

            let _ = agent_tx.send(AgentToTui::ApiResponse {
                content,
                tool_calls: None,
                stop_reason: None,
                usage,
                reasoning_content: final_reasoning,
            });
            return;
        }

        // ── Tool call round ──
        agent.tool_calls_this_turn += parsed.len() as u32;

        session::save_live_snapshot(
            &agent.session_seed,
            &agent.ctx.to_vec(),
            &agent.config.model,
            agent.config.effort.as_deref(),
            None,
        );

        for tc in &parsed {
            match execute_single_tool(agent, tc, agent_tx) {
                ToolOutcome::Break => break,
                ToolOutcome::Continue => continue,
                ToolOutcome::Executed => {}
            }

            let all_tool_calls: Vec<ToolCall> = agent
                .ctx
                .to_vec()
                .iter()
                .flat_map(|m| &m.content)
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { id, name, input } = b {
                        Some(ToolCall {
                            id: id.clone(),
                            call_type: "function".into(),
                            function: dsx_types::FunctionCall {
                                name: name.clone(),
                                arguments: input.to_string(),
                            },
                        })
                    } else {
                        None
                    }
                })
                .collect();
            let _ = agent_tx.send(AgentToTui::ToolState {
                explored: agent.has_explored,
                declared_files: all_tool_calls
                    .iter()
                    .filter(|tc| tc.function.name == "explore")
                    .map(|tc| format!("{}: {}", tc.function.name, tc.function.arguments))
                    .collect(),
                read_files: all_tool_calls
                    .iter()
                    .filter(|tc| tc.function.name == "read_file")
                    .map(|tc| tc.function.arguments.clone())
                    .collect(),
                written_this_turn: agent.files_written_this_turn.clone(),
            });
        }

        if agent.pending_ask_user.is_some() {
            break;
        }

        if agent.tool_failures >= 3 {
            log::warn!("safety gate: 3 cumulative tool failures");
            agent.turn_annotations.push(
                "[System] 3 consecutive tool failures. Respond with analysis — do not call more tools."
                    .to_string(),
            );
            agent.tool_failures = 0;
        }
    }

    // ── Check for pending ask_user before entering post-tool-loop ──
    if agent.pending_ask_user.is_some() {
        return;
    }

    // ── Post-tool-loop: one more API call to let the model wrap up ──
    if agent.max_tool_rounds > 0 {
        agent.turn_annotations.push(format!(
            "[System] Max tool rounds ({}) reached. Respond with what you have.",
            agent.max_tool_rounds
        ));
    }

    let max_post_rounds = 3u32;
    let mut sent_final_response = false;
    for _post_round in 0..max_post_rounds {
        let (system, messages, _breakdown) = crate::assembly::build_context(agent);

        let _ = agent_tx.send(AgentToTui::CachePrediction {
            hit_rate: agent.predicted_cache_hit_pct,
        });

        let messages_json = serde_json::to_value(&messages).unwrap_or_default();

        let chat = AgentToHp::ApiChat {
            model: agent.config.model.clone(),
            system: Some(system),
            messages: messages_json,
            effort: agent.config.effort.clone(),
            max_tokens: Some(agent.config.max_tokens),
            tools: None,
            user_id: Some(agent.session_seed.clone()),
            api_key: Some(dsx_proto::Redacted(agent.config.api_key.clone())),
        };
        let _ = dsx_proto::write_frame(hp.get_mut(), &chat);

        let HpStreamResponse {
            mut content,
            reasoning_content,
            thinking_signature,
            usage,
            tool_calls_raw,
        } = match read_hp_stream_response(hp, agent, agent_tx, _post_round) {
            Ok(r) => r,
            Err(()) => {
                agent.stream_cancelled = false;
                return;
            }
        };


        // ── Parse DSML/XML tool calls from content ──
        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);
        if content.contains("\u{ff5c}DSML\u{ff5c}tool_calls") {
            let (cleaned, dsml_tcs) =
                tool_parser::parse_dsml_tool_calls(&content, &agent.tool_defs);
            content = cleaned;
            parsed = dsml_tcs;
        }
        if parsed.is_empty()
            && (content.contains("<tool_use>")
                || content.contains("<read>")
                || content.contains("<exec>")
                || content.contains("<write>")
                || content.contains("<search>"))
        {
            let tool_names: Vec<String> =
                agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let (cleaned, xml_tcs) =
                tool_parser::parse_xml_tool_calls(&content, &tool_names);
            content = cleaned;
            parsed = xml_tcs;
        }

        if parsed.is_empty() {
            let assistant_msg = build_and_push_assistant(
                agent,
                &content,
                &reasoning_content,
                &thinking_signature,
                &parsed,
            );

            let final_reasoning = (!agent.stream_reasoning.is_empty())
                .then(|| agent.stream_reasoning.clone())
                .or_else(|| reasoning_content.clone());
            agent.stream_content.clear();
            agent.stream_reasoning.clear();

            learning::auto_extract_memory(agent, &assistant_msg);

            agent.health.record_turn(false);

            let _ = agent_tx.send(AgentToTui::ApiResponse {
                content,
                reasoning_content: final_reasoning,
                tool_calls: None,
                stop_reason: None,
                usage,
            });
            sent_final_response = true;
            break;
        }

        // ── DSML/XML tool calls found — execute them ──
        build_and_push_assistant(
            agent,
            &content,
            &reasoning_content,
            &thinking_signature,
            &parsed,
        );

        for tc in &parsed {
            match execute_single_tool(agent, tc, agent_tx) {
                ToolOutcome::Break => break,
                _ => {}
            }
        }
    }

    if !sent_final_response {
        let _ = agent_tx.send(AgentToTui::ApiResponse {
            content: format!(
                "[System] Max post-tool rounds ({}) reached.",
                max_post_rounds
            ),
            tool_calls: None,
            stop_reason: Some("max_rounds".into()),
            usage: None,
            reasoning_content: None,
        });
    }

    // End of post-tool-loop — finalize turn
    agent.health_status_line = health_status(agent);
    agent.health.reset_turn();

    session::save_live_snapshot(
        &agent.session_seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        agent.config.effort.as_deref(),
        None,
    );
    super::maybe_save_session(agent);
}
