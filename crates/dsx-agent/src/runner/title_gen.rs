//! Session title generation via V4 `<|title|>` Quick Instruction token.

use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::io::BufReader;

use dsx_proto::AgentToHp;
use dsx_types::Message;

use crate::agent::AgentState;

/// Generate a session title from the first user+assistant exchange.
/// Appends `<|title|>` token to the first assistant response and sends
/// a lightweight API call. Sets `agent.session_title` on success.
pub fn generate_title(agent: &mut AgentState, hp: &mut BufReader<TcpStream>) {
    if agent.health.turn > 1 {
        return;
    }

    let messages: Vec<Message> = agent.ctx.to_vec();
    if messages.len() < 3 {
        return;
    }

    let (first_user, first_assistant) = match find_first_pair(&messages) {
        Some(p) => p,
        None => return,
    };

    let sys = messages.iter().find(|m| m.role == "system").cloned();
    let mut title_msgs = Vec::new();
    if let Some(s) = sys {
        title_msgs.push(serde_json::to_value(&s).unwrap_or_default());
    }
    title_msgs.push(serde_json::to_value(&first_user).unwrap_or_default());

    let mut asst_with_token = first_assistant.clone();
    let last_block = asst_with_token.content.last_mut();
    if let Some(dsx_types::ContentBlock::Text { ref mut text }) = last_block {
        text.push_str("<|title|>");
    } else {
        asst_with_token.content.push(dsx_types::ContentBlock::text("<|title|>"));
    }
    title_msgs.push(serde_json::to_value(&asst_with_token).unwrap_or_default());

    let request = serde_json::to_string(&AgentToHp::ApiChat {
        model: agent.config.model.clone(),
        system: None,
        messages: serde_json::to_value(&title_msgs).unwrap_or_default(),
        tools: None,
        max_tokens: Some(30),
        effort: None,
        user_id: None,
        api_key: None,
    }).unwrap_or_default();

    let inner = hp.get_mut();
    if writeln!(inner, "{}", request).is_err() { return; }
    if inner.flush().is_err() { return; }

    let mut line = String::new();
    let mut title = String::new();
    for _ in 0..20 {
        line.clear();
        if hp.read_line(&mut line).is_err() { break; }
        let t = line.trim();
        if t.is_empty() { continue; }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            if let Some(choices) = v["choices"].as_array() {
                if let Some(delta) = choices[0].get("delta").or_else(|| choices[0].get("message")) {
                    if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
                        title.push_str(c);
                    }
                }
            }
            if v["choices"][0].get("finish_reason").and_then(|f| f.as_str()) == Some("stop") {
                break;
            }
        }
    }

    let title = title.trim().to_string();
    if !title.is_empty() {
        agent.session_title = Some(title);
    }
}

fn find_first_pair(messages: &[Message]) -> Option<(Message, Message)> {
    let user = messages.iter().find(|m| m.role == "user")?;
    let asst = messages.iter().skip_while(|m| m.role != "user")
        .skip(1)
        .find(|m| m.role == "assistant")?;
    Some((user.clone(), asst.clone()))
}
