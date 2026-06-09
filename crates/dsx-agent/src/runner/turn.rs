//! Turn processing (v5): user input → tool-calling loop → round-based UI events → session save.

use std::sync::mpsc;

use dsx_proto::{Agent2Ui, RoundBlock, ToolCallDef, ToolResultDef, TurnData, RoundData};
use dsx_types::{ContentBlock, ToolCall};

use crate::agent::AgentState;
use crate::orchestrator::learning;
use crate::tool_parser;

use super::api_turn::run_api_turn;
use super::ui_emit::{build_and_push_assistant, make_tool_def};
use super::{build_documents, build_recent_edits, build_tasks, emit};

/// Per-turn outcome: usage + tool stats for Dashboard.
pub struct TurnOutcome {
    pub usage: Option<dsx_types::UsageInfo>,
    pub tool_calls: u32,
    pub tool_failures: u32,
}

fn truncate_exec_for_model(output: &str) -> String {
    if output.len() <= 8192 { return output.to_string(); }
    let lines: Vec<&str> = output.lines().collect();
    let head = 30usize;
    let tail = 40usize;
    if lines.len() <= head + tail + 10 { return output.to_string(); }
    let head_part = lines[..head].join("\n");
    let tail_part = lines[lines.len() - tail..].join("\n");
    let skipped = lines.len() - head - tail;
    format!("{head_part}\n--- {skipped} lines omitted (exec output truncated for context) ---\n{tail_part}")
}

