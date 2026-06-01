//! Turn processing: context building, API chat, tool execution loop.
//!
//! This is the core of the agent — one turn = push user message → build
//! context → HP API call → parse tool calls → execute tools → repeat
//! until no more tool calls or max rounds reached.

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, AgentToHp, Agent2Ui};
use dsx_types::{ContentBlock, Message, ToolCall};

use crate::agent::{AgentState, ToolResultAppender};
use crate::assembly::AssemblerError;
use crate::orchestrator::{learning, tracker};
use crate::session;
use crate::tool_parser;

use super::hp_bridge::emit_tool_result;

/// Build an assistant message from LLM response parts and push to context.
pub fn build_and_push_assistant(
    agent: &mut AgentState,
    content: &str,
    reasoning_content: &Option<String>,
    parsed: &[ToolCall],
) -> Message {
    let mut blocks: Vec<ContentBlock> = Vec::new();
    if let Some(ref rc) = reasoning_content {
        if !rc.is_empty() {
            blocks.push(ContentBlock::Reasoning {
                reasoning: rc.clone(),
            });
        }
    }
    if !content.is_empty() {
        blocks.push(ContentBlock::Text {
            text: content.to_string(),
        });
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

    // Defensive: model returned zero text + zero tool calls + zero reasoning → placeholder
    if blocks.is_empty() {
        blocks.push(ContentBlock::Text {
            text: "[Empty response]".to_string(),
        });
    }
    let assistant_msg = Message {
        role: "assistant".into(),
        name: None,
        content: blocks,
    };

    if let Err(e) = agent.ctx.push_assistant(assistant_msg.clone()) {
        log::error!("push_assistant failed: {:?} — repairing", e);
        agent.ctx.push_assistant_restore(assistant_msg.clone());
    }

    assistant_msg
}

/// Process a pending ask_user reply, pushing the user's text as a tool result.
/// Returns `true` if a pending ask_user was handled (caller skips normal push flow).
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
/// Returns `false` on a fatal error (caller should abort the turn).
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

/// Initialize the session on the first user message: restore summary, auto-detect
/// initial phase, and apply phase config.
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
            let tokens_used = agent.token_estimate;
            let cache_hit_pct = 0.0;
            let _ = agent_tx.send(Agent2Ui::SessionRestored {
                seed: agent.session_seed.clone(),
                message_count: msg_count as u64,
                summary,
                tokens_used,
                cache_hit_pct,
            });
        }
    }
}

/// Run one API turn: build context → send ApiChat → read HP stream response.
///
/// When `allow_tools` is `true`, system messages are filtered out of the serialized
/// messages and `tool_defs` are sent; when `false`, all messages are included and
/// no tools are sent.
///
/// Returns `Err(())` to signal that the caller should exit `handle_user_input`
/// entirely (stream error or cancellation).
fn run_api_turn(
    agent: &mut AgentState,
    hp: &mut BufReader<TcpStream>,
    agent_tx: &mpsc::Sender<Agent2Ui>,
    round: u32,
    allow_tools: bool,
) -> Result<
    (
        String,
        Option<String>,
        serde_json::Value,
        Option<dsx_types::UsageInfo>,
    ),
    (),
> {
    let messages = crate::assembly::build_context(agent);

    let messages_json = {
        log::debug!(
            "turn round={} messages={}",
            round,
            messages.len(),
        );
        serde_json::to_value(&messages).unwrap_or_default()
    };

    let chat = AgentToHp::ApiChat {
        model: agent.config.model.clone(),
        system: None,
        messages: messages_json,
        effort: agent.config.effort.clone(),
        max_tokens: Some(agent.config.max_tokens),
        tools: if allow_tools {
            Some(serde_json::to_value(&agent.tool_defs).unwrap_or_default())
        } else {
            None
        },
        user_id: Some(agent.session_seed.clone()),
        api_key: Some(dsx_proto::Redacted(agent.config.api_key.clone())),
    };

    if let Err(e) = dsx_proto::write_frame(hp.get_mut(), &chat) {
        log::error!("dsx-agent: write_frame to HP failed: {}", e);
        let _ = agent_tx.send(Agent2Ui::Error {
            message: "Failed to communicate with HP daemon.".into(),
        });
        return Err(());
    }

    // Read HP frames one by one, push UI events and accumulate state in turn.rs
    loop {
        let frame = match super::hp_bridge::read_hp_frame(hp) {
            Ok(Some(f)) => f,
            Ok(None) | Err(..) => {
                let _ = agent_tx.send(Agent2Ui::Error {
                    message: "HP connection closed unexpectedly.".into(),
                });
                return Err(());
            }
        };

        match frame {
            dsx_proto::HpToAgent::ContentDelta { delta, reasoning } => {
                if agent.stream_cancelled
                    || dsx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst)
                {
                    log::info!("dsx-agent: streaming cancelled");
                    agent.stream_cancelled = false;
                    return Err(());
                }
                let _ = agent_tx.send(Agent2Ui::ContentDelta {
                    delta: delta.clone(),
                    reasoning: reasoning.clone(),
                });
                if let Some(ref r) = reasoning {
                    agent.stream_reasoning.push_str(r);
                }
                agent.stream_content.push_str(&delta);
            }
            dsx_proto::HpToAgent::ToolProgress { id, content: prog_content, stream_type, .. } => {
                let _ = agent_tx.send(Agent2Ui::ToolProgress {
                    id,
                    content: prog_content,
                    stream_type,
                });
            }
            dsx_proto::HpToAgent::ApiResponse {
                content, tool_calls, stop_reason: _, reasoning_content, usage,
            } => {
                return Ok((
                    content,
                    reasoning_content,
                    tool_calls.unwrap_or(serde_json::Value::Null),
                    usage,
                ));
            }
            dsx_proto::HpToAgent::Balance { is_available, total_balance, currency } => {
                let _ = agent_tx.send(Agent2Ui::Balance { is_available, total_balance, currency });
            }
            dsx_proto::HpToAgent::Error { message } => {
                let _ = agent_tx.send(Agent2Ui::Error { message: message.clone() });
                return Err(());
            }
            _ => {}
        }
    }
}

