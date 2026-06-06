//! dsx-gate — API Proxy daemon.
//!
//! Security boundary process that holds API keys and proxies LLM requests.
//!
//! ## IPC protocol
//!
//! JSON-LP frames over TCP `localhost`. Port written to
//! `{data_dir}/hp.port`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;
use std::thread;

use crate::openai_api::{Provider, StreamEvent};
use dsx_proto::AgentToHp;

static HP_CONFIG: OnceLock<Provider> = OnceLock::new();
static HP_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

// ── Main ──

/// Load API config: config.json (priority) then env vars, then defaults.
fn load_hp_config() -> Provider {
    let store = dsx_types::ConfigStore::default_location();
    let api_key = store.load_api_key().unwrap_or_default();
    let base_url = store
        .load_value()
        .and_then(|v| v.get("base_url").and_then(|b| b.as_str()).map(String::from))
        .unwrap_or_else(|| "https://api.deepseek.com".into());
    Provider::new(&base_url, &api_key)
}

pub fn run() {
    let hp_cfg = load_hp_config();
    let _ = HP_CONFIG.set(hp_cfg);
    let _ = HP_RUNTIME.set(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create tokio runtime")
    );
    if HP_CONFIG.get().map_or(true, |c| c.api_key.is_empty()) {
        eprintln!("dsx-gate: WARNING — no API key configured, run 'dsx config' to set up");
    } else {
        eprintln!("dsx-gate: API proxy configured");
    }

    let port = std::env::var("DSX_HP_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(0);

    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).expect("dsx-gate: failed to bind TCP listener");
    let actual_port = listener.local_addr().expect("listener local_addr").port();

    write_port_file(actual_port);
    eprintln!("dsx-gate: listening on 127.0.0.1:{actual_port}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| handle_connection(stream));
            }
            Err(e) => {
                eprintln!("dsx-gate: accept error: {e}");
            }
        }
    }

    clear_port_file();
}

// ── Connection handler ──

fn handle_connection(stream: TcpStream) {
    let peer = stream.peer_addr().ok();
    let mut reader = match stream.try_clone() {
        Ok(s) => BufReader::new(s),
        Err(e) => {
            eprintln!("dsx-gate: try_clone failed for {:?}: {e}", peer);
            return;
        }
    };

    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = buf.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(AgentToHp::ApiChat { .. }) = serde_json::from_str(trimmed) {
                    if let Ok(mut w) = stream.try_clone() {
                        handle_api_chat_streaming(trimmed, &mut w);
                    }
                } else {
                    let response = dispatch_frame(trimmed);
                    if let Ok(mut w) = stream.try_clone() {
                        let _ = writeln!(w, "{response}");
                        let _ = w.flush();
                    }
                }
            }
            Err(_) => break,
        }
    }

    if let Some(addr) = peer {
        eprintln!("dsx-gate: connection closed: {addr}");
    }
}

/// Minimal dispatch — Register/Heartbeat/Unregister are accepted as no-ops.
/// All other frame types (Judge, Query, List) are obsolete and ignored.
fn dispatch_frame(line: &str) -> String {
    match serde_json::from_str::<AgentToHp>(line) {
        Ok(AgentToHp::Register { .. }) => json_response("ok", "registered"),
        Ok(AgentToHp::Heartbeat { .. }) => json_response("ok", "heartbeat recorded"),
        Ok(AgentToHp::Unregister { .. }) => json_response("ok", "unregistered"),
        Ok(_) => json_response("ok", "ok"),
        Err(e) => json_response("error", &format!("invalid frame: {e}")),
    }
}

// ── Helpers ──

fn json_response(status: &str, message: &str) -> String {
    serde_json::json!({"type": status, "message": message}).to_string()
}

