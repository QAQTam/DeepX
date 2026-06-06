//! API turn execution: direct HTTP SSE streaming → typed UI events (v5).

use std::sync::mpsc;

use dsx_proto::{Agent2Ui, RoundDeltaKind};

use crate::agent::AgentState;
use crate::gate;

use super::emit;

struct StreamState {
    has_text_start: bool,
    has_reasoning_start: bool,
    has_tool_call_start: bool,
    dsml_tool_names: Vec<String>,
}

impl StreamState {
    fn new() -> Self {
        Self { has_text_start: false, has_reasoning_start: false, has_tool_call_start: false, dsml_tool_names: Vec::new() }
    }
}

/// Run one API turn: build context → call gate::chat_stream → emit
/// RoundDelta events during streaming, return complete response data.
pub(super) fn run_api_turn(
    agent: &mut AgentState,
    agent_tx: &mpsc::Sender<Agent2Ui>,
    turn_id: &str,
    round_num: u32,
    allow_tools: bool,
) -> Result<
    (
        String,
        Option<String>,
        serde_json::Value,
        Option<dsx_types::UsageInfo>,
        Option<String>,
    ),
    (),
> {
    let messages = agent.build_context();

    let provider = match agent.config.protocol.as_str() {
        "anthropic" => gate::ProviderConfig::anthropic(
            &agent.config.base_url, &agent.config.api_key, &agent.config.model),
        _ => gate::ProviderConfig::openai(
            &agent.config.base_url, &agent.config.api_key, &agent.config.model),
    };
    let tools = if allow_tools {
        Some(agent.tool_defs.clone())
    } else {
        None
    };

    let mut stream = StreamState::new();
    let mut stream_content = String::new();
    let mut stream_reasoning = String::new();
    let mut stream_tool_calls: serde_json::Value = serde_json::Value::Null;
    let mut stream_usage: Option<dsx_types::UsageInfo> = None;
    let mut stream_stop_reason: Option<String> = None;
    let mut had_error = false;

    let result = gate::chat_stream(
        &provider,
        None, // system prompt handled by ContextAssembler
        messages,
        tools,
        agent.config.max_tokens,
        Some(agent.config.reasoning_effort.clone()),
        Some(agent.session.seed.clone()),
        &mut |event| {
            match event {
                gate::StreamEvent::ContentDelta(delta) => {
                    if agent.turn.stream_cancelled
                        || dsx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst)
                    {
                        agent.turn.stream_cancelled = false;
                        return;
                    }

                    if !delta.is_empty() {
                        if !stream.has_text_start {
                            stream.has_text_start = true;
                        }
                        emit(&agent_tx, Agent2Ui::RoundDelta {
                            turn_id: turn_id.into(),
                            round_num,
                            kind: RoundDeltaKind::Answering,
                            delta: delta.clone(),
                        });
                        stream_content.push_str(&delta);
                    }
                }
                gate::StreamEvent::ReasoningDelta(reasoning) => {
                    if agent.turn.stream_cancelled
                        || dsx_tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst)
                    {
                        agent.turn.stream_cancelled = false;
                        return;
                    }

                    if !reasoning.is_empty() {
                        if !stream.has_reasoning_start {
                            stream.has_reasoning_start = true;
                        }
                        emit(&agent_tx, Agent2Ui::RoundDelta {
                            turn_id: turn_id.into(),
                            round_num,
                            kind: RoundDeltaKind::Thinking,
                            delta: reasoning.clone(),
                        });
                        stream_reasoning.push_str(&reasoning);
                    }
                }
                gate::StreamEvent::ToolCallProgress { ref name, .. } => {
                    if !name.is_empty() {
                        if !stream.has_tool_call_start {
                            stream.has_tool_call_start = true;
                            stream.dsml_tool_names.clear();
                        }
                        if !stream.dsml_tool_names.contains(name) {
                            stream.dsml_tool_names.push(name.clone());
                            emit(&agent_tx, Agent2Ui::RoundDelta {
                                turn_id: turn_id.into(),
                                round_num,
                                kind: RoundDeltaKind::ToolCalling,
                                delta: name.clone(),
                            });
                        }
                    }
                }
                gate::StreamEvent::Done { raw_message, usage, stop_reason } => {
                    let mut final_content = String::new();
                    let mut final_reasoning = String::new();
                    let mut tool_calls: Vec<serde_json::Value> = Vec::new();

                    for block in &raw_message.content {
                        match block {
                            dsx_types::ContentBlock::Text { text } => {
                                final_content.push_str(text);
                            }
                            dsx_types::ContentBlock::Reasoning { reasoning: r } => {
                                final_reasoning.push_str(r);
                            }
                            dsx_types::ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(serde_json::json!({
                                    "id": id,
                                    "name": name,
                                    "arguments": serde_json::to_string(input).unwrap_or_default(),
                                }));
                            }
                            _ => {}
                        }
                    }

                    // Prefer streaming content if we have it, otherwise use final message
                    if !stream_content.is_empty() {
                        // keep stream_content
                    } else {
                        stream_content = final_content;
                    }
                    if stream_reasoning.is_empty() && !final_reasoning.is_empty() {
                        stream_reasoning = final_reasoning;
                    }
                    stream_tool_calls = if tool_calls.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::Array(tool_calls)
                    };
                    stream_usage = usage;
                    stream_stop_reason = stop_reason;
                }
                gate::StreamEvent::Balance { is_available, total_balance, currency } => {
                    emit(&agent_tx, Agent2Ui::Balance { is_available, total_balance, currency });
                }
                gate::StreamEvent::Error(message) => {
                    emit(&agent_tx, Agent2Ui::Error { message: message.clone() });
                    had_error = true;
                }
            }
        },
    );

    if had_error {
        return Err(());
    }

    if let Err(e) = result {
        log::error!("dsx-agent: chat_stream failed: {e}");
        emit(&agent_tx, Agent2Ui::Error {
            message: format!("API request failed: {e}"),
        });
        return Err(());
    }

    // Check if stream was cancelled
    if stream_stop_reason.as_deref() == Some("cancelled")
        || agent.turn.stream_cancelled
    {
        agent.turn.stream_cancelled = false;
        return Ok((String::new(), None, serde_json::Value::Null, None, Some("cancelled".into())));
    }

    let final_reasoning = if stream_reasoning.is_empty() {
        None
    } else {
        Some(stream_reasoning)
    };

    Ok((
        stream_content,
        final_reasoning,
        stream_tool_calls,
        stream_usage,
        stream_stop_reason,
    ))
}
