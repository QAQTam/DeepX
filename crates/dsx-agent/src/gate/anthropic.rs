//! Native Anthropic Messages API streaming client — sync (ureq).
//!
//! Converts internal ContentBlock messages to Anthropic's content-block format,
//! sends to `/v1/messages`, parses SSE, emits StreamEvent via callback.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use dsx_types::{
    AnthropicCacheControl, AnthropicContent, AnthropicMessage,
    AnthropicSystemBlock, AnthropicThinking, AnthropicTool,
    AnthropicStreamEvent, AnthropicDelta, AnthropicContentBlockStart,
    ContentBlock, Message, ToolDef, UsageInfo,
};

use super::types::{ProviderConfig, StreamEvent};

/// Stream a chat completion via the native Anthropic Messages API.
///
/// `effort` controls thinking budget: low→2048, medium→4096, high→8192, max→16384.
pub fn chat_stream_anthropic(
    provider: &ProviderConfig,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDef>>,
    max_tokens: u32,
    effort: Option<String>,
    on_event: &mut dyn FnMut(StreamEvent),
) -> anyhow::Result<()> {
    // ── 1. Build system blocks (cache_control on last) ──
    let mut system_blocks: Vec<AnthropicSystemBlock> = Vec::new();

    if let Some(ref base) = system {
        if !base.is_empty() {
            system_blocks.push(AnthropicSystemBlock {
                block_type: "text".into(),
                text: base.clone(),
                cache_control: None,
            });
        }
    }

    // Extract system-role messages → additional system blocks
    let mut conv_msgs: Vec<Message> = Vec::new();
    for msg in messages {
        if msg.role == "system" {
            for block in &msg.content {
                if let ContentBlock::Text { text } = block {
                    if !text.is_empty() {
                        system_blocks.push(AnthropicSystemBlock {
                            block_type: "text".into(),
                            text: text.clone(),
                            cache_control: None,
                        });
                    }
                }
            }
        } else {
            conv_msgs.push(msg);
        }
    }

    if let Some(last) = system_blocks.last_mut() {
        last.cache_control = Some(AnthropicCacheControl { cache_type: "ephemeral".into() });
    }

    // ── 2. Convert messages to Anthropic content-block format ──
    let mut anthropic_msgs = convert_to_anthropic(conv_msgs);

    // ── 3. Cache control on penultimate message ──
    if anthropic_msgs.len() >= 2 {
        let idx = anthropic_msgs.len() - 2;
        if let Some(last_block) = anthropic_msgs[idx].content.last_mut() {
            match last_block {
                AnthropicContent::Text { ref mut cache_control, .. }
                | AnthropicContent::ToolUse { ref mut cache_control, .. } => {
                    *cache_control = Some(AnthropicCacheControl { cache_type: "ephemeral".into() });
                }
                _ => {}
            }
        }
    }

    // ── 4. Convert tools (cache_control on last) ──
    let anthropic_tools: Option<Vec<AnthropicTool>> = tools.map(|tds| {
        let count = tds.len();
        tds.into_iter().enumerate().map(|(i, td)| AnthropicTool {
            name: td.function.name,
            description: td.function.description,
            input_schema: td.function.parameters,
            cache_control: if i == count - 1 {
                Some(AnthropicCacheControl { cache_type: "ephemeral".into() })
            } else { None },
        }).collect()
    });

    // ── 5. Build request (DeepSeek Anthropic-compatible endpoint) ──
    // DeepSeek ignores budget_tokens; reasoning intensity goes through output_config.effort
    let effort_val = effort.unwrap_or_else(|| "high".into());
    let request = dsx_types::AnthropicRequest {
        system: if system_blocks.is_empty() { None } else { Some(system_blocks) },
        messages: anthropic_msgs,
        model: provider.model.clone(),
        max_tokens,
        stream: true,
        tools: anthropic_tools,
        tool_choice: None,
        thinking: Some(AnthropicThinking {
            thinking_type: "enabled".into(),
            budget_tokens: None, // ignored by DeepSeek
        }),
        output_config: Some(dsx_types::OutputConfig { effort: effort_val }),
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
        metadata: None,
    };

    // ── 7. HTTP POST ──
    let url = build_anthropic_url(&provider.base_url);

    let resp = ureq::post(&url)
        .set("x-api-key", &provider.api_key)
        .set("anthropic-version", "2023-06-01")
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(120))
        .send_json(&request);

    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let msg = format!("Anthropic API {}: {}", code, text);
            on_event(StreamEvent::Error(msg.clone()));
            return Err(anyhow::anyhow!("{}", msg));
        }
        Err(ureq::Error::Transport(e)) => {
            let msg = format!("Anthropic transport error: {e}");
            on_event(StreamEvent::Error(msg.clone()));
            return Err(anyhow::anyhow!("{}", msg));
        }
    };

    // ── 8. Parse SSE stream ──
    let mut reader = resp.into_reader();
    let mut sse_buf = String::new();
    let mut byte_buf = [0u8; 4096];

    let mut text_buf = String::new();
    let mut think_buf = String::new();
    let mut _think_sig: Option<String> = None;
    let mut tool_acc: HashMap<usize, (String, String, String)> = HashMap::new(); // (id, name, args)
    let mut usage_info: Option<UsageInfo> = None;
    let mut stop_reason: Option<String> = None;

    loop {
        let n = reader.read(&mut byte_buf).map_err(|e| {
            let msg = format!("SSE read error: {e}");
            on_event(StreamEvent::Error(msg.clone()));
            anyhow::anyhow!("{}", msg)
        })?;

        if n == 0 { break; }

        sse_buf.push_str(&String::from_utf8_lossy(&byte_buf[..n]));

        while let Some(pos) = sse_buf.find("\n\n") {
            let raw = sse_buf[..pos].to_string();
            sse_buf = sse_buf[pos + 2..].to_string();

            let mut data_str = String::new();
            for line in raw.lines() {
                let t = line.trim();
                if let Some(dt) = t.strip_prefix("data: ") {
                    data_str = dt.to_string();
                }
            }
            if data_str.is_empty() { continue; }

            let event: AnthropicStreamEvent = match serde_json::from_str(&data_str) {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("Anthropic SSE: deserialize fail: {} — data: {}", e, data_str);
                    continue;
                }
            };

            match event {
                AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                    match content_block {
                        AnthropicContentBlockStart::ToolUse { id, name, .. } => {
                            tool_acc.entry(index).or_insert_with(|| (id, name, String::new()));
                        }
                        _ => {}
                    }
                }
                AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                    match delta {
                        AnthropicDelta::TextDelta { text } => {
                            text_buf.push_str(&text);
                            on_event(StreamEvent::ContentDelta(text));
                        }
                        AnthropicDelta::ThinkingDelta { thinking } => {
                            think_buf.push_str(&thinking);
                            on_event(StreamEvent::ReasoningDelta(thinking));
                        }
                        AnthropicDelta::SignatureDelta { signature } => {
                            _think_sig = Some(signature);
                        }
                        AnthropicDelta::InputJsonDelta { partial_json } => {
                            if let Some(entry) = tool_acc.get_mut(&index) {
                                entry.2.push_str(&partial_json);
                                on_event(StreamEvent::ToolCallProgress {
                                    index,
                                    id: entry.0.clone(),
                                    name: entry.1.clone(),
                                    args_so_far: entry.2.clone(),
                                });
                            }
                        }
                    }
                }
                AnthropicStreamEvent::MessageDelta { delta, usage } => {
                    stop_reason = delta.stop_reason;
                    if let Some(u) = usage {
                        usage_info = Some(UsageInfo {
                            prompt_tokens: u.input_tokens,
                            completion_tokens: u.output_tokens,
                            total_tokens: u.input_tokens + u.output_tokens,
                            prompt_cache_hit_tokens: u.cache_read_input_tokens,
                            prompt_cache_miss_tokens: u.cache_creation_input_tokens,
                            reasoning_tokens: 0,
                        });
                    }
                }
                _ => {} // message_start, content_block_stop, message_stop, ping
            }
        }
    }

    // ── 9. Build final Message ──
    let mut blocks: Vec<ContentBlock> = Vec::new();

    if !think_buf.is_empty() {
        blocks.push(ContentBlock::Reasoning { reasoning: think_buf });
    }
    if !text_buf.is_empty() {
        blocks.push(ContentBlock::text(&text_buf));
    }

    let mut sorted: Vec<(usize, String, String, String)> = tool_acc.into_iter()
        .map(|(idx, (id, name, args))| (idx, id, name, args))
        .collect();
    sorted.sort_by_key(|(idx, _, _, _)| *idx);
    for (_idx, id, name, args_json) in sorted {
        let input: serde_json::Value =
            serde_json::from_str(&args_json).unwrap_or(serde_json::Value::Null);
        blocks.push(ContentBlock::ToolUse { id, name, input });
    }

    let raw_message = Message {
        role: "assistant".into(),
        name: None,
        content: blocks,
    };

    // Preserve thinking signature for next request (Anthropic requires it)
    // Note: our Message format doesn't have a dedicated field for this.
    // The signature is embedded in the Reasoning block's data.

    on_event(StreamEvent::Done { raw_message, usage: usage_info, stop_reason });

    Ok(())
}

