//! dsx-agent — central process. Connects TUI ↔ Agent ↔ Tools ↔ HP.
//!
//! Wires all intelligence modules (session, memory, orchestrator, context,
//! skills, router, tokenizer, health) into the HP-bridged conversation loop.
//!
//! Uses shared dsx-proto types for all IPC channels (TUI, Tools, HP).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use dsx_proto::{
    self, TuiToAgent, AgentToTui, AgentToTools, ToolsToAgent, AgentToHp, HpToAgent,
};

use crate::agent::{AgentState, ToolResultAppender};
use crate::assembly::AssemblerError;
use crate::config::Config;
use crate::dsc_log;
use crate::orchestrator::{gates, learning, phase_detector, tracker, turn_scorer, session_persistence};
use crate::router;
use crate::session;
use crate::tokenizer;
use crate::tools;
use crate::tool_parser;
use dsx_types::{ContentBlock, Message, ToolCall};

// ── Session initialization ──

fn init_session(agent: &mut AgentState, restore_seed: Option<&str>) {
    // Check if a specific session seed was requested via CLI
    if let Some(seed) = restore_seed {
        if let Some(file) = session::load_session(seed) {
            agent.session_seed = file.seed.clone();
            agent.session_start = file.created_at;
            let (ctx, repairs) = crate::assembly::ContextAssembler::from_legacy(file.messages);
            agent.ctx = ctx;
        
            dsc_log::set_session(&agent.session_seed);
            tools::set_current_session(&agent.session_seed);
            eprintln!("dsx-agent: restored session {} ({} msgs)", agent.session_seed, agent.ctx.message_count());
            if !repairs.is_empty() { log::warn!("session restore: {:?} repairs", repairs); }
            return;
        }
        eprintln!("dsx-agent: session {seed} not found, creating new");
    }
    agent.session_seed = session::generate_seed();
    agent.session_start = session::now_epoch();
    dsc_log::set_session(&agent.session_seed);
    tools::set_current_session(&agent.session_seed);
    session::save_live_snapshot(
        &agent.session_seed, &agent.ctx.to_vec(),
        &agent.config.model, agent.config.effort.as_deref(), None);
    eprintln!("dsx-agent: new session {}", agent.session_seed);
}

fn health_status(agent: &AgentState) -> String {
    let assessment = agent.health.assess();
    format!(
        "[{} {} {} | tier:{:?} | {}% | t{}]",
        if assessment.level == crate::health::HealthLevel::Red { "RED" }
        else if assessment.level == crate::health::HealthLevel::Yellow { "YLW" }
        else { "OK" },
        assessment.emotion.emoji(),
        assessment.emotion.label(),
        agent.health.context_tier,
        (assessment.success_rate * 100.0) as u32,
        agent.health.turn,
    )
}

// ── Helper: apply phase config with user overrides ──

fn apply_phase_config(agent: &mut AgentState, phase: dsx_types::TaskPhase, level: dsx_types::DebugLevel) {
    let phase_name = format!("{:?}", phase).to_lowercase();
    if let Some(user_pc) = agent.config.phase_configs.get(&phase_name) {
        agent.config.model = user_pc.model.clone();
        agent.config.effort = user_pc.effort.clone().filter(|e| !e.is_empty());
        agent.config.max_tokens = user_pc.max_tokens;
        agent.config.context_limit = user_pc.context_limit;
    } else {
        let pc = router::phase_config(phase, level);
        agent.config.model = pc.model.to_string();
        agent.config.effort = pc.effort.map(|s| s.to_string());
        agent.config.max_tokens = pc.max_tokens;
    }
}

// ── Helper: write a tool_result JSON-LP frame to TUI ──

fn emit_tool_result(w: &mut impl Write, id: &str, name: &str, content: &str, success: bool) {
    let frame = serde_json::json!({
        "type": "tool_result",
        "id": id,
        "name": name,
        "content": content,
        "success": success,
    });
    let _ = writeln!(w, "{}", serde_json::to_string(&frame).unwrap_or_default());
    let _ = w.flush();
}

