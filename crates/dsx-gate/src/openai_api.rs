//! Native OpenAI Chat Completions API streaming client.
//!
//! Messages and tools are converted from internal ContentBlock format to
//! OpenAI's chat completion format. The endpoint is DeepSeek's
//! OpenAI-compatible API at `https://api.deepseek.com/chat/completions`.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use dsx_types::{ContentBlock, Message, ToolDef, UsageInfo};
use futures_util::StreamExt;
use reqwest::Client as HttpClient;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct Provider {
    pub base_url: String,
    pub api_key: String,
}

impl Provider {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self { base_url: base_url.to_string(), api_key: api_key.to_string() }
    }
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolCallProgress {
        index: usize,
        id: String,
        name: String,
        args_so_far: String,
    },
    Done {
        raw_message: Message,
        usage: Option<UsageInfo>,
        stop_reason: Option<String>,
    },
    Balance {
        is_available: bool,
        total_balance: String,
        currency: String,
    },
    Error(String),
}

pub async fn chat_stream(
    provider: &Provider,
    model: &str,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDef>>,
    max_tokens: u32,
    effort: Option<String>,
    user_id: Option<String>,
    client: &reqwest::Client,
    tx: mpsc::Sender<StreamEvent>,
) -> anyhow::Result<()> {
    let api_msgs = convert_messages(messages, system);

    let openai_tools: Option<Vec<serde_json::Value>> = tools.map(|tds| {
        tds.into_iter()
            .map(|td| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": td.function.name,
                        "description": td.function.description,
                        "parameters": td.function.parameters,
                    }
                })
            })
            .collect()
    });

    let mut body = serde_json::Map::new();
    body.insert("model".into(), serde_json::json!(model));
    body.insert("messages".into(), serde_json::Value::Array(api_msgs));
    body.insert("stream".into(), serde_json::json!(true));
    body.insert("thinking".into(), serde_json::json!({"type": "enabled"}));
    body.insert("max_tokens".into(), serde_json::json!(max_tokens));

    if let Some(e) = effort {
        body.insert("reasoning_effort".into(), serde_json::json!(e));
    }
    if let Some(t) = openai_tools {
        body.insert("tools".into(), serde_json::Value::Array(t));
    }
    if let Some(ref uid) = user_id {
        body.insert("user_id".into(), serde_json::json!(uid));
    }

    let body = serde_json::Value::Object(body);

    let url = build_chat_url(&provider.base_url);

    dump_api_request(user_id.as_deref(), &body);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        dump_api_error(user_id.as_deref(), status.as_u16(), &text);
        let code_desc = deepseek_error_description(status.as_u16());
        let msg = format!("OpenAI API HTTP {} ({})", status, code_desc);
        let _ = tx.send(StreamEvent::Error(format!("{}: {}", msg, text))).await;
        return Err(anyhow::anyhow!("{}", msg));
    }

    let api_key_for_balance = provider.api_key.clone();
    let tx_balance = tx.clone();
    tokio::spawn(async move {
        match query_balance(&api_key_for_balance).await {
            Some(info) => {
                let _ = tx_balance.send(StreamEvent::Balance {
                    is_available: info.is_available,
                    total_balance: info.total_balance,
                    currency: info.currency,
                }).await;
            }
            None => {
                let _ = tx_balance.send(StreamEvent::Error("Balance query failed".into())).await;
            }
        }
    });

    let mut byte_stream = resp.bytes_stream();
    let mut sse_buf = String::new();
    let mut text_buf = String::new();
    let mut reasoning_buf = String::new();
    let mut tool_acc: HashMap<usize, (String, String, String)> = HashMap::new();
    let mut dsml_buf: String = String::new();
    let mut dsml_seen: HashSet<String> = HashSet::new();
    let mut usage_info: Option<UsageInfo> = None;
    let mut stop_reason: Option<String> = None;

    loop {
        let chunk = match byte_stream.next().await {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                log::warn!("SSE stream I/O error: {e}; emitting partial done with accumulated content");
                stop_reason = Some("connection_lost".to_string());
                break;
            }
            None => break,
        };
        sse_buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = sse_buf.find("\n\n") {
            let raw = sse_buf[..pos].to_string();
            sse_buf = sse_buf[pos + 2..].to_string();

            let mut data_str = String::new();
            for line in raw.lines() {
                let trimmed = line.trim();
                if let Some(dt) = trimmed.strip_prefix("data: ") {
                    data_str = dt.to_string();
                }
            }
            if data_str.is_empty() || data_str == "[DONE]" {
                continue;
            }

            let ev: serde_json::Value = match serde_json::from_str(&data_str) {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("OpenAI SSE: deserialize fail: {} — data: {}", e, data_str);
                    continue;
                }
            };

            if let Some(choices) = ev.get("choices").and_then(|c| c.as_array()) {
                if let Some(choice) = choices.first() {
                    let finish = choice.get("finish_reason").and_then(|v| v.as_str());
                    if let Some(fr) = finish {
                        if !fr.is_empty() && fr != "null" {
                            stop_reason = Some(fr.to_string());
                        }
                    }

                    if let Some(delta) = choice.get("delta") {
                        if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                            let t = text.to_string();
                            text_buf.push_str(&t);
                            let _ = tx.send(StreamEvent::ContentDelta(t.clone())).await;

                            dsml_buf.push_str(&t);
                            let mut search_from = 0usize;
                            while let Some(start) = dsml_buf[search_from..].find("<｜DSML｜invoke name=\"") {
                                let abs_start = search_from + start;
                                let after_tag = abs_start + "<｜DSML｜invoke name=\"".len();
                                if let Some(rest) = dsml_buf.get(after_tag..) {
                                    if let Some(quote_end) = rest.find('"') {
                                        let name = rest[..quote_end].to_string();
                                        if dsml_seen.insert(name.clone()) {
                                            let idx = dsml_seen.len() - 1;
                                            let _ = tx.send(StreamEvent::ToolCallProgress {
                                                index: idx,
                                                id: format!("dsml_tc_{}", idx),
                                                name,
                                                args_so_far: String::new(),
                                            }).await;
                                        }
                                        search_from = after_tag + quote_end + 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                        }

                        if let Some(rc) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                            let r = rc.to_string();
                            reasoning_buf.push_str(&r);
                            let _ = tx.send(StreamEvent::ReasoningDelta(r)).await;
                        }

                        if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                            for tc in tcs {
                                let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                                let entry = tool_acc.entry(idx).or_insert_with(|| {
                                    let tid = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let tname = tc.get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    (tid, tname, String::new())
                                });
                                if let Some(args) = tc.get("function")
                                    .and_then(|f| f.get("arguments"))
                                    .and_then(|v| v.as_str())
                                {
                                    entry.2.push_str(args);
                                    let _ = tx.send(StreamEvent::ToolCallProgress {
                                        index: idx,
                                        id: entry.0.clone(),
                                        name: entry.1.clone(),
                                        args_so_far: entry.2.clone(),
                                    }).await;
                                }
                            }
                        }
                    }
                }
            }

            if let Some(u) = ev.get("usage") {
                let pt = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let ct = u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let hit = u.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let miss = u.get("prompt_cache_miss_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let rt = u.get("completion_tokens_details")
                    .and_then(|d| d.get("reasoning_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                usage_info = Some(UsageInfo {
                    prompt_tokens: pt,
                    completion_tokens: ct,
                    total_tokens: pt + ct,
                    prompt_cache_hit_tokens: hit,
                    prompt_cache_miss_tokens: miss,
                    reasoning_tokens: rt,
                });
            }
        }
    }

    let mut blocks: Vec<ContentBlock> = Vec::new();

    if !reasoning_buf.is_empty() {
        blocks.push(ContentBlock::Reasoning {
            reasoning: reasoning_buf,
        });
    }
    if !text_buf.is_empty() {
        blocks.push(ContentBlock::text(&text_buf));
    }

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
        name: None,
        content: blocks,
    };

    let _ = tx.send(StreamEvent::Done { raw_message, usage: usage_info, stop_reason }).await;
    Ok(())
}

