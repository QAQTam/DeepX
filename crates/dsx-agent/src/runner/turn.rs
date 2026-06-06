//! Turn processing (v5): user input → tool-calling loop → round-based UI events → session save.

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{Agent2Ui, ToolCallDef, ToolResultDef, TurnData, RoundData};
use dsx_types::{ContentBlock, ToolCall};

use crate::agent::{AgentState, ToolResultAppender};
use crate::orchestrator::{learning, tracker};
use crate::session;
use crate::tool_parser;

use super::api_turn::run_api_turn;
use super::ui_emit::{build_and_push_assistant, make_tool_def, make_tool_result};
use super::{build_documents, build_recent_edits, build_tasks, cache_tokens, emit};

/// Process a pending ask_user reply, pushes the user's text as a tool result.
fn process_ask_user_response(agent: &mut AgentState, text: &str) -> bool {
    if let Some(tool_call_id) = agent.pending_ask_user.take() {
        agent.ctx.replace_tool_result(&tool_call_id, text);
        true
    } else {
        false
    }
}

/// Push `text` to the ContextAssembler (auto-repairs cancellation deadlocks).
fn push_user_message_with_repair(agent: &mut AgentState, text: &str) {
    agent.ctx.push_user(text);
}

/// Initialize the session on the first user message.
fn init_session_on_first_message(
    agent: &mut AgentState,
    agent_tx: &mpsc::Sender<Agent2Ui>,
) {
    if agent.session.seed.is_empty() {
        let seed = agent.session.resume_seed.clone();
        super::lifecycle::init_session(agent, seed.as_deref());
        if seed.is_some() {
            // Build TurnData from existing context for SessionRestored
            let turns = build_turns_from_context(agent);
            emit(&agent_tx, Agent2Ui::SessionRestored {
                seed: agent.session.seed.clone(),
                turns,
                tokens_used: agent.token_estimate,
                cache_hit_pct: 0.0,
            });
        }
    }
}

/// Reconstruct TurnData from the existing context (for session resume).
pub(super) fn build_turns_from_context(agent: &AgentState) -> Vec<TurnData> {
    let mut turns = Vec::new();
    for (ti, turn) in agent.ctx.turns().iter().enumerate() {
        let mut rounds = Vec::new();
        let mut round_num = 0u32;
        for step in &turn.steps {
            let thinking = step.assistant.content.iter().find_map(|b| {
                if let ContentBlock::Reasoning { reasoning } = b {
                    Some(reasoning.clone())
                } else {
                    None
                }
            });
            let answer = step.assistant.content.iter().find_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.clone())
                } else {
                    None
                }
            });
            let tool_calls: Vec<ToolCallDef> = step.assistant.content.iter().filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some(ToolCallDef {
                        id: id.clone(),
                        name: name.clone(),
                        args_display: name.clone(),
                        args_json: input.to_string(),
                    })
                } else {
                    None
                }
            }).collect();
            let tool_results: Vec<ToolResultDef> = step.tool_results.iter().filter_map(|tr| {
                tr.content.iter().find_map(|b| {
                    if let ContentBlock::ToolResult { content, .. } = b {
                        Some(ToolResultDef {
                            tool_call_id: String::new(),
                            output: content.clone(),
                            success: true,
                            file: None,
                        })
                    } else {
                        None
                    }
                })
            }).collect();

            rounds.push(RoundData {
                round_num,
                thinking,
                answer,
                tool_calls,
                tool_results,
            });
            round_num += 1;
        }
        if !rounds.is_empty() {
            // Get user text from the turn's user message
            let user_text = turn.user.content.iter().find_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.clone())
                } else {
                    None
                }
            }).unwrap_or_default();
            turns.push(TurnData {
                turn_id: format!("t{}", ti + 1),
                user_text,
                rounds,
            });
        }
    }
    turns
}