// ── Message conversion (ContentBlock → Anthropic) ──

/// Convert internal ContentBlock messages to Anthropic format.
///
/// Rules:
/// - System messages → handled by caller
/// - User → AnthropicContent::Text
/// - Assistant → text + thinking + tool_use blocks
/// - Tool results → grouped into a single user message with ToolResult blocks
fn convert_to_anthropic(msgs: Vec<Message>) -> Vec<AnthropicMessage> {
    let mut out: Vec<AnthropicMessage> = Vec::new();
    let mut tool_results: Vec<AnthropicContent> = Vec::new();

    for msg in msgs {
        match msg.role.as_str() {
            "user" => {
                flush_results(&mut out, &mut tool_results);
                let mut blocks = Vec::new();
                for block in &msg.content {
                    if let ContentBlock::Text { text } = block {
                        if !text.is_empty() {
                            blocks.push(AnthropicContent::Text {
                                text: text.clone(),
                                cache_control: None,
                            });
                        }
                    }
                }
                if blocks.is_empty() {
                    blocks.push(AnthropicContent::Text {
                        text: String::new(),
                        cache_control: None,
                    });
                }
                // Merge consecutive user messages
                if let Some(last) = out.last_mut() {
                    if last.role == "user" {
                        last.content.extend(blocks);
                        continue;
                    }
                }
                out.push(AnthropicMessage { role: "user".into(), content: blocks });
            }
            "assistant" => {
                flush_results(&mut out, &mut tool_results);
                let mut blocks: Vec<AnthropicContent> = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Reasoning { reasoning } => {
                            blocks.push(AnthropicContent::Thinking {
                                thinking: reasoning.clone(),
                                signature: String::new(), // signature rebuilt from stream
                            });
                        }
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                blocks.push(AnthropicContent::Text {
                                    text: text.clone(),
                                    cache_control: None,
                                });
                            }
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            blocks.push(AnthropicContent::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                                cache_control: None,
                            });
                        }
                        _ => {}
                    }
                }

                out.push(AnthropicMessage { role: "assistant".into(), content: blocks });
            }
            "tool" => {
                for block in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content } = block {
                        let is_error = content.starts_with("[ERROR]") || content.starts_with("[FAIL]");
                        tool_results.push(AnthropicContent::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content.clone(),
                            is_error: if is_error { Some(true) } else { None },
                        });
                    }
                }
            }
            _ => {}
        }
    }

    flush_results(&mut out, &mut tool_results);
    out
}

fn flush_results(out: &mut Vec<AnthropicMessage>, pending: &mut Vec<AnthropicContent>) {
    if pending.is_empty() { return; }
    out.push(AnthropicMessage {
        role: "user".into(),
        content: std::mem::take(pending),
    });
}

// ── URL builder ──

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
