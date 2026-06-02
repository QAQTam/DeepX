//! Turn processing: user input → tool-calling loop → UI events → session save.

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::Agent2Ui;
use dsx_types::{ContentBlock, ToolCall};

use crate::agent::{AgentState, ToolResultAppender};
use crate::assembly::AssemblerError;
use crate::orchestrator::{learning, tracker};
use crate::session;
use crate::tool_parser;

use super::api_turn::run_api_turn;
use super::ui_emit::{build_and_push_assistant, make_tool_def, emit_tool_result};

/// Process a pending ask_user reply, pushing the user's text as a tool result.
fn process_ask_user_response(agent: &mut AgentState, text: &str) -> bool {
    if let Some(tool_call_id) = agent.pending_ask_user.take() {
        if let Err(e) = agent.ctx.push_tool_result(&tool_call_id, text) {
            log::error!("push_tool_result for ask_user failed: {:?} — text dropped", e);
        }
        true
    } else {
        false
    }
}

/// Push `text` to the ContextAssembler, repairing TurnIncomplete deadlocks.
fn push_user_message_with_repair(agent: &mut AgentState, text: &str) -> bool {
    match agent.ctx.push_user(text) {
        Ok(()) => true,
        Err(AssemblerError::TurnIncomplete { .. }) => {
            log::warn!("push_user TurnIncomplete — repairing (cancellation deadlock)");
            agent.ctx.remove_last_step_if_incomplete();
            agent.ctx.push_user_restore(text);
            true
        }
        Err(e) => {
            log::error!("push_user failed: {:?}", e);
            false
        }
    }
}

/// Initialize the session on the first user message.
fn init_session_on_first_message(
    agent: &mut AgentState,
    _text: &str,
    agent_tx: &mpsc::Sender<Agent2Ui>,
) {
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
            let _ = agent_tx.send(Agent2Ui::SessionRestored {
                seed: agent.session_seed.clone(),
                message_count: msg_count as u64,
                summary,
                tokens_used: agent.token_estimate,
                cache_hit_pct: 0.0,
            });
        }
    }
}