// ── Shared helpers ──

struct HpStreamResponse {
    content: String,
    reasoning_content: Option<String>,
    thinking_signature: Option<String>,
    usage: Option<dsx_types::UsageInfo>,
    tool_calls_raw: serde_json::Value,
}

/// Read HP streaming response until ApiResponse is received.
/// Forwards ContentDelta / ToolProgress frames to TUI.
fn read_hp_stream_response(
    hp: &mut BufReader<TcpStream>,
    agent: &mut AgentState,
    tui_writer: &mut impl Write,
    round: u32,
) -> Result<HpStreamResponse, ()> {
    let content: String;
    let reasoning_content: Option<String>;
    let thinking_signature: Option<String>;
    let usage: Option<dsx_types::UsageInfo>;
    let tcs_raw: serde_json::Value;

    loop {
        let hp_resp: HpToAgent = match dsx_proto::read_frame(hp) {
            Ok(Some(r)) => r,
            Ok(None) => {
                eprintln!("dsx-agent: HP connection closed (EOF)");
                agent.health.record_api_error();
                return Err(());
            }
            Err(e) => {
                eprintln!("dsx-agent: HP parse error: {e}");
                agent.health.record_api_error();
                return Err(());
            }
        };

        match hp_resp {
            HpToAgent::ContentDelta { delta, reasoning } => {
                if agent.stream_cancelled || crate::tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                    eprintln!("dsx-agent: streaming cancelled");
                    return Err(());
                }
                if round == 0 {
                    eprintln!("dsx DEBUG: hp.ContentDelta d={} r={}", delta.len(), reasoning.as_ref().map(|s| s.len()).unwrap_or(0));
                }

                let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::ContentDelta {
                    delta: delta.clone(),
                    reasoning: reasoning.clone(),
                });
                if let Some(ref r) = reasoning {
                    agent.stream_reasoning.push_str(r);
                }
                agent.stream_content.push_str(&delta);
            }
            HpToAgent::ToolProgress { id, content: prog_content, stream_type } => {
                let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::ToolProgress {
                    id: id.clone(),
                    content: prog_content.clone(),
                    stream_type: stream_type.clone(),
                });
            }
            HpToAgent::ApiResponse {
                content: c, tool_calls, stop_reason: _,
                reasoning_content: rc, thinking_signature: ts, usage: u,
            } => {
                content = c;
                tcs_raw = tool_calls.unwrap_or(serde_json::Value::Null);
                reasoning_content = rc;
                thinking_signature = ts;
                usage = u;
                return Ok(HpStreamResponse { content, reasoning_content, thinking_signature, usage, tool_calls_raw: tcs_raw });
            }
            HpToAgent::Error { message } => {
                let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::Error { message: message.clone() });
                agent.health.record_api_error();
                return Err(());
            }
            _ => { /* ignore non-stream frames */ }
        }
    }
}