/// Streaming variant of `handle_api_chat` — writes content deltas to `writer`
/// as they arrive from the LLM, then a final `api_response` frame.
fn handle_api_chat_streaming(line: &str, writer: &mut impl Write) {
    let config = match HP_CONFIG.get() {
        Some(c) => c.clone(),
        None => {
            let _ = writeln!(writer, "{}", json_response("error", "HP not configured (no API key)"));
            let _ = writer.flush();
            return;
        }
    };

    let frame: AgentToHp = match serde_json::from_str(line) {
        Ok(f) => f,
        Err(e) => {
            let _ = writeln!(writer, "{}", json_response("error", &format!("invalid api_chat frame: {e}")));
            let _ = writer.flush();
            return;
        }
    };

    let (model, system, effort, max_tokens, user_id, api_key, messages_val, tools_val) = match frame {
        AgentToHp::ApiChat { model, system, messages, effort, max_tokens, tools, user_id, api_key } => {
            let api_key = api_key
                .filter(|k| !k.0.is_empty())
                .map(|r| r.0)
                .unwrap_or_else(|| config.api_key.clone());
            (model, system, effort, max_tokens.unwrap_or(16000), user_id, api_key, messages, tools)
        }
        _ => {
            let _ = writeln!(writer, "{}", json_response("error", "expected api_chat frame"));
            let _ = writer.flush();
            return;
        }
    };

    let messages_val: Vec<dsx_types::Message> = match serde_json::from_value(messages_val) {
        Ok(msgs) => msgs,
        Err(_) => {
            let _ = writeln!(writer, "{}", json_response("error", "api_chat: invalid messages"));
            let _ = writer.flush();
            return;
        }
    };

    let rt = match HP_RUNTIME.get() {
        Some(r) => r,
        None => {
            let _ = writeln!(writer, "{}", json_response("error", "HP runtime not ready"));
            let _ = writer.flush();
            return;
        }
    };

    let tools: Option<Vec<dsx_types::ToolDef>> = tools_val
        .and_then(|t| serde_json::from_value(t).ok());

    let provider_cfg = Provider::new(&config.base_url, &api_key);

    // Reusable HTTP client — avoids building a new one per request
    let http_client = HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120))
            .pool_max_idle_per_host(0)
            .build()
            .expect("build reqwest client")
    });

    rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

        let msgs = messages_val.clone();
        let model_o = model.to_string();
        let sys = system;
        let gw = provider_cfg;
        let effort_o = effort.clone();
        let client = http_client.clone();
        tokio::spawn(async move {
            let uid = user_id.clone();
            let result = crate::openai_api::chat_stream(
                &gw, &model_o, sys, msgs, tools, max_tokens, effort_o, uid, &client, tx,
            ).await;
            if let Err(e) = result {
                eprintln!("dsx-gate: gateway error: {e:?}");
                log::error!("dsx-gate: chat_stream failed: {e:?}");
            }
        });

        let mut full_content = String::new();
        let mut reasoning = String::new();
        // Line-buffered forwarding: aggregate token-level SSE deltas into
        // complete lines before sending.  This eliminates the fragile
        // character-by-character draft management on the TUI side while
        // keeping sub-100ms latency (lines flush on every \n boundary).
        let mut content_line_buf = String::new();
        let mut reasoning_line_buf = String::new();

        /// Drain complete lines from `buf` and write them as frames.
        /// `is_reasoning`: true → write to `reasoning` field (and empty `delta`).
        ///                 false → write to `delta` field.
        fn flush_lines(buf: &mut String, is_reasoning: bool, writer: &mut impl Write) {
            while let Some(nl) = buf.find('\n') {
                let line: String = buf.drain(..=nl).collect();
                let frame = if is_reasoning {
                    serde_json::json!({
                        "type": "content_delta",
                        "delta": "",
                        "reasoning": line,
                    })
                } else {
                    serde_json::json!({
                        "type": "content_delta",
                        "delta": line,
                    })
                };
                if let Ok(s) = serde_json::to_string(&frame) {
                    let _ = writeln!(writer, "{s}");
                    let _ = writer.flush();
                }
            }
        }

        /// Flush any remaining incomplete line in `buf`.
        fn flush_remainder(buf: &mut String, is_reasoning: bool, writer: &mut impl Write) {
            if buf.is_empty() { return; }
            let tail: String = buf.drain(..).collect();
            let frame = if is_reasoning {
                serde_json::json!({
                    "type": "content_delta",
                    "delta": "",
                    "reasoning": tail,
                })
            } else {
                serde_json::json!({
                    "type": "content_delta",
                    "delta": tail,
                })
            };
            if let Ok(s) = serde_json::to_string(&frame) {
                let _ = writeln!(writer, "{s}");
                let _ = writer.flush();
            }
        }

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::ContentDelta(delta) => {
                    full_content.push_str(&delta);
                    content_line_buf.push_str(&delta);
                    flush_lines(&mut content_line_buf, false, writer);
                }
                StreamEvent::ReasoningDelta(delta) => {
                    reasoning.push_str(&delta);
                    reasoning_line_buf.push_str(&delta);
                    flush_lines(&mut reasoning_line_buf, true, writer);
                }
                StreamEvent::ToolCallProgress { ref id, ref name, ref args_so_far, .. } => {
                    let frame = serde_json::json!({
                        "type": "tool_progress",
                        "id": id,
                        "name": name,
                        "content": args_so_far,
                        "stream_type": "progress",
                    });
                    if let Ok(s) = serde_json::to_string(&frame) {
                        let _ = writeln!(writer, "{s}");
                        let _ = writer.flush();
                    }
                }
                StreamEvent::Done { raw_message, stop_reason: sr, usage } => {
                    // Flush any remaining incomplete lines before the final response
                    flush_remainder(&mut reasoning_line_buf, true, writer);
                    flush_remainder(&mut content_line_buf, false, writer);
                    build_final_response(raw_message, sr, usage, &mut full_content, &mut reasoning, writer);
                    return;
                }
                StreamEvent::Balance { is_available, total_balance, currency } => {
                    let frame = serde_json::json!({
                        "type": "balance",
                        "is_available": is_available,
                        "total_balance": total_balance,
                        "currency": currency,
                    });
                    if let Ok(s) = serde_json::to_string(&frame) {
                        let _ = writeln!(writer, "{s}");
                        let _ = writer.flush();
                    }
                }
                  StreamEvent::Error(e) => {
                      log::error!("hp: API error (non-fatal) — {}", e);
                      let _ = writeln!(writer, "{}", json_response("error", &e));
                      let _ = writer.flush();
                      // Do NOT return — keep processing remaining stream events.
                  }
            }
        }
    });
}

