//! HP (Health Platform) TCP bridge: stream reading, frame dispatch, result emission.

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, Agent2Ui, HpToAgent};
use dsx_types::UsageInfo;

use crate::agent::AgentState;

/// Final response from a single HP API call (ApiResponse frame).
pub struct HpStreamResponse {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub thinking_signature: Option<String>,
    pub usage: Option<UsageInfo>,
    pub tool_calls_raw: serde_json::Value,
}

/// Send a ToolResult frame via the agent-to-TUI channel (`agent_tx`).
pub fn emit_tool_result(
    tx: &mpsc::Sender<Agent2Ui>,
    id: &str,
    name: &str,
    content: &str,
    success: bool,
) {
    let _ = tx.send(Agent2Ui::ToolResult {
        id: id.to_string(),
        name: name.to_string(),
        content: content.to_string(),
        success,
    });
}

/// Read HP streaming response until `ApiResponse` (or `Error`) is received.
/// Sends `ContentDelta` / `ToolProgress` frames via channel as they arrive.
pub fn read_hp_stream_response(
    hp: &mut BufReader<TcpStream>,
    agent: &mut AgentState,
    agent_tx: &mpsc::Sender<Agent2Ui>,
    _round: u32,
) -> Result<HpStreamResponse, ()> {
    loop {
        let hp_resp: HpToAgent = match dsx_proto::read_frame(hp) {
            Ok(Some(r)) => r,
            Ok(None) => {
                log::warn!("dsx-agent: HP connection closed (EOF)");
                return Err(());
            }
            Err(e) => {
                log::warn!("dsx-agent: HP parse error: {e}");
                return Err(());
            }
        };

        match hp_resp {
            HpToAgent::ContentDelta { delta, reasoning } => {
                if agent.stream_cancelled
                    || crate::tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst)
                {
                    log::info!("dsx-agent: streaming cancelled");
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
            HpToAgent::ToolProgress {
                id,
                content: prog_content,
                stream_type,
            } => {
                let _ = agent_tx.send(Agent2Ui::ToolProgress {
                    id: id.clone(),
                    content: prog_content.clone(),
                    stream_type: stream_type.clone(),
                });
            }
            HpToAgent::ApiResponse {
                content: c,
                tool_calls,
                stop_reason: _,
                reasoning_content: rc,
                thinking_signature: ts,
                usage: u,
            } => {
                return Ok(HpStreamResponse {
                    content: c,
                    tool_calls_raw: tool_calls.unwrap_or(serde_json::Value::Null),
                    reasoning_content: rc,
                    thinking_signature: ts,
                    usage: u,
                });
            }
            HpToAgent::Error { message } => {
                let _ = agent_tx.send(Agent2Ui::Error {
                    message: message.clone(),
                });
                return Err(());
            }
            _ => { /* ignore non-stream frames */ }
        }
    }
}