fn build_and_push_assistant(
    agent: &mut AgentState,
    content: &str,
    reasoning_content: &Option<String>,
    thinking_signature: &Option<String>,
    parsed: &[ToolCall],
) -> Message {
    let mut blocks: Vec<ContentBlock> = Vec::new();
    if !content.is_empty() || parsed.is_empty() {
        blocks.push(ContentBlock::Text { text: content.to_string() });
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
        let input: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
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

/// Outcome of execute_single_tool for the caller's loop control.
enum ToolOutcome {
    Continue,
    Executed,
    Break,
}

/// Execute one tool call: gates → intercepts → cancel check → IPC execution → tracking.
/// Returns `Executed` if the tool actually ran (caller may emit ToolState),
/// `Continue` if gated/intercepted (caller should skip ToolState),
/// `Break` if IPC is dead (caller should break the tool loop).
fn execute_single_tool(
    agent: &mut AgentState,
    tc: &ToolCall,
    tui_writer: &mut impl Write,
) -> ToolOutcome {
    let name = &tc.function.name;
    let id = &tc.id;
    let args = &tc.function.arguments;

    if gates::phase_check_tool(agent, name, id) {
        emit_tool_result(tui_writer, id, name, "[BLOCKED] Phase gate prevented this tool.", false);
        return ToolOutcome::Continue;
    }
    if gates::explore_gate(agent, name, id, args) {
        emit_tool_result(tui_writer, id, name, "[BLOCKED] Explore gate prevented this tool.", false);
        return ToolOutcome::Continue;
    }
    if let Some(err_msg) = gates::pre_tool_health_check(agent, name) {
        let _ = agent.ctx.push_tool_result(id, &err_msg);
        agent.health.record_error(name, &err_msg);
        agent.tool_failures += 1;
        emit_tool_result(tui_writer, id, name, &err_msg, false);
        return ToolOutcome::Continue;
    }

    if name == "status" {
        let args_val: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
        let state = args_val.get("state").and_then(|v| v.as_str()).unwrap_or("coding");
        if state == "explore" || state == "chat" {
            let err = format!("[ERROR] Mode '{state}' no longer exists. Use: plan, coding, debug");
            let _ = agent.ctx.push_tool_result(id, &err);
            agent.tool_results.push((id.to_string(), err.clone()));
            emit_tool_result(tui_writer, id, name, &err, false);
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
        let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::PhaseChanged { phase: phase_name });
        let result = format!("[OK] Switched to {} mode", state);
        let _ = agent.ctx.push_tool_result(id, &result);
        agent.tool_results.push((id.to_string(), result.clone()));
        emit_tool_result(tui_writer, id, name, &result, true);
        return ToolOutcome::Continue;
    }

    if name == "ask_user" || name == "ask" {
        let args_val: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
        let question = args_val.get("question")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let options = args_val.get("options")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
            .filter(|v| !v.is_empty());
        let frame = AgentToTui::AskUser {
            id: id.to_string(),
            question,
            options,
        };
        let _ = dsx_proto::write_frame(tui_writer, &frame);
        agent.pending_ask_user = Some(id.to_string());
        agent.tool_results.push((id.to_string(), "[ASK_USER] Awaiting your response.".to_string()));
        emit_tool_result(tui_writer, id, name, "[ASK_USER] Awaiting your response.", false);
        return ToolOutcome::Continue;
    }

    if crate::tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
        crate::tools::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = agent.ctx.push_tool_result(&tc.id, "[CANCELLED] Tool execution cancelled by user.\n[HINT] This tool was not executed.");
        emit_tool_result(tui_writer, id, name, "[CANCELLED] Tool execution cancelled by user.\n[HINT] This tool was not executed.", false);
        return ToolOutcome::Continue;
    }

    let tr_content = crate::tools::execute_tool_with_id(name, "", args, id);
    if tr_content.contains("tools IPC") || tr_content.contains("not initialised") {
        let mut appender = ToolResultAppender::new(agent);
        appender.append(name, &tc.id, args, &tr_content);
        emit_tool_result(tui_writer, id, name, &tr_content, false);
        return ToolOutcome::Break;
    }

    let tr_success = !tr_content.starts_with("[ERROR]") && !tr_content.starts_with("[FAIL]");
    let failed = !tr_success;

    agent.health.record_tool_call(name);
    agent.monitor.tool_calls_this_turn += 1;
    if failed {
        agent.tool_failures += 1;
        agent.health.record_error(name, &tr_content);
    }
    gates::post_tool_health_record(agent, name, !failed);

    {
        let mut appender = ToolResultAppender::new(agent);
        appender.append(name, &tc.id, args, &tr_content);
    }
    emit_tool_result(tui_writer, id, name, &tr_content, tr_success);

    tracker::track_tool_code(agent, name, args, &tr_content);
    if !failed && name == "file"
        && crate::orchestrator::arg_parser::tool_action(args) == "write"
    {
        tracker::track_file_written(agent, args);
    }
    if name == "exec" && crate::orchestrator::arg_parser::tool_action(args) == "explore" {
        agent.has_explored = true;
    }
    if name == "file" && crate::orchestrator::arg_parser::tool_action(args) == "read" {
        agent.turns_since_last_read = 0;
    }

    ToolOutcome::Executed
}

// ── User input handler (fully wired) ──

/// Handle a TuiToAgent::UserInput frame with full module integration.
#[allow(unused_variables)]
fn handle_user_input(
    agent: &mut AgentState,
    text: &str,
    hp: &mut BufReader<TcpStream>,
    tui_writer: &mut impl Write,
    _tui_reader: &mut impl BufRead,
) {
    // ── Handle ask_user response ──
    if let Some(tool_call_id) = agent.pending_ask_user.take() {
        if let Err(e) = agent.ctx.push_tool_result(&tool_call_id, text) {
            log::error!("push_tool_result for ask_user failed: {:?} — text dropped", e);
        }
        // Skip session init, skill matching, push_user — this is a tool continuation
    } else {
        if text.is_empty() { return; }

        // ── Session init on first message ──
        if agent.session_seed.is_empty() {
            let seed = agent.resume_seed.clone();
            init_session(agent, seed.as_deref());
            if seed.is_some() {
                let msg_count = agent.ctx.message_count();
                let summary = agent.ctx.turns().last()
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
                dsx_proto::write_line(tui_writer, &format!(
                    r#"{{"type":"session_restored","seed":"{}","message_count":{},"summary":"{}","tokens_used":{},"cache_hit_pct":{}}}"#,
                    agent.session_seed, msg_count,
                    summary.replace('"', "\\\"").replace('\n', "\\n"),
                    tokens_used, cache_hit_pct,
                ));
            }
            if agent.auto_mode {
                let phase = router::detect_initial_phase(text);
                let level = dsx_types::DebugLevel::Medium;
                router::set_phase(phase, level);
                agent.current_task_phase = phase;
                apply_phase_config(agent, phase, level);
                let phase_name = format!("{:?}", phase).to_lowercase();
                let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::PhaseChanged { phase: phase_name });
                log::info!("auto initial phase: {:?} model={} effort={:?}", phase, agent.config.model, agent.config.effort);
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
                    // Recovery: cancellation deadlock where the last turn has no assistant
                    // response (or unfulfilled tool calls). Remove the broken step and
                    // append text to the existing user message so the conversation continues.
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
    agent.tool_code_content.clear();
    agent.tool_code_path.clear();
    agent.tool_code_action.clear();
    agent.tool_code_status = None;
    agent.tool_failures = 0;
    agent.tool_calls_this_turn = 0;
    agent.files_written_this_turn.clear();
    agent.monitor.tool_calls_this_turn = 0;

    // Live snapshot after user message
    session::save_live_snapshot(
        &agent.session_seed, &agent.ctx.to_vec(),
        &agent.config.model, agent.config.effort.as_deref(), None);

    // ── Tool-calling loop ──
    for _round in 0..agent.max_tool_rounds {
        if agent.stream_cancelled {
            agent.stream_cancelled = false;
            agent.system_note("system", "用户终止了当前操作。".to_string());
            break;
        }

        let (system, messages, breakdown) =
            crate::assembly::build_context(agent);

        let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::CachePrediction { hit_rate: agent.predicted_cache_hit_pct });

        let msgs_no_system: Vec<&Message> = messages.iter().filter(|m| m.role != "system").collect();
        let messages_json = serde_json::to_value(&msgs_no_system).unwrap_or_default();
        log::debug!("turn round={} messages={} tokens={}", _round, messages.len(), tokenizer::estimate_messages_tokens(&messages));

        let chat = AgentToHp::ApiChat {
            model: agent.config.model.clone(),
            system: Some(system),
            messages: messages_json,
            effort: agent.config.effort.clone(),
            max_tokens: Some(agent.config.max_tokens),
            tools: Some(serde_json::to_value(&agent.tool_defs).unwrap_or_default()),
            user_id: Some(agent.session_seed.clone()),
        };
        let _ = dsx_proto::write_frame(hp.get_mut(), &chat);

        let HpStreamResponse { mut content, reasoning_content, thinking_signature, usage, tool_calls_raw } =
            match read_hp_stream_response(hp, agent, tui_writer, _round) {
                Ok(r) => r,
                Err(()) => return,
            };

        agent.health.record_api_success(&agent.config.model);

        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);

        if parsed.is_empty() && (content.contains("<tool_use>") || content.contains("<read>") || content.contains("<exec>") || content.contains("<write>") || content.contains("<search>")) {
            let tool_names: Vec<String> = agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let (cleaned, xml_tcs) = tool_parser::parse_xml_tool_calls(&content, &tool_names);
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
            agent.health.context_tokens, agent.config.context_limit,
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
                    let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::PhaseChanged { phase: phase_name });
                }
            }
        }

        let assistant_msg = build_and_push_assistant(agent, &content, &reasoning_content, &thinking_signature, &parsed);

        if !has_tools {
            let final_reasoning = (!agent.stream_reasoning.is_empty())
                .then(|| agent.stream_reasoning.clone())
                .or_else(|| reasoning_content.clone());
            agent.turn_scores.push(turn_scorer::score_current_turn(agent));
            agent.stream_content.clear();
            agent.stream_reasoning.clear();

            learning::auto_extract_memory(agent, &assistant_msg);

            agent.health.record_turn(false);
            agent.health_status_line = health_status(agent);
            agent.health.reset_turn();

            session::save_live_snapshot(
                &agent.session_seed, &agent.ctx.to_vec(),
                &agent.config.model, agent.config.effort.as_deref(), None,
            );
            session_persistence::maybe_save_session(agent);

            let tui_resp = AgentToTui::ApiResponse {
                content,
                tool_calls: None,
                stop_reason: None,
                usage,
                reasoning_content: final_reasoning,
            };
            let _ = dsx_proto::write_frame(tui_writer, &tui_resp);
            return;
        }

        // ── Tool call round ──
        agent.tool_calls_this_turn += parsed.len() as u32;
        agent.monitor.tool_calls_this_turn = agent.tool_calls_this_turn;

        session::save_live_snapshot(
            &agent.session_seed, &agent.ctx.to_vec(),
            &agent.config.model, agent.config.effort.as_deref(), None,
        );

        for tc in &parsed {
            match execute_single_tool(agent, tc, tui_writer) {
                ToolOutcome::Break => break,
                ToolOutcome::Continue => continue,
                ToolOutcome::Executed => {}
            }

            let all_tool_calls: Vec<ToolCall> = agent.ctx.to_vec().iter()
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
            let tool_state_frame = AgentToTui::ToolState {
                explored: agent.has_explored,
                declared_files: all_tool_calls.iter()
                    .filter(|tc| tc.function.name == "explore")
                    .map(|tc| format!("{}: {}", tc.function.name, tc.function.arguments))
                    .collect(),
                read_files: all_tool_calls.iter()
                    .filter(|tc| tc.function.name == "read_file")
                    .map(|tc| tc.function.arguments.clone())
                    .collect(),
                written_this_turn: agent.files_written_this_turn.clone(),
            };
            let _ = dsx_proto::write_frame(tui_writer, &tool_state_frame);
        }

        if agent.pending_ask_user.is_some() {
            break;
        }

        if agent.tool_failures >= 3 {
            log::warn!("safety gate: 3 cumulative tool failures");
            agent.turn_scores.push(turn_scorer::score_current_turn(agent));
            agent.turn_annotations.push("[System] 3 consecutive tool failures. Respond with analysis — do not call more tools.".to_string());
            agent.tool_failures = 0;
        }
    }

    // ── Check for pending ask_user before entering post-tool-loop ──
    if agent.pending_ask_user.is_some() {
        return;
    }

    // ── Post-tool-loop: one more API call to let the model wrap up ──
    // Even with tools:None, DeepSeek V4 may output DSML tool calls inline.
    // We parse DSML/XML here so raw markup doesn't appear in the chat,
    // and if valid tool calls are found, execute them in a mini-loop.

    if agent.max_tool_rounds > 0 {
        agent.turn_annotations.push(format!("[System] Max tool rounds ({}) reached. Respond with what you have.", agent.max_tool_rounds));
    }

    let max_post_rounds = 3u32;
    let mut sent_final_response = false;
    for _post_round in 0..max_post_rounds {
        let (system, messages, _breakdown) =
            crate::assembly::build_context(agent);

        let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::CachePrediction { hit_rate: agent.predicted_cache_hit_pct });

        let messages_json = serde_json::to_value(&messages).unwrap_or_default();

        let chat = AgentToHp::ApiChat {
            model: agent.config.model.clone(),
            system: Some(system),
            messages: messages_json,
            effort: agent.config.effort.clone(),
            max_tokens: Some(agent.config.max_tokens),
            tools: None,
            user_id: Some(agent.session_seed.clone()),
        };
        let _ = dsx_proto::write_frame(hp.get_mut(), &chat);

        let HpStreamResponse { mut content, reasoning_content, thinking_signature, usage, tool_calls_raw } =
            match read_hp_stream_response(hp, agent, tui_writer, _post_round) {
                Ok(r) => r,
                Err(()) => return,
            };

        agent.health.record_api_success(&agent.config.model);

        // ── Parse DSML/XML tool calls from content ──
        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);
        if content.contains("\u{ff5c}DSML\u{ff5c}tool_calls") {
            let (cleaned, dsml_tcs) = tool_parser::parse_dsml_tool_calls(&content, &agent.tool_defs);
            content = cleaned;
            parsed = dsml_tcs;
        }
        if parsed.is_empty() && (content.contains("<tool_use>") || content.contains("<read>") || content.contains("<exec>") || content.contains("<write>") || content.contains("<search>")) {
            let tool_names: Vec<String> = agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let (cleaned, xml_tcs) = tool_parser::parse_xml_tool_calls(&content, &tool_names);
            content = cleaned;
            parsed = xml_tcs;
        }

        if parsed.is_empty() {
            // No tool calls → final answer
            let assistant_msg = build_and_push_assistant(agent, &content, &reasoning_content, &thinking_signature, &parsed);

            let final_reasoning = (!agent.stream_reasoning.is_empty())
                .then(|| agent.stream_reasoning.clone())
                .or_else(|| reasoning_content.clone());
            agent.turn_scores.push(turn_scorer::score_current_turn(agent));
            agent.stream_content.clear();
            agent.stream_reasoning.clear();

            learning::auto_extract_memory(agent, &assistant_msg);

            agent.health.record_turn(false);

            let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::ApiResponse {
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
        build_and_push_assistant(agent, &content, &reasoning_content, &thinking_signature, &parsed);

        for tc in &parsed {
            match execute_single_tool(agent, tc, tui_writer) {
                ToolOutcome::Break => break,
                _ => {}
            }
        }
    }

    // Guard: if post-tool-loop exhausted without sending ApiResponse (all 3 rounds
    // returned XML tool calls), send a fallback to prevent TUI ghost hang.
    if !sent_final_response {
        let fallback = AgentToTui::ApiResponse {
            content: format!("[System] Max post-tool rounds ({}) reached.", max_post_rounds),
            tool_calls: None,
            stop_reason: Some("max_rounds".into()),
            usage: None,
            reasoning_content: None,
        };
        let _ = dsx_proto::write_frame(tui_writer, &fallback);
    }

    // End of post-tool-loop — finalize turn
    agent.health_status_line = health_status(agent);
    agent.health.reset_turn();

    session::save_live_snapshot(
        &agent.session_seed, &agent.ctx.to_vec(),
        &agent.config.model, agent.config.effort.as_deref(), None);
    session_persistence::maybe_save_session(agent);
}
// ── Main ──