/// Handle a user input message with full module integration.
/// Emits structured Agent2Ui events in guaranteed order:
///   UserMsg → (AssistantMsg | ToolCall | ToolResult)* → TurnEnd
pub fn handle_user_input(
    agent: &mut AgentState,
    text: &str,
    hp: &mut BufReader<TcpStream>,
    agent_tx: &mpsc::Sender<Agent2Ui>,
) {
    if !process_ask_user_response(agent, text) {
        if text.is_empty() {
            return;
        }

        init_session_on_first_message(agent, text, agent_tx);

        if !push_user_message_with_repair(agent, text) {
            return;
        }
    }

    let turn_num = agent.health.turn.to_string();
    let user_msg_id = format!("u{}", turn_num);
    let _ = agent_tx.send(Agent2Ui::UserMsg {
        id: user_msg_id.clone(),
        text: text.to_string(),
    });

    agent.tool_failures = 0;
    agent.tool_calls_this_turn = 0;
    agent.files_written_this_turn.clear();

    // Inject current task progress into context (Layer 3 tail)
    agent.refresh_progress_context();

    // Auto-detect and activate matching skills
    crate::skills::auto_activate(agent, text);

    let mut ipc_broken = false;
    let mut max_rounds_exhausted = false;
    let mut msg_seq = 0u64;

    for _round in 0..agent.max_tool_rounds {
        if ipc_broken {
            break;
        }
        if agent.stream_cancelled {
            agent.stream_cancelled = false;
            agent.system_note("system", "用户终止了当前操作。".to_string());
            break;
        }

        let a_msg_id = format!("a{}-{}", turn_num, msg_seq);
        msg_seq += 1;

        let (content, reasoning_content, tool_calls_raw, usage, stop_reason) =
            match run_api_turn(agent, hp, agent_tx, &a_msg_id, true) {
                Ok(v) => v,
                Err(()) => return,
            };

        let stripped = tool_parser::strip_fenced_code(&content);

        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);

        let mut content = content;
        if parsed.is_empty()
            && (stripped.contains("\u{ff5c}DSML\u{ff5c}tool_calls")
                || stripped.contains("\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls"))
        {
            let (cleaned, dsml_tcs) =
                tool_parser::parse_dsml_tool_calls(&stripped, &agent.tool_defs);
            content = cleaned;
            parsed = dsml_tcs;
            agent.dsml_compat_count += parsed.len() as u32;
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
            agent.dsml_compat_count += parsed.len() as u32;
        }

        let has_tools = !parsed.is_empty();

        if let Some(ref u) = usage {
            agent.api_usage = Some(u.clone());
            agent.session_tokens += u.total_tokens as u64;
            agent.token_estimate = u.prompt_tokens;
        }

        let assistant_msg = build_and_push_assistant(agent, &content, &reasoning_content, &parsed);

        let _ = agent_tx.send(Agent2Ui::AssistantMsg {
            id: a_msg_id.clone(),
            thinking: reasoning_content.clone()
                .filter(|r| !r.is_empty()),
            text: content.clone(),
        });

        if !has_tools {
            if stop_reason.as_deref() == Some("length") {
                agent.stream_content.clear();
                agent.stream_reasoning.clear();
                log::info!("turn: stop_reason=length at r={}, nudging model to continue", _round);
                continue;
            }

            if content.trim().is_empty() {
                agent.stream_content.clear();
                agent.stream_reasoning.clear();
                agent.turn_annotations.push(
                    "[System] You produced reasoning but no visible response. Summarize your findings now."
                        .to_string(),
                );
                continue;
            }

            agent.stream_content.clear();
            agent.stream_reasoning.clear();

            learning::post_turn_maintenance(agent, &assistant_msg);

            super::title_gen::generate_title(agent, hp);

            agent.health.record_turn();
            agent.health.reset_turn();

            session::save_live_snapshot(
                &agent.session_seed,
                &agent.ctx.to_vec(),
                &agent.config.model,
                agent.config.effort.as_deref(),
            );

            let _ = agent_tx.send(Agent2Ui::TurnEnd {
                stop_reason,
                usage,
                context_tokens: agent.token_estimate,
                context_limit: agent.config.context_limit,
                session_tokens: agent.session_tokens,
            });
            return;
        }

        agent.tool_calls_this_turn += parsed.len() as u32;

        session::save_live_snapshot(
            &agent.session_seed,
            &agent.ctx.to_vec(),
            &agent.config.model,
            agent.config.effort.as_deref(),
        );

        if !agent.has_explored {
            if parsed.iter().any(|tc| {
                tc.function.name == "exec"
                    && dsx_types::arg::tool_action(&tc.function.arguments) == "explore"
            }) {
                agent.has_explored = true;
            }
        }

        for tc in &parsed {
            if tc.function.name == "ask_user" { continue; }
            let tool_def = make_tool_def(&tc.id, &tc.function.name, &tc.function.arguments);
            let _ = agent_tx.send(Agent2Ui::ToolCall {
                msg_id: a_msg_id.clone(),
                tool: tool_def,
            });
        }

        let results: Vec<(String, String, String, String)> =
            if parsed.len() > 1 && !parsed.iter().any(|tc| tc.function.name == "ask_user") {
            use std::thread;
            let mut handles = Vec::new();
        let tool_names: Vec<String> = parsed.iter()
            .filter(|tc| tc.function.name != "ask_user")
            .map(|tc| tc.function.name.clone())
            .collect();
        if !tool_names.is_empty() {
            let _ = agent_tx.send(Agent2Ui::StreamStart {
                msg_id: a_msg_id.clone(),
                kind: dsx_proto::StreamKind::ToolCalling,
                tool_names: tool_names.clone(),
            });
        }

        for tc in &parsed {
                let name = tc.function.name.clone();
                let args = tc.function.arguments.clone();
                let id = tc.id.clone();
                handles.push(thread::spawn(move || {
                    let result =
                        crate::tools::execute_tool_with_id(&name, "", &args, &id);
                    (name, id, args, result)
                }));
            }
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        } else {
            parsed.iter().map(|tc| {
                let result =
                    crate::tools::execute_tool_with_id(&tc.function.name, "", &tc.function.arguments, &tc.id);
                (tc.function.name.clone(), tc.id.clone(), tc.function.arguments.clone(), result)
            }).collect()
        };

        for (tc_idx, (name, id, args, tr_content)) in results.iter().enumerate() {
            if dsx_tools::CANCEL.compare_exchange(
                true, false,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ).is_ok()
            {
                let msg = "[CANCELLED] Tool execution cancelled by user.";
                let mut appender = ToolResultAppender::new(agent);
                appender.append(name, id, args, msg);
                emit_tool_result(agent_tx, id, msg, false, None);
                for remaining_idx in tc_idx + 1..results.len() {
                    let (rn, ri, ra, _) = &results[remaining_idx];
                    let mut appender = ToolResultAppender::new(agent);
                    appender.append(rn, ri, ra, msg);
                    emit_tool_result(agent_tx, ri, msg, false, None);
                }
                break;
            }

            let tr_success =
                !tr_content.starts_with("[ERROR]") && !tr_content.starts_with("[FAIL]");

            if tr_content.contains("tools IPC") || tr_content.contains("not initialised") {
                ipc_broken = true;
            }

            let failed = !tr_success;
            agent.health.record_tool_call();
            if failed {
                agent.tool_failures += 1;
            }

            {
                let mut appender = ToolResultAppender::new(agent);
                appender.append(name, id, args, tr_content);
            }
            emit_tool_result(agent_tx, id, tr_content, tr_success, None);

            if !failed && name == "write_file" {
                tracker::track_file_written(agent, args);
            }
            if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                agent.touch_file(&path);
            }
            if name == "delete_file" {
                if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                    agent.file_last_read.remove(&path);
                }
            }
        }

        let _ = agent_tx.send(Agent2Ui::StreamEnd {
            msg_id: a_msg_id.clone(),
            is_final: false,
        });

        for (name, id, args, _) in results.iter() {
            if name == "ask_user" {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                    let question = parsed.get("question")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let options = parsed.get("options")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>());
                    let options_field = if options.as_ref().map(|o| o.is_empty()).unwrap_or(true) { None } else { options };
                    let _ = agent_tx.send(Agent2Ui::AskUser {
                        id: id.clone(),
                        question,
                        options: options_field,
                    });
                    agent.pending_ask_user = Some(id.clone());
                }
                break;
            }
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
        max_rounds_exhausted = true;
    }

    if agent.pending_ask_user.is_some() {
        return;
    }

    if max_rounds_exhausted {
        agent.turn_annotations.push(format!(
            "[System] Max tool rounds ({}) reached. Respond with what you have.",
            agent.max_tool_rounds
        ));
    }

    let a_msg_id = format!("a{}-{}", turn_num, msg_seq);

    let (content, reasoning_content, tool_calls_raw, usage, stop_reason) =
        match run_api_turn(agent, hp, agent_tx, &a_msg_id, false) {
            Ok(v) => v,
            Err(()) => return,
        };

    let stripped = tool_parser::strip_fenced_code(&content);
    let mut post_parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);

    let mut content = content;
    if stripped.contains("\u{ff5c}DSML\u{ff5c}tool_calls")
        || stripped.contains("\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls") {
        let (cleaned, dsml_tcs) =
            tool_parser::parse_dsml_tool_calls(&stripped, &agent.tool_defs);
        content = cleaned;
        post_parsed = dsml_tcs;
        agent.dsml_compat_count += post_parsed.len() as u32;
    }
    if post_parsed.is_empty()
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
        post_parsed = xml_tcs;
        agent.dsml_compat_count += post_parsed.len() as u32;
    }

    if post_parsed.is_empty() {
        let final_assistant = build_and_push_assistant(agent, &content, &reasoning_content, &post_parsed);

        let _ = agent_tx.send(Agent2Ui::AssistantMsg {
            id: a_msg_id.clone(),
            thinking: reasoning_content.clone()
                .filter(|r| !r.is_empty()),
            text: content.clone(),
        });

        learning::post_turn_maintenance(agent, &final_assistant);
        super::title_gen::generate_title(agent, hp);
        agent.health.record_turn();
        if let Some(ref u) = usage {
            agent.session_tokens += (u.prompt_cache_miss_tokens + u.completion_tokens) as u64;
            agent.token_estimate = u.prompt_tokens;
        }
    } else {
        let _ = build_and_push_assistant(agent, &content, &reasoning_content, &post_parsed);

        let _ = agent_tx.send(Agent2Ui::AssistantMsg {
            id: a_msg_id.clone(),
            thinking: reasoning_content.clone()
                .filter(|r| !r.is_empty()),
            text: content.clone(),
        });

        for tc in &post_parsed {
            let tool_def = make_tool_def(&tc.id, &tc.function.name, &tc.function.arguments);
            let _ = agent_tx.send(Agent2Ui::ToolCall {
                msg_id: a_msg_id.clone(),
                tool: tool_def,
            });
        }

        let results: Vec<(String, String, String, String)> = {
            use std::thread;
            let mut handles = Vec::new();
            for tc in &post_parsed {
                let name = tc.function.name.clone();
                let args = tc.function.arguments.clone();
                let id = tc.id.clone();
                handles.push(thread::spawn(move || {
                    let result =
                        crate::tools::execute_tool_with_id(&name, "", &args, &id);
                    (name, id, args, result)
                }));
            }
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        };

        for (name, id, args, tr_content) in &results {
            let tr_success = !tr_content.starts_with("[ERROR]") && !tr_content.starts_with("[FAIL]");
            let failed = !tr_success;
            agent.health.record_tool_call();
            if failed { agent.tool_failures += 1; }

            let mut appender = ToolResultAppender::new(agent);
            appender.append(name, id, args, tr_content);
            emit_tool_result(agent_tx, id, tr_content, tr_success, None);

            if !failed && name == "write_file" {
                tracker::track_file_written(agent, args);
            }
            if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                agent.touch_file(&path);
            }
            if name == "delete_file" {
                if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                    agent.file_last_read.remove(&path);
                }
            }
        }

        super::title_gen::generate_title(agent, hp);

        let _ = agent_tx.send(Agent2Ui::TurnEnd {
            stop_reason: Some("tool_calls".to_string()),
            usage,
            context_tokens: agent.token_estimate,
            context_limit: agent.config.context_limit,
            session_tokens: agent.session_tokens,
        });
        return;
    }

    agent.health.reset_turn();

    super::title_gen::generate_title(agent, hp);

    let _ = agent_tx.send(Agent2Ui::TurnEnd {
        stop_reason,
        usage,
        context_tokens: agent.token_estimate,
        context_limit: agent.config.context_limit,
        session_tokens: agent.session_tokens,
    });

    session::save_live_snapshot(
        &agent.session_seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        agent.config.effort.as_deref(),
    );
}
