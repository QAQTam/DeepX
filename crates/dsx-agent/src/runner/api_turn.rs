//! API turn execution: SSE streaming from gate → typed UI events (v5).

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, AgentToHp, Agent2Ui, RoundDeltaKind};

use crate::agent::AgentState;

use super::gate_bridge::read_hp_frame;

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

/// Run one API turn: build context → send ApiChat → read HP stream → emit
/// RoundDelta events during streaming, return complete response data.
pub(super) fn run_api_turn(
    agent: &mut AgentState,
    hp: &mut BufReader<TcpStream>,
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
    let messages = crate::assembly::build_context(agent);

    let messages_json = serde_json::to_value(&messages).unwrap_or_default();

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
            message: "Failed to communicate with gate daemon.".into(),
        });
        return Err(());
    }

    let mut stream = StreamState::new();
    let mut stream_content = String::new();
    let mut stream_reasoning = String::new();

    loop {
        let frame = match read_hp_frame(hp) {
            Ok(Some(f)) => f,
            Ok(None) | Err(..) => {
                let _ = agent_tx.send(Agent2Ui::Error {
                    message: "Gate connection closed unexpectedly.".into(),
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
                    // Don't send RoundDelta for cancelled stream — caller handles TurnEnd
                    break;
                }

                if let Some(r) = &reasoning {
                    if !r.is_empty() {
                        if !stream.has_reasoning_start {
                            stream.has_reasoning_start = true;
                        }
                        let _ = agent_tx.send(Agent2Ui::RoundDelta {
                            turn_id: turn_id.into(),
                            round_num,
                            kind: RoundDeltaKind::Thinking,
                            delta: r.clone(),
                        });
                        stream_reasoning.push_str(r);
                    }
                }

                if !delta.is_empty() {
                    if !stream.has_text_start {
                        stream.has_text_start = true;
                    }
                    let _ = agent_tx.send(Agent2Ui::RoundDelta {
                        turn_id: turn_id.into(),
                        round_num,
                        kind: RoundDeltaKind::Answering,
                        delta: delta.clone(),
                    });
                    stream_content.push_str(&delta);
                }
            }
            dsx_proto::HpToAgent::ToolProgress { ref name, .. } => {
                if !name.is_empty() {
                    if !stream.has_tool_call_start {
                        stream.has_tool_call_start = true;
                        stream.dsml_tool_names.clear();
                    }
                    if !stream.dsml_tool_names.contains(name) {
                        stream.dsml_tool_names.push(name.clone());
                        let _ = agent_tx.send(Agent2Ui::RoundDelta {
                            turn_id: turn_id.into(),
                            round_num,
                            kind: RoundDeltaKind::ToolCalling,
                            delta: name.clone(),
                        });
                    }
                }
            }
            dsx_proto::HpToAgent::ApiResponse {
                content, tool_calls, stop_reason, reasoning_content, usage,
            } => {
                let final_content = if !stream_content.is_empty() { stream_content } else { content };
                let final_reasoning = if !stream_reasoning.is_empty() {
                    Some(stream_reasoning)
                } else {
                    reasoning_content
                };
                return Ok((
                    final_content,
                    final_reasoning,
                    tool_calls.unwrap_or(serde_json::Value::Null),
                    usage,
                    stop_reason,
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

    // If we broke out of the loop (cancelled), return empty
    Ok((String::new(), None, serde_json::Value::Null, None, Some("cancelled".into())))
}