/// Build and write the final API response from the Done stream event.
fn build_final_response(
    raw_message: dsx_types::Message,
    stop_reason: Option<String>,
    usage: Option<dsx_types::UsageInfo>,
    full_content: &mut String,
    reasoning: &mut String,
    writer: &mut impl Write,
) {
    full_content.clear();
    reasoning.clear();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    for block in &raw_message.content {
        match block {
            dsx_types::ContentBlock::Text { text } => {
                full_content.push_str(text);
            }
            dsx_types::ContentBlock::Reasoning { reasoning: r } => {
                reasoning.push_str(r);
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
    let mut resp = serde_json::json!({
        "type": "api_response",
        "content": full_content,
    });
    if !reasoning.is_empty() {
        resp["reasoning_content"] = serde_json::json!(reasoning);
    }
    if !tool_calls.is_empty() {
        resp["tool_calls"] = serde_json::Value::Array(tool_calls);
    }
    if let Some(ref s) = stop_reason {
        resp["stop_reason"] = serde_json::json!(s);
    }
    if let Some(ref u) = usage {
        resp["usage"] = serde_json::json!({
            "prompt_tokens": u.prompt_tokens,
            "completion_tokens": u.completion_tokens,
            "total_tokens": u.total_tokens,
            "prompt_cache_hit_tokens": u.prompt_cache_hit_tokens,
            "prompt_cache_miss_tokens": u.prompt_cache_miss_tokens,
            "reasoning_tokens": u.reasoning_tokens,
        });
    }
    if let Ok(s) = serde_json::to_string(&resp) {
        let _ = writeln!(writer, "{s}");
        let _ = writer.flush();
    }
}

// ── Port file management ──

fn port_file_path() -> std::path::PathBuf {
    dsx_types::platform::hp_port_path()
}

fn write_port_file(port: u16) {
    let path = port_file_path();
    if let Some(parent) = path.parent() {
        drop(std::fs::create_dir_all(parent));
    }
    drop(std::fs::write(&path, port.to_string()));
}

fn clear_port_file() {
    drop(std::fs::write(port_file_path(), ""));
}