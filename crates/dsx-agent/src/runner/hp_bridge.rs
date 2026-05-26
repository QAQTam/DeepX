//! HP (Health Platform) TCP bridge: stream reading, frame dispatch, result emission.

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, AgentToTui, HpToAgent};
use dsx_types::UsageInfo;

use crate::agent::AgentState;

/// Accumulated response from an HP streaming session.
pub struct HpStreamResponse {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub thinking_signature: Option<String>,
    pub usage: Option<UsageInfo>,
    pub tool_calls_raw: serde_json::Value,
}

/// Send a ToolResult frame via the TUI channel.
pub fn emit_tool_result(
    tx: &mpsc::Sender<AgentToTui>,
    id: &str,
    name: &str,
    content: &str,
    success: bool,
) {
    let _ = tx.send(AgentToTui::ToolResult {
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
    agent_tx: &mpsc::Sender<AgentToTui>,
    round: u32,
) -> Result<HpStreamResponse, ()> {
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
                if agent.stream_cancelled
                    || crate::tools::CANCEL.load(std::sync::atomic::Ordering::SeqCst)
                {
                    eprintln!("dsx-agent: streaming cancelled");
                    return Err(());
                }
                if round == 0 {
                    eprintln!(
                        "dsx DEBUG: hp.ContentDelta d={} r={}",
                        delta.len(),
                        reasoning.as_ref().map(|s| s.len()).unwrap_or(0)
                    );
                }

                let _ = agent_tx.send(AgentToTui::ContentDelta {
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
                let _ = agent_tx.send(AgentToTui::ToolProgress {
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
                let _ = agent_tx.send(AgentToTui::Error {
                    message: message.clone(),
                });
                agent.health.record_api_error();
                return Err(());
            }
            _ => { /* ignore non-stream frames */ }
        }
    }
}
