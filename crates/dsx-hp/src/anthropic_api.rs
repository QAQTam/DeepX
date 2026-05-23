//! Native Anthropic Messages API streaming client.
//!
//! Messages are already in Anthropic-native format (role + ContentBlock array),
//! so no format conversion is needed — only serialization.

use std::collections::HashMap;
use std::time::Duration;

use dsx_types::{ContentBlock, Message, ToolDef, UsageInfo};
use futures_util::StreamExt;
use reqwest::Client as HttpClient;
use tokio::sync::mpsc;

/// Minimal config needed by the Anthropic API client.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub base_url: String,
    pub api_key: String,
}

/// Events emitted during API streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolCallProgress {
        name: String,
        args_so_far: String,
    },
    Done {
        raw_message: Message,
        usage: Option<UsageInfo>,
        stop_reason: Option<String>,
    },
    Error(String),
}

/// Stream a chat completion via the native Anthropic Messages API.
pub async fn chat_stream(
    config: &GatewayConfig,
    model: &str,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDef>>,
    max_tokens: u32,
    effort: Option<String>,
    user_id: Option<String>,
    tx: mpsc::Sender<StreamEvent>,
) -> anyhow::Result<()> {
    // ── 1. System blocks (cache_control on first) ──
    let mut system_blocks: Vec<serde_json::Value> = Vec::new();
    if let Some(ref base) = system {
        if !base.is_empty() {
            let mut block = serde_json::json!({"type": "text", "text": base});
            block["cache_control"] = serde_json::json!({"type": "ephemeral"});
            system_blocks.push(block);
        }
    }

    // ── 2. Normalize messages (tool role → user+ToolResult) ──
    let mut api_msgs = normalize_messages(messages);

    // ── 3. Cache_control on penultimate message ──
    if api_msgs.len() >= 2 {
        let idx = api_msgs.len() - 2;
        if let Some(content) = api_msgs[idx].get_mut("content") {
            if let Some(arr) = content.as_array_mut() {
                if let Some(last) = arr.last_mut() {
                    if let Some(obj) = last.as_object_mut() {
                        if obj.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                            obj.insert("cache_control".into(), serde_json::json!({"type": "ephemeral"}));
                        }
                    }
                }
            }
        }
    }

    // ── 4. Tool definitions ──
    let anthropic_tools: Option<Vec<serde_json::Value>> = tools.map(|tds| {
        let count = tds.len();
        tds.into_iter()
            .enumerate()
            .map(|(i, td)| {
                let mut t = serde_json::json!({
                    "name": td.function.name,
                    "description": td.function.description,
                    "input_schema": td.function.parameters,
                });
                if i == count - 1 {
                    t["cache_control"] = serde_json::json!({"type": "ephemeral"});
                }
                t
            })
            .collect()
    });

    // ── 5. Build request ──
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": api_msgs,
        "thinking": {"type": "enabled"},
    });
    if !system_blocks.is_empty() {
        body["system"] = serde_json::Value::Array(system_blocks);
    }
    if let Some(t) = anthropic_tools {
        body["tools"] = serde_json::Value::Array(t);
    }
    if let Some(uid) = user_id {
        body["metadata"] = serde_json::json!({"user_id": uid});
    }
    if let Some(e) = effort {
        body["output_config"] = serde_json::json!({"effort": e});
    }

    // ── 6. HTTP POST ──
    let url = build_anthropic_url(&config.base_url);
    let client = HttpClient::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(0)
        .build()?;

    let resp = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let msg = format!("Anthropic API {}: {}", status, text);
        let _ = tx.send(StreamEvent::Error(msg.clone())).await;
        return Err(anyhow::anyhow!("{}", msg));
    }

    // ── 7. SSE parsing ──
    let mut byte_stream = resp.bytes_stream();
    let mut sse_buf = String::new();
    let mut text_buf = String::new();
    let mut think_buf = String::new();
    let mut think_sig: Option<String> = None;
    let mut tool_acc: HashMap<usize, (String, String, String)> = HashMap::new();
    let mut usage_info: Option<UsageInfo> = None;
    let mut stop_reason: Option<String> = None;

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        sse_buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = sse_buf.find("\n\n") {
            let raw = sse_buf[..pos].to_string();
            sse_buf = sse_buf[pos + 2..].to_string();

            let mut data_str = String::new();
            for line in raw.lines() {
                if let Some(dt) = line.trim().strip_prefix("data: ") {
                    data_str = dt.to_string();
                }
            }
            if data_str.is_empty() {
                continue;
            }

            let ev: serde_json::Value = match serde_json::from_str(&data_str) {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("Anthropic SSE: deserialize fail: {} — data: {}", e, data_str);
                    continue;
                }
            };

            let ev_type = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match ev_type {
                "content_block_start" => {
                    let index = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if let Some(cb) = ev.get("content_block") {
                        if cb.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            let id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = cb.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            tool_acc.entry(index).or_insert((id, name, String::new()));
                        }
                    }
                }
                "content_block_delta" => {
                    let index = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if let Some(delta) = ev.get("delta") {
                        let dt = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match dt {
                            "text_delta" => {
                                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                    text_buf.push_str(text);
                                    let _ = tx.send(StreamEvent::ContentDelta(text.to_string())).await;
                                }
                            }
                            "thinking_delta" => {
                                if let Some(t) = delta.get("thinking").and_then(|v| v.as_str()) {
                                    think_buf.push_str(t);
                                    let _ = tx.send(StreamEvent::ReasoningDelta(t.to_string())).await;
                                }
                            }
                            "signature_delta" => {
                                if let Some(s) = delta.get("signature").and_then(|v| v.as_str()) {
                                    think_sig = Some(s.to_string());
                                }
                            }
                            "input_json_delta" => {
                                if let Some(pj) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                    if let Some(entry) = tool_acc.get_mut(&index) {
                                        entry.2.push_str(pj);
                                        let _ = tx.send(StreamEvent::ToolCallProgress {
                                            name: entry.1.clone(),
                                            args_so_far: entry.2.clone(),
                                        }).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "message_delta" => {
                    if let Some(delta) = ev.get("delta") {
                        stop_reason = delta.get("stop_reason").and_then(|v| v.as_str()).map(String::from);
                    }
                    if let Some(u) = ev.get("usage") {
                        let it = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let ot = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        usage_info = Some(UsageInfo {
                            prompt_tokens: it,
                            completion_tokens: ot,
                            total_tokens: it + ot,
                            prompt_cache_hit_tokens: u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                            prompt_cache_miss_tokens: u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                            completion_tokens_details: None,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // ── 8. Build final Message ──
    let mut blocks: Vec<ContentBlock> = Vec::new();

    if !think_buf.is_empty() {
        blocks.push(ContentBlock::Thinking {
            thinking: think_buf,
            signature: think_sig.unwrap_or_default(),
        });
    }
    if !text_buf.is_empty() {
        blocks.push(ContentBlock::text(&text_buf));
    }

    // Sort tool calls by index and add as ToolUse blocks
    let mut sorted: Vec<(usize, String, String, String)> = tool_acc
        .into_iter()
        .map(|(idx, (id, name, args))| (idx, id, name, args))
        .collect();
    sorted.sort_by_key(|(idx, _, _, _)| *idx);
    for (_idx, id, name, args_json) in sorted {
        let input: serde_json::Value = serde_json::from_str(&args_json).unwrap_or(serde_json::Value::Null);
        blocks.push(ContentBlock::ToolUse { id, name, input });
    }

    let raw_message = Message {
        role: "assistant".into(),
        content: blocks,
    };

    let _ = tx.send(StreamEvent::Done { raw_message, usage: usage_info, stop_reason }).await;
    Ok(())
}

/// Normalize: convert internal role:"tool" to user-role messages with ToolResult blocks.
fn normalize_messages(messages: Vec<Message>) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut pending: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "user" | "assistant" => {
                flush_pending(&mut out, &mut pending);
                out.push(serde_json::json!({
                    "role": msg.role,
                    "content": msg.content,
                }));
            }
            "tool" => {
                for block in msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content, is_error } = block {
                        let mut tr = serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content,
                        });
                        if let Some(err) = is_error {
                            tr["is_error"] = serde_json::json!(err);
                        }
                        pending.push(tr);
                    }
                }
            }
            _ => {} // system — skipped
        }
    }

    flush_pending(&mut out, &mut pending);
    out
}

fn flush_pending(out: &mut Vec<serde_json::Value>, pending: &mut Vec<serde_json::Value>) {
    if pending.is_empty() {
        return;
    }
    out.push(serde_json::json!({
        "role": "user",
        "content": std::mem::take(pending),
    }));
}

fn build_anthropic_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1/messages") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{}/messages", base)
    } else {
        format!("{}/v1/messages", base)
    }
}