fn convert_messages(messages: Vec<Message>, system: Option<String>) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();

    if let Some(sys) = system {
        if !sys.is_empty() {
            out.push(serde_json::json!({"role": "system", "content": sys}));
        }
    }

    for msg in messages {
        let name = &msg.name;
        match msg.role.as_str() {
            "system" => {
                if let Some(tb) = msg.content.iter().find_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                }) {
                    let mut obj = serde_json::json!({"role": "system", "content": tb});
                    if let Some(n) = name {
                        obj["name"] = serde_json::json!(n);
                    }
                    out.push(obj);
                }
            }
            "user" => {
                let mut text = String::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(t),
                        _ => {}
                    }
                }
                let mut obj = serde_json::json!({"role": "user", "content": text});
                if let Some(n) = name {
                    obj["name"] = serde_json::json!(n);
                }
                out.push(obj);
            }
            "assistant" => {
                let mut content = String::new();
                let mut reasoning = String::new();
                let mut tool_calls: Vec<serde_json::Value> = Vec::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => content.push_str(text),
                        ContentBlock::Reasoning { reasoning: r } => reasoning.push_str(r),
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_calls.push(serde_json::json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(input).unwrap_or_default(),
                                }
                            }));
                        }
                        _ => {}
                    }
                }
                let mut obj = serde_json::json!({"role": "assistant"});
                if !content.is_empty() {
                    obj["content"] = serde_json::json!(content);
                } else if tool_calls.is_empty() && !reasoning.is_empty() {
                    obj["content"] = serde_json::json!("[Thinking complete]");
                }
                if !reasoning.is_empty() {
                    obj["reasoning_content"] = serde_json::json!(reasoning);
                }
                if !tool_calls.is_empty() {
                    obj["tool_calls"] = serde_json::json!(tool_calls);
                }
                if obj.as_object().map_or(false, |m| m.len() > 1) {
                    out.push(obj);
                }
            }
            "tool" => {
                for block in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content } = block {
                        out.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content,
                        }));
                    }
                }
            }
            _ => {}
        }
    }

    out
}