pub fn run() {
    eprintln!("dsx-agent starting");

    // ── 1. Initialize logging ──
    dsc_log::init();

    // ── 2. Load configuration ──
    let config = Config::load().unwrap_or_default();
    eprintln!("dsx-agent: model={} effort={:?} context_limit={}",
        config.model, config.effort, config.context_limit);

    // ── 3. Parse CLI args ──
    let args: Vec<String> = std::env::args().collect();
    let resume_seed = args.windows(2).find(|w| w[0] == "--session").and_then(|w| Some(w[1].clone()));
    if let Some(ref seed) = resume_seed {
        eprintln!("dsx-agent: resume request for session {seed}");
    }

    // ── 4. Initialize AgentState ──
    let mut agent = AgentState::new(config);
    agent.resume_seed = resume_seed;
    agent.health.context_limit = agent.config.context_limit;

    // ── 4. Connect to HP (single stream, no try_clone) ──
    let mut hp_conn: Option<BufReader<TcpStream>> = crate::hp::connect().map(BufReader::new);

    // Drain register response
    if let Some(ref mut hp) = hp_conn {
        let _: Option<HpToAgent> = dsx_proto::read_frame(hp).ok().flatten();
    }

    // ── 5. Spawn dsx-tools ──
    let exe = std::env::current_exe().unwrap();
    let (tools_child, mut tools_reader, mut tools_writer) = crate::tools_spawn::spawn_process(&exe);
    let mut tools_option: Option<std::process::Child> = Some(tools_child);

    // Send init frame and read Ready response
    let init = AgentToTools::Init {
        allowed_tools: vec![],
        session_seed: "pipe".into(),
        auto_mode: agent.auto_mode,
    };
    let _ = dsx_proto::write_frame(&mut tools_writer, &init);
    let ready: Option<ToolsToAgent> = dsx_proto::read_frame(&mut tools_reader).ok().flatten();
    if let Some(ToolsToAgent::Ready { tools }) = &ready {
        let essential: &[&str] = &["exec", "read_file", "write_file", "edit_file", "edit_file_diff", "explore", "search", "list_dir", "glance", "ask_user", "status", "task_create", "task_update", "task_list", "plan_create", "plan_update", "plan_read", "plan_list", "web_fetch", "web_search", "git", "mem_save", "mem_read", "mem_forget", "recall", "pitfall_save", "pitfall_guide"];
        agent.tool_defs = tools.iter().filter(|t| essential.contains(&t.function.name.as_str())).cloned().collect();
        eprintln!("dsx-agent: tools → {} (filtered from {})", agent.tool_defs.len(), tools.len());
    }

    // Hand pipes over to tools.rs for the compatibility execute_tool() layer
    tools::init_tools_ipc(tools_reader, tools_writer, agent.tool_defs.clone());

    // ── 6. Session check ──
    let lives = session::find_live_sessions();
    if !lives.is_empty() {
        eprintln!("dsx-agent: {} live session(s) available for resume", lives.len());
    }

    // ── 7. Main loop ──
    let stdin = std::io::stdin();
    let mut tui_reader = BufReader::new(stdin.lock());
    let mut tui_writer = std::io::stdout();

    loop {
        let frame: TuiToAgent = match dsx_proto::read_frame(&mut tui_reader) {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                eprintln!("dsx-agent: TUI parse error: {e}");
                continue;
            }
        };

        eprintln!("dsx-agent: tui ← {:?}", std::mem::discriminant(&frame));

        match frame {
            TuiToAgent::UserInput { text } => {
                // Respawn tools if IPC was lost
                if tools::all_tools().is_empty() {
                    eprintln!("dsx-agent: tools IPC dead, respawning...");
                    if crate::tools_spawn::respawn(&mut tools_option) {
                        agent.tool_defs = tools::all_tools();
                        eprintln!("dsx-agent: tools IPC restored ({} tools)", agent.tool_defs.len());
                    } else {
                        eprintln!("dsx-agent: tools respawn FAILED");
                    }
                }
                // Process input — if HP not connected or fails, try reconnect once
                let hp_failed = if let Some(ref mut hp) = hp_conn {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || handle_user_input(&mut agent, &text, hp, &mut tui_writer, &mut tui_reader)
                    ));
                    result.is_err()
                } else {
                    // HP was never connected — try reconnect now
                    true
                };

                if hp_failed {
                    eprintln!("dsx-agent: HP failed, reconnecting...");
                    if let Some(stream) = crate::hp::try_reconnect() {
                        let reader = std::io::BufReader::new(stream);
                        hp_conn = Some(reader);
                        eprintln!("dsx-agent: HP reconnected, retry input");
                        // Retry with new connection
                        if let Some(ref mut hp) = hp_conn {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || handle_user_input(&mut agent, &text, hp, &mut tui_writer, &mut tui_reader)
                            ));
                        }
                    } else {
                        eprintln!("dsx-agent: HP reconnect failed");
                        let _ = dsx_proto::write_frame(&mut tui_writer,
                            &AgentToTui::Error { message: "HP disconnected. Please try again.".into() });
                    }
                }
                let _ = dsx_proto::write_frame(&mut tui_writer, &AgentToTui::Done);
            }

            TuiToAgent::ToolCall { id: _, name, action, args } => {
                // Execute tool via IPC and forward result to TUI
                let args_str = args.to_string();
                let content = crate::tools::execute_tool(&name, &action, &args_str);
                let tui_resp = AgentToTui::ApiResponse {
                    content, reasoning_content: None, tool_calls: None, stop_reason: None, usage: None,
                };
                let _ = dsx_proto::write_frame(&mut tui_writer, &tui_resp);
                let _ = dsx_proto::write_frame(&mut tui_writer, &AgentToTui::Done);
            }

            TuiToAgent::SetPhase { phase } => {
                let task_phase = match phase.as_str() {
                    "plan" => dsx_types::TaskPhase::Plan,
                    "coding" | "code" => dsx_types::TaskPhase::Coding,
                    "debug" => dsx_types::TaskPhase::Debug,
                    _ => dsx_types::TaskPhase::Coding,
                };
                agent.current_task_phase = task_phase;
                router::set_phase(task_phase, router::read_debug_level());

                let _ = dsx_proto::write_frame(&mut tui_writer, &AgentToTui::PhaseChanged { phase });
            }

            TuiToAgent::ToolConfirm { .. } => {} // Confirm flow removed — all tools auto-pass

            TuiToAgent::Cancel => {
                crate::tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                agent.stream_cancelled = true;
                // Also send cancel to tools subprocess
                crate::tools::cancel_current_tool();
            }

            TuiToAgent::Shutdown => {
                session_persistence::maybe_save_session(&mut agent);
                let _ = dsx_proto::write_frame(&mut tui_writer, &AgentToTui::ShutdownAck);
                break;
            }

            _ => {} // Future variants — silently ignored
        }
    }

    // ── Cleanup ──
    crate::tools::shutdown_tools();
    if let Some(mut c) = tools_option.take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    agent.maybe_save_session();

    if let Some(ref mut hp) = hp_conn {
        let unreg = AgentToHp::Unregister { pid: std::process::id() };
        let _ = dsx_proto::write_frame(hp.get_mut(), &unreg);
    }

    eprintln!("dsx-agent: shutdown complete (session {}, {} turns, {} tokens)",
        agent.session_seed, agent.ctx.turn_count(), agent.session_tokens);
}