/// Handle a user input message with full module integration.
/// Emits round-based Agent2Ui events in guaranteed order:
///   TurnStart → (RoundDelta* → RoundComplete → ToolResults)* → TurnEnd
pub fn handle_user_input(
    agent: &mut AgentState,
    text: &str,
    hp: &mut BufReader<TcpStream>,
    agent_tx: &mpsc::Sender<Agent2Ui>,
) {
    let is_ask_reply = process_ask_user_response(agent, text);

    if !is_ask_reply {
        if text.is_empty() {
            return;
        }

        init_session_on_first_message(agent, agent_tx);

        push_user_message_with_repair(agent, text);
    }

    let turn_num = agent.turn_count.to_string();
    let turn_id = format!("t{}", turn_num);

    // Only send TurnStart for new turns (not ask_user replies)
    if !is_ask_reply {
        emit(&agent_tx, Agent2Ui::TurnStart {
            turn_id: turn_id.clone(),
            user_text: text.to_string(),
        });
    }

    agent.turn.tool_failures = 0;
    agent.turn.tool_calls_this_turn = 0;
    agent.files.files_written_this_turn.clear();

    agent.refresh_progress_context();
    crate::skills::auto_activate(agent, text);

    let mut ipc_broken = false;
    let mut round_num = 0u32;

    loop {
        if ipc_broken {
            break;
        }
        if agent.turn.stream_cancelled {
            agent.turn.stream_cancelled = false;
            agent.system_note("system", "用户终止了当前操作。".to_string());
            break;
        }

        let (content, reasoning_content, tool_calls_raw, usage, stop_reason) =
            match run_api_turn(agent, hp, agent_tx, &turn_id, round_num, true) {
                Ok(v) => v,
                Err(()) => return,
            };

        let stripped = tool_parser::strip_fenced_code(&content);
        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);
        let mut content = content;
        let mut dsml_detected = false;
        let mut dsml_source: Vec<bool> = Vec::new();

        if parsed.is_empty() && tool_parser::has_dsml(&stripped) {
            dsml_detected = true;
            let (cleaned, dsml_tcs) =
                tool_parser::parse_dsml_tool_calls(&stripped, &agent.tool_defs);
            if !dsml_tcs.is_empty() {
                content = cleaned;
                parsed = dsml_tcs;
                dsml_source = vec![true; parsed.len()];
            } else {
                emit(&agent_tx, Agent2Ui::ToolNotice {
                    message: "DSML detected but no valid tool calls found.".into(),
                    level: "warn".into(),
                });
            }
        }
        if parsed.is_empty()
            && (stripped.contains("<tool_use>")
                || stripped.contains("<invoke ")
                || stripped.contains("<tool_calls>")
                || stripped.contains("<read>")
                || stripped.contains("<exec>")
                || stripped.contains("<write>")
                || stripped.contains("<search>"))
        {
            let tool_names: Vec<String> =
                agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let (cleaned, xml_tcs) =
                tool_parser::parse_xml_tool_calls(&stripped, &tool_names);
            content = cleaned;
            parsed = xml_tcs;
            if dsml_detected {
                dsml_source = vec![false; parsed.len()];
            }
        }

        let has_tools = !parsed.is_empty();

        if let Some(ref u) = usage {
            agent.api_usage = Some(u.clone());
            agent.session.tokens += u.total_tokens as u64;
            agent.token_estimate = u.prompt_tokens;
        }

        let assistant_msg = build_and_push_assistant(agent, &content, &reasoning_content, &parsed);

        // Build tool call defs for RoundComplete
        let tool_call_defs: Vec<ToolCallDef> = parsed.iter()
            .filter(|tc| tc.function.name != "ask_user")
            .map(|tc| make_tool_def(&tc.id, &tc.function.name, &tc.function.arguments))
            .collect();

        // Send RoundComplete
        emit(&agent_tx, Agent2Ui::RoundComplete {
            turn_id: turn_id.clone(),
            round_num,
            thinking: reasoning_content.clone().filter(|r| !r.is_empty()),
            answer: if has_tools { None } else { Some(content.clone()) },
            tool_calls: tool_call_defs.clone(),
            is_final: !has_tools,
        });

        if !has_tools {
            if stop_reason.as_deref() == Some("length") {
                log::info!("turn: stop_reason=length at r={}, nudging model to continue", round_num);
                round_num += 1;
                continue;
            }

            if content.trim().is_empty() {
                agent.turn.annotations.push(
                    "[System] You produced reasoning but no visible response. Summarize your findings now."
                        .to_string(),
                );
                round_num += 1;
                continue;
            }

            learning::post_turn_maintenance(agent, &assistant_msg);

            save_snapshot(agent);

            emit(&agent_tx, Agent2Ui::TurnEnd {
                turn_id: turn_id.clone(),
                stop_reason,
                usage,
                context_tokens: agent.token_estimate,
                context_limit: agent.config.context_limit,
                session_tokens: agent.session.tokens,
            });
            return;
        }

        agent.turn.tool_calls_this_turn += parsed.len() as u32;

        save_snapshot(agent);

        if !agent.files.has_explored {
            if parsed.iter().any(|tc| {
                tc.function.name == "exec"
                    && dsx_types::arg::tool_action(&tc.function.arguments) == "explore"
            }) {
                agent.files.has_explored = true;
            }
        }

        // Execute tools and collect results (with interrupt support)
        let results: Vec<(String, String, String, crate::tools::ToolExecResult)> =
            if parsed.len() > 1 && !parsed.iter().any(|tc| tc.function.name == "ask_user") {
                use std::thread;
                let mut handles = Vec::new();
                for tc in &parsed {
                    let name = tc.function.name.clone();
                    let args = tc.function.arguments.clone();
                    let id = tc.id.clone();
                    handles.push(thread::spawn(move || {
                        let result =
                            crate::tools::execute_tool_with_id_full(&name, "", &args, &id);
                        (name, id, args, result)
                    }));
                }
                handles.into_iter().map(|h| h.join().unwrap_or_else(|e| {
                    let msg = format!("[ERROR] tool thread panicked: {:?}", e.downcast_ref::<&str>().unwrap_or(&"unknown"));
                    (String::new(), String::new(), String::new(), crate::tools::ToolExecResult { content: msg, interrupt: None })
                })).collect()
            } else {
                parsed.iter().map(|tc| {
                    let result =
                        crate::tools::execute_tool_with_id_full(&tc.function.name, "", &tc.function.arguments, &tc.id);
                    (tc.function.name.clone(), tc.id.clone(), tc.function.arguments.clone(), result)
                }).collect()
            };

        let mut tool_result_defs: Vec<ToolResultDef> = Vec::new();

        for (tc_idx, (name, id, args, tr_result)) in results.iter().enumerate() {
            if dsx_tools::CANCEL.compare_exchange(
                true, false,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ).is_ok()
            {
                let msg = "[CANCELLED] Tool execution cancelled by user.";
                let mut appender = ToolResultAppender::new(agent);
                appender.append(name, id, args, msg);
                tool_result_defs.push(make_tool_result(id, msg, false, None));
                for remaining_idx in tc_idx + 1..results.len() {
                    let (rn, ri, ra, _) = &results[remaining_idx];
                    let mut appender = ToolResultAppender::new(agent);
                    appender.append(rn, ri, ra, msg);
                    tool_result_defs.push(make_tool_result(ri, msg, false, None));
                }
                break;
            }

            let tr_content = &tr_result.content;
            let tr_success =
                !tr_content.starts_with("[ERROR]") && !tr_content.starts_with("[FAIL]");

            if tr_content.contains("tools IPC") || tr_content.contains("not initialised") {
                ipc_broken = true;
            }

            let failed = !tr_success;
            agent.turn.tool_calls_this_turn += 1;
            if failed {
                agent.turn.tool_failures += 1;
            }

            {
                let mut appender = ToolResultAppender::new(agent);
                appender.append(name, id, args, tr_content);
            }
            tool_result_defs.push(make_tool_result(id, tr_content, tr_success, None));

            // Emit audit record for InfoPanel real-time tool log
            let summary = tr_content.lines().next().unwrap_or(tr_content);
            emit(&agent_tx, Agent2Ui::AuditRecord {
                tool_name: name.clone(),
                result_summary: summary.chars().take(120).collect(),
                success: tr_success,
            });

            if tc_idx < dsml_source.len() && dsml_source[tc_idx] {
                if tr_success {
                    agent.dsml_compat_count += 1;
                } else {
                    let short = tr_content.chars().take(120).collect::<String>();
                    emit(&agent_tx, Agent2Ui::ToolNotice {
                        message: format!("DSML tool '{name}' failed: {short}"),
                        level: "error".into(),
                    });
                }
            }

            if !failed && name == "write_file" {
                tracker::track_file_written(agent, args);
            }
            if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                if matches!(name.as_str(), "write_file" | "edit_file") {
                    agent.mark_file_written(&path);
                } else {
                    agent.touch_file(&path);
                }
            }
            if name == "delete_file" {
                if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                    agent.files.file_read_at.remove(&path);
                    agent.files.file_written_at.remove(&path);
                }
            }
        }

        // Send collected tool results
        if !tool_result_defs.is_empty() {
            emit(&agent_tx, Agent2Ui::ToolResults {
                turn_id: turn_id.clone(),
                round_num,
                results: tool_result_defs,
            });
        }

        // Emit real-time debug snapshot
        emit(&agent_tx, Agent2Ui::DebugSnapshot {
            hp_connected: true,
            session_seed: agent.session.seed.clone(),
            context_tokens: agent.token_estimate,
            tool_calls_total: agent.turn.tool_calls_this_turn,
            tool_failures: agent.turn.tool_failures as u32,
            current_phase: "tool_batch".to_string(),
            streaming: false,
            dsml_compat_count: agent.dsml_compat_count,
            documents: build_documents(agent),
            recent_edits: build_recent_edits(agent),
            tasks: build_tasks(agent),
            session_title: agent.session.title.clone(),
            prompt_cache_hit_tokens: cache_tokens(agent).0,
            prompt_cache_miss_tokens: cache_tokens(agent).1,
        });

        // ── Generic interrupt: any tool can request user input ──
        // Check all results for interrupt requests (not just ask_user).
        for (_name, id, _args, tr_result) in results.iter() {
            if let Some(ir) = &tr_result.interrupt {
                let options_field = if ir.options.is_empty() { None } else { Some(ir.options.clone()) };
                emit(&agent_tx, Agent2Ui::AskUser {
                    id: id.clone(),
                    question: ir.prompt.clone(),
                    options: options_field,
                });
                agent.pending_ask_user = Some(id.clone());
                break;
            }
        }

        if agent.pending_ask_user.is_some() {
            break;
        }

        if agent.turn.tool_failures >= 3 {
            log::warn!("safety gate: 3 cumulative tool failures");
            agent.turn.annotations.push(
                "[System] 3 consecutive tool failures. Respond with analysis — do not call more tools."
                    .to_string(),
            );
            agent.turn.tool_failures = 0;
        }

        round_num += 1;
    }

    if agent.pending_ask_user.is_some() {
        return;
    }

    if ipc_broken {
        emit(&agent_tx, Agent2Ui::TurnEnd {
            turn_id: turn_id.clone(),
            stop_reason: Some("error".to_string()),
            usage: None,
            context_tokens: agent.token_estimate,
            context_limit: agent.config.context_limit,
            session_tokens: agent.session.tokens,
        });
        return;
    }

    emit(&agent_tx, Agent2Ui::TurnEnd {
        turn_id: turn_id.clone(),
        stop_reason: Some("cancelled".to_string()),
        usage: None,
        context_tokens: agent.token_estimate,
        context_limit: agent.config.context_limit,
        session_tokens: agent.session.tokens,
    });

    save_snapshot(agent);
}

/// Save the current session snapshot to disk (crash recovery).
fn save_snapshot(agent: &AgentState) {
    session::save_live_snapshot(
        &agent.session.seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        agent.config.effort.as_deref(),
    );
}