fn build_chat_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1/chat/completions") || base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{}/chat/completions", base)
    } else {
        format!("{}/v1/chat/completions", base)
    }
}

fn dump_api_request(user_id: Option<&str>, body: &serde_json::Value) {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let seed = user_id.unwrap_or("unknown");
    let path = dir.join(format!("{seed}_api.json"));
    let mut entries: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    entries.push(serde_json::json!({"ts": ts, "req": body}));
    if entries.len() > 20 { entries.remove(0); }
    if let Ok(json) = serde_json::to_string_pretty(&entries) {
        let _ = std::fs::write(&path, json);
    }
}

fn dump_api_error(user_id: Option<&str>, status: u16, text: &str) {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let seed = user_id.unwrap_or("unknown");
    let path = dir.join(format!("{seed}_api.json"));
    let mut entries: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    entries.push(serde_json::json!({"ts": ts, "err": {"status": status, "body": text}}));
    if entries.len() > 20 { entries.remove(0); }
    if let Ok(json) = serde_json::to_string_pretty(&entries) {
        let _ = std::fs::write(&path, json);
    }
}

fn log_dir() -> std::path::PathBuf {
    let mut p = dsx_types::platform::data_dir();
    p.push("logs");
    p
}

fn deepseek_error_description(status: u16) -> &'static str {
    match status {
        400 => "Bad Request — 格式错误",
        401 => "Unauthorized — API key 无效",
        402 => "Payment Required — 余额不足",
        422 => "Unprocessable — 参数错误",
        429 => "Rate Limit — 请求速率超限",
        500 => "Internal Error — 服务器故障",
        503 => "Service Unavailable — 服务器繁忙",
        _ => "Unknown",
    }
}

pub async fn query_balance(api_key: &str) -> Option<dsx_types::BalanceInfo> {
    let client = HttpClient::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build().ok()?;

    let resp = client
        .get("https://api.deepseek.com/user/balance")
        .header("Authorization", &format!("Bearer {}", api_key))
        .send().await.ok()?;

    let body: serde_json::Value = resp.json().await.ok()?;
    let is_available = body.get("is_available").and_then(|v| v.as_bool()).unwrap_or(false);
    let infos = body.get("balance_infos").and_then(|v| v.as_array())?;
    let first = infos.first()?;
    let currency = first.get("currency").and_then(|v| v.as_str()).unwrap_or("CNY").to_string();
    let total_balance = first.get("total_balance").and_then(|v| v.as_str()).unwrap_or("0").to_string();

    Some(dsx_types::BalanceInfo {
        is_available,
        currency,
        total_balance,
        granted_balance: first.get("granted_balance").and_then(|v| v.as_str()).unwrap_or("0").to_string(),
        topped_up_balance: first.get("topped_up_balance").and_then(|v| v.as_str()).unwrap_or("0").to_string(),
    })
}