/// Handle a user input message with full module integration.
/// Sends Agent2Ui events via the provided channel sender.
pub fn handle_user_input(
    agent: &mut AgentState,
    text: &str,
    hp: &mut BufReader<TcpStream>,
    agent_tx: &mpsc::Sender<Agent2Ui>,
) {
    // ── Handle ask_user response ──
    if !process_ask_user_response(agent, text) {
        if text.is_empty() {
            return;
        }

        // ── Session init on first message ──
        init_session_on_first_message(agent, text, agent_tx);

        // ── Push user message to ContextAssembler ──
        if !push_user_message_with_repair(agent, text) {
            return;
        }
    }

    // ── Reset per-turn state ──
    agent.tool_results.clear();
    agent.tool_failures = 0;
    agent.tool_calls_this_turn = 0;
    agent.files_written_this_turn.clear();

    let mut ipc_broken = false;
    let mut max_rounds_exhausted = false;

    // ── Tool-calling loop ──
    for _round in 0..agent.max_tool_rounds {
        if ipc_broken {
            break;
        }
        if agent.stream_cancelled {
            agent.stream_cancelled = false;
            agent.system_note("system", "用户终止了当前操作。".to_string());
            break;
        }

        let (mut content, reasoning_content, tool_calls_raw, usage) =
            match run_api_turn(agent, hp, agent_tx, _round, true) {
                Ok(v) => v,
                Err(()) => return,
            };

        content = tool_parser::strip_fenced_code(&content);

        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);

        if parsed.is_empty()
            && (content.contains("\u{ff5c}DSML\u{ff5c}tool_calls")
                || content.contains("\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls"))
        {
            let (cleaned, dsml_tcs) =
                tool_parser::parse_dsml_tool_calls(&content, &agent.tool_defs);
            content = cleaned;
            parsed = dsml_tcs;
            agent.dsml_compat_count += parsed.len() as u32;
        }

        if parsed.is_empty()
            && (content.contains("<tool_use>")
                || content.contains("<invoke ")
                || content.contains("<tool_calls>")
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
            agent.dsml_compat_count += parsed.len() as u32;
        }

        let has_tools = !parsed.is_empty();

        if let Some(ref u) = usage {
            agent.api_usage = Some(u.clone());
            agent.session_tokens += u.total_tokens as u64;
        }

        let assistant_msg = build_and_push_assistant(
            agent,
            &content,
            &reasoning_content,
            &parsed,
        );

        if !has_tools {
            let final_reasoning = (!agent.stream_reasoning.is_empty())
                .then(|| agent.stream_reasoning.clone())
                .or_else(|| reasoning_content.clone());
            agent.stream_content.clear();
            agent.stream_reasoning.clear();

            learning::post_turn_maintenance(agent, &assistant_msg);

            agent.health.record_turn();
            agent.health.reset_turn();

            session::save_live_snapshot(
                &agent.session_seed,
                &agent.ctx.to_vec(),
                &agent.config.model,
                agent.config.effort.as_deref(),
            );

            let _ = agent_tx.send(Agent2Ui::ApiResponse {
                content,
                tool_calls: None,
                stop_reason: None,
                usage,
                reasoning_content: final_reasoning,
                context_tokens: agent.token_estimate,
                context_limit: agent.config.context_limit,
                session_tokens: agent.session_tokens,
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
        );

        // Pre-scan: mark explored if any tool call is exec(explore),
        // so read_file doesn't get blocked by explore_gate in the same batch.
        if !agent.has_explored {
            if parsed.iter().any(|tc| {
                tc.function.name == "exec"
                    && dsx_types::arg::tool_action(&tc.function.arguments) == "explore"
            }) {
                agent.has_explored = true;
            }
        }

        let results: Vec<(String, String, String, String)> =
            if parsed.len() > 1 && !parsed.iter().any(|tc| tc.function.name == "ask_user") {
            use std::thread;
            let mut handles = Vec::new();
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
            // Cancel check (before pushing result)
            if dsx_tools::CANCEL.compare_exchange(
                true, false,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ).is_ok()
            {
                let msg = "[CANCELLED] Tool execution cancelled by user.";
                let mut appender = ToolResultAppender::new(agent);
                appender.append(name, id, args, msg);
                emit_tool_result(agent_tx, id, name, msg, false, Some(args.clone()));
                for remaining_idx in tc_idx + 1..results.len() {
                    let (rn, ri, ra, _) = &results[remaining_idx];
                    let mut appender = ToolResultAppender::new(agent);
                    appender.append(rn, ri, ra, msg);
                    emit_tool_result(agent_tx, ri, rn, msg, false, Some(ra.clone()));
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

            // Push result to context (always — even for failed tools)
            {
                let mut appender = ToolResultAppender::new(agent);
                appender.append(name, id, args, tr_content);
            }
            emit_tool_result(agent_tx, id, name, tr_content, tr_success, Some(args.clone()));

            if !failed && name == "write_file" {
                tracker::track_file_written(agent, args);
            }
            if name == "read_file" {
                agent.turns_since_last_read = 0;
            }
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
        let _ = agent_tx.send(Agent2Ui::ToolState {
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

    // ── Check for pending ask_user before entering post-tool-loop ──
    if agent.pending_ask_user.is_some() {
        return;
    }

    // ── Post-tool-loop: one more API call to let the model wrap up ──
    if max_rounds_exhausted {
        agent.turn_annotations.push(format!(
            "[System] Max tool rounds ({}) reached. Respond with what you have.",
            agent.max_tool_rounds
        ));
    }

    // ── Post-tool-loop: single wrap-up call, parallel tool execution ──
    let (mut content, reasoning_content, tool_calls_raw, usage) =
        match run_api_turn(agent, hp, agent_tx, 0, false) {
            Ok(v) => v,
            Err(()) => return,
        };

    content = tool_parser::strip_fenced_code(&content);
    let mut post_parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);
    if content.contains("\u{ff5c}DSML\u{ff5c}tool_calls")
        || content.contains("\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls") {
        let (cleaned, dsml_tcs) =
            tool_parser::parse_dsml_tool_calls(&content, &agent.tool_defs);
        content = cleaned;
        post_parsed = dsml_tcs;
        agent.dsml_compat_count += post_parsed.len() as u32;
    }
    if post_parsed.is_empty()
        && (content.contains("<tool_use>")
            || content.contains("<invoke ")
            || content.contains("<tool_calls>")
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
        post_parsed = xml_tcs;
        agent.dsml_compat_count += post_parsed.len() as u32;
    }

    if post_parsed.is_empty() {
        let assistant_msg = build_and_push_assistant(agent, &content, &reasoning_content, &post_parsed);
        let final_reasoning = (!agent.stream_reasoning.is_empty())
            .then(|| agent.stream_reasoning.clone())
            .or_else(|| reasoning_content.clone());
        agent.stream_content.clear();
        agent.stream_reasoning.clear();
        learning::post_turn_maintenance(agent, &assistant_msg);
        agent.health.record_turn();

        let _ = agent_tx.send(Agent2Ui::ApiResponse {
            content,
            reasoning_content: final_reasoning,
            tool_calls: None,
            stop_reason: None,
            usage,
            context_tokens: agent.token_estimate,
            context_limit: agent.config.context_limit,
            session_tokens: agent.session_tokens,
        });
    } else {
        // Tools found — execute in parallel (same as main loop)
        build_and_push_assistant(agent, &content, &reasoning_content, &post_parsed);

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
            emit_tool_result(agent_tx, id, name, tr_content, tr_success, Some(args.clone()));

            if !failed && name == "write_file" {
                tracker::track_file_written(agent, args);
            }
            if name == "read_file" {
                agent.turns_since_last_read = 0;
            }
        }

        let _ = agent_tx.send(Agent2Ui::ApiResponse {
            content: String::new(),
            tool_calls: None,
            stop_reason: None,
            usage,
            reasoning_content: None,
            context_tokens: agent.token_estimate,
            context_limit: agent.config.context_limit,
            session_tokens: agent.session_tokens,
        });
    }

    // End of turn
    agent.health.reset_turn();

    session::save_live_snapshot(
        &agent.session_seed,
        &agent.ctx.to_vec(),
        &agent.config.model,
        agent.config.effort.as_deref(),
    );
}