/// Push `text` to the MessageStore (auto-repairs cancellation deadlocks).
/// Reconstruct TurnData from the existing context (for session resume).
#[allow(dead_code)]
pub(super) fn build_turns_from_context(agent: &AgentState) -> Vec<TurnData> {
    let mut turns = Vec::new();
    for (ti, turn) in agent.msg.turns().iter().enumerate() {
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
    agent_tx: &mpsc::Sender<Agent2Ui>,
) -> TurnOutcome {
        let turn_num = agent.turn_count.to_string();
    let turn_id = format!("t{}", turn_num);
    let mut last_usage: Option<dsx_types::UsageInfo> = None;

    emit(&agent_tx, Agent2Ui::TurnStart {
        turn_id: turn_id.clone(),
        user_text: text.to_string(),
    });

    // Tool stats sourced from ToolManager
    agent.turn.stream_cancelled = false;
    agent.files.files_written_this_turn.clear();
    dsx_tools::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);

    agent.refresh_progress_context();

    let mut ipc_broken = false;
    let mut round_num = 0u32;

    loop {
        if ipc_broken {
            break;
        }
        if agent.turn.stream_cancelled
            || dsx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst)
        {
            agent.turn.stream_cancelled = false;
            dsx_tools::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
            agent.system_note("system", "用户终止了当前操作。".to_string());
            break;
        }

        let (content, reasoning_content, tool_calls_raw, usage, stop_reason) =
            match run_api_turn(agent, agent_tx, &turn_id, round_num, true) {
                Ok(v) => v,
                Err(()) => return TurnOutcome { usage: None, tool_calls: 0, tool_failures: 0 },
            };

        let stripped = tool_parser::strip_fenced_code(&content);
        let mut parsed: Vec<ToolCall> = tool_parser::parse_tool_calls(&tool_calls_raw);
        let mut content = content;
        let mut dsml_detected = false;
        let mut _dsml_source: Vec<bool> = Vec::new();

        if parsed.is_empty() && tool_parser::has_dsml(&stripped) {
            dsml_detected = true;
            let (cleaned, dsml_tcs) =
                tool_parser::parse_dsml_tool_calls(&stripped, &agent.tool_defs);
            if !dsml_tcs.is_empty() {
                content = cleaned;
                parsed = dsml_tcs;
                _dsml_source = vec![true; parsed.len()];
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
                _dsml_source = vec![false; parsed.len()];
            }
        }

        let has_tools = !parsed.is_empty();

        if let Some(ref u) = usage {
            agent.session.tokens += u.total_tokens as u64;
            last_usage = Some(u.clone());
        }

        let assistant_msg = build_and_push_assistant(agent, &content, &reasoning_content, &parsed);

        // Build ordered blocks from ContentBlock sequence (preserves LLM output order)
        let mut round_blocks: Vec<RoundBlock> = Vec::new();
        for cb in &assistant_msg.content {
            match cb {
                ContentBlock::Reasoning { reasoning } if !reasoning.is_empty() => {
                    round_blocks.push(RoundBlock::Reasoning { content: reasoning.clone() });
                }
                ContentBlock::Text { text } if !text.is_empty() => {
                    round_blocks.push(RoundBlock::Text { content: text.clone() });
                }
                ContentBlock::ToolUse { id, name, input } => {
                    if name != "ask_user" {
                        let (display, _) = super::ui_emit::format_tool_display(name, &input.to_string());
                        round_blocks.push(RoundBlock::Tool {
                            card: ToolCallDef {
                                id: id.clone(),
                                name: name.clone(),
                                args_display: display,
                                args_json: input.to_string(),
                            }
                        });
                    }
                }
                _ => {}
            }
        }

        // Build tool call defs for RoundComplete
        let tool_call_defs: Vec<ToolCallDef> = parsed.iter()
            .filter(|tc| tc.function.name != "ask_user")
            .map(|tc| make_tool_def(&tc.id, &tc.function.name, &tc.function.arguments))
            .collect();

        // Send RoundComplete
        // Always include the answer text, even when tool calls are present.
        // The LLM may output explanatory text before deciding to call tools.
        // Filtering it out caused the streaming preview answer to vanish on round_complete.
        emit(&agent_tx, Agent2Ui::RoundComplete {
            turn_id: turn_id.clone(),
            round_num,
            thinking: reasoning_content.clone().filter(|r| !r.is_empty()),
            answer: Some(content.clone()).filter(|c| !c.is_empty()),
            tool_calls: tool_call_defs.clone(),
            blocks: round_blocks,
            is_final: !has_tools,
        });

        if !has_tools {
            // Cancel / stop_reason="cancelled": user interrupted — exit cleanly, don't loop.
            if stop_reason.as_deref() == Some("cancelled") {
                log::info!("turn: cancelled at r={}, exiting turn", round_num);
                save_snapshot(agent);
                emit(&agent_tx, Agent2Ui::TurnEnd {
                    turn_id: turn_id.clone(),
                    stop_reason,
                    usage: usage.clone(),
                });
                let stats = crate::tools::global_stats(); return TurnOutcome { usage, tool_calls: stats.calls_total, tool_failures: stats.failures };
            }

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
                usage: usage.clone(),
            });
            let stats = crate::tools::global_stats(); return TurnOutcome { usage, tool_calls: stats.calls_total, tool_failures: stats.failures };
        }

        save_snapshot(agent);

        if !agent.files.has_explored {
            if parsed.iter().any(|tc| {
                tc.function.name == "exec"
                    && dsx_types::arg::tool_action(&tc.function.arguments) == "explore"
            }) {
                agent.files.has_explored = true;
            }
        }

        // Execute tools via ToolManager (parallel, with cancel + exec streaming)
        let tools: Vec<dsx_message::ToolExecRequest> = parsed.iter()
            .filter(|tc| tc.function.name != "ask_user")
            .map(|tc| {
                let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                dsx_message::ToolExecRequest {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    args,
                }
            })
            .collect();

        let reports = crate::tools::execute_tools_parallel(
            tools,
            None,
            Some(agent_tx),
        );

        for (tc_id, report) in &reports {
            let tr_content = &report.content;

            if tr_content.contains("tools IPC") || tr_content.contains("not initialised") {
                ipc_broken = true;
            }

            // Truncate exec output for model context, push to message
            {
                let ctx_content = if parsed.iter().any(|tc| tc.id == *tc_id && tc.function.name == "exec") {
                    truncate_exec_for_model(tr_content)
                } else {
                    tr_content.clone()
                };
                agent.msg.push_tool_result(tc_id, &ctx_content);
            }
        }

        // Emit real-time debug snapshot
        let stats = crate::tools::global_stats();
        emit(&agent_tx, Agent2Ui::Dashboard {
            hp_connected: true,
            session_seed: agent.session.seed.clone(),
            context_limit: agent.config.context_limit,
            tool_calls_total: stats.calls_total,
            tool_failures: stats.failures,
            current_phase: "tool_batch".to_string(),
            streaming: false,
            dsml_compat_count: agent.dsml_compat_count,
            documents: build_documents(agent),
            recent_edits: build_recent_edits(agent),
            tasks: build_tasks(agent),
            session_title: agent.session.title.clone(),
            usage: last_usage.clone(),
            
        });

        {
            let stats = crate::tools::global_stats();
            if stats.failures >= 3 {
                log::warn!("safety gate: {} cumulative tool failures", stats.failures);
                agent.turn.annotations.push("[System] Multiple tool failures. Respond with analysis — do not call more tools.".into());
            }
        }

        round_num += 1;
    }

        if ipc_broken {
        emit(&agent_tx, Agent2Ui::TurnEnd {
            turn_id: turn_id.clone(),
            stop_reason: Some("error".to_string()),
            usage: None,
        });
        return TurnOutcome { usage: None, tool_calls: 0, tool_failures: 0 };
    }

    save_snapshot(agent);

    emit(&agent_tx, Agent2Ui::TurnEnd {
        turn_id: turn_id.clone(),
        stop_reason: Some("interrupted".to_string()),
        usage: last_usage.clone(),
    });
    let stats = crate::tools::global_stats(); TurnOutcome { usage: last_usage, tool_calls: stats.calls_total, tool_failures: stats.failures }
}

/// Save the current session snapshot to disk (crash recovery).
fn save_snapshot(agent: &AgentState) {
    agent.msg.snapshot(&agent.config.model, &agent.config.reasoning_effort);
}
