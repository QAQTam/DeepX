//! deepx-msglp: message-loop driver for the agent child process.
//!
//! The [`Loop`] reads [`Ui2Agent`] frames from stdin via JSON-LP
//! and writes [`Agent2Ui`] frames to stdout. It drives the full
//! user-input → gate → tools → response pipeline.
//!
//! Responsibilities:
//!   1. Ingest [`Ui2Agent`] frames from stdin (JSON-LP)
//!   2. Drive `UserInput` through gate → message → tools
//!   3. Propagate `Cancel` via [`CancelToken`] / `Arc<AtomicBool>`
//!   4. Emit all [`Agent2Ui`] responses to stdout
//!   5. Handle session lifecycle (CreateSession, ResumeSession, Shutdown)

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub mod agent;
use agent::AgentState;
mod lifecycle;
mod dashboard;
use dashboard::{build_documents, build_recent_edits, build_tasks};
use deepx_message::Effect;
use deepx_proto::{Agent2Ui, Ui2Agent, RoundDeltaKind};

/// Number of recent turns sent on session restore for incremental loading.
const INITIAL_LOAD_COUNT: usize = 20;

// ═══════════════════════════════════════════════════════
// CancelToken — shared abort flag
// ═══════════════════════════════════════════════════════

#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self { inner: Arc::new(AtomicBool::new(false)) }
    }

    pub fn set(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.inner.store(false, Ordering::SeqCst);
    }

    pub fn is_set(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }

    pub fn arc(&self) -> Arc<AtomicBool> {
        self.inner.clone()
    }
}

// ═══════════════════════════════════════════════════════
// LoopPhase — what's currently running
// ═══════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
enum LoopPhase {
    Idle,
    GateRunning,
    ToolsRunning,
}

// ═══════════════════════════════════════════════════════
// Loop — reads Ui2Agent from stdin (JSON-LP), writes
// Agent2Ui to stdout. `input: R` is BufRead (stdin),
// `output: W` is Write (stdout). No internal channels.
// ═══════════════════════════════════════════════════════

pub struct Loop<R: BufRead, W: Write> {
    agent: AgentState,
    input: R,
    output: W,
    cancel: CancelToken,
    phase: LoopPhase,
}

impl<R: BufRead, W: Write> Loop<R, W> {
    pub fn new_ipc(agent: AgentState, input: R, output: W) -> Self {
        let cancel = CancelToken::new();
        Self { agent, input, output, cancel, phase: LoopPhase::Idle }
    }

    pub fn run(&mut self) {
        self.agent.rebind_store();
        self.emit_dashboard();
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Ready);

        eprintln!("[AGENT] entering main event loop, waiting for Ui2Agent...");
        loop {
            let frame: Ui2Agent = match deepx_proto::read_frame(&mut self.input) {
                Ok(Some(f)) => {
                    eprintln!("[AGENT] received Ui2Agent frame");
                    f
                }
                Ok(None) => { eprintln!("[AGENT] read_frame returned None (EOF)"); break; }
                Err(e) => { eprintln!("[AGENT] read_frame error: {e}"); break; }
            };

            match frame {
                Ui2Agent::UserInput { text } => { self.handle_user_input(&text); }
                Ui2Agent::Cancel => { self.handle_cancel(); }
                Ui2Agent::CreateSession => { self.handle_create_session(); }
                Ui2Agent::ResumeSession { ref seed } => { self.handle_resume_session(seed); }
                Ui2Agent::LoadMoreTurns { ref before_turn_id, count } => {
                    let all_turns = build_turns_from_context(&self.agent);
                    let idx = all_turns.iter().position(|t| t.turn_id == *before_turn_id);
                    let end = idx.unwrap_or(all_turns.len());
                    let start = end.saturating_sub(count as usize);
                    let batch: Vec<_> = all_turns[start..end].to_vec();
                    let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::MoreTurns {
                        turns: batch,
                        has_more: start > 0,
                    });
                }
                Ui2Agent::NewSession => { self.handle_create_session(); }
                Ui2Agent::ReloadConfig => { self.handle_reload_config(); }
                Ui2Agent::Shutdown => {
                    self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
                    let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::ShutdownAck);
                    break;
                }
                Ui2Agent::ToolCall { id, name, action, args } => { self.handle_tool_call(&id, &name, &action, &args); }
                Ui2Agent::UndoTurn { ref turn_id } => { self.handle_undo_turn(turn_id); }
                Ui2Agent::Compact => { self.handle_compact(); }
                _ => {}
            }
        }

        deepx_tools::bridge::shutdown_tools();
        self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
    }

    fn handle_cancel(&mut self) {
        self.cancel.set();
        deepx_tools::CANCEL.store(true, Ordering::SeqCst);
        match self.phase {
            LoopPhase::ToolsRunning => { deepx_tools::bridge::cancel_current_tool(); }
            _ => {}
        }
        self.phase = LoopPhase::Idle;
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Cancelled);
    }

    fn handle_create_session(&mut self) {
        lifecycle::create_session(&mut self.agent);
        self.agent.rebind_store();
        self.emit_dashboard();
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::SessionCreated {
            seed: self.agent.session.seed.clone(),
        });
    }

    // Slice to the latest INITIAL_LOAD_COUNT turns for incremental loading.
    fn handle_resume_session(&mut self, seed: &str) {
        eprintln!("[AGENT] handle_resume_session seed={seed}");
        if lifecycle::init_session(&mut self.agent, Some(seed)) {
            eprintln!("[AGENT] init_session succeeded, current_seed={}", self.agent.session.seed);
            self.agent.rebind_store();
            self.emit_dashboard();
            let current_seed = self.agent.session.seed.clone();
            if current_seed == seed {
                let all_turns = build_turns_from_context(&self.agent);
                let total = all_turns.len() as u32;
                let start = total.saturating_sub(INITIAL_LOAD_COUNT as u32) as usize;
                let recent: Vec<_> = all_turns[start..].to_vec();
                let has_more = start > 0;
                eprintln!("[AGENT] sending SessionRestored, turns.len={} (total={}, has_more={})", recent.len(), total, has_more);
                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::SessionRestored {
                    seed: current_seed,
                    turns: recent,
                    tokens_used: 0,
                    cache_hit_pct: 0.0,
                    total_turns: total,
                    has_more,
                });
            } else {
                eprintln!("[AGENT] seed changed {} -> {}, sending SessionCreated", seed, current_seed);
                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::SessionCreated {
                    seed: current_seed,
                });
            }
        } else {
            eprintln!("[AGENT] init_session returned false");
            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Error {
                message: format!("Failed to resume session: {seed}"),
            });
        }
    }

    fn handle_reload_config(&mut self) {
        if let Ok(cfg) = deepx_config::Config::load() {
            self.agent.config.api_key = cfg.api_key;
            self.agent.config.model = cfg.model;
            self.agent.config.base_url = cfg.base_url;
            self.agent.config.endpoint = cfg.endpoint;
            self.agent.config.provider_id = cfg.provider_id;
            self.agent.config.reasoning_effort = cfg.reasoning_effort;
            self.agent.config.max_tokens = cfg.max_tokens;
            self.agent.config.context_limit = cfg.context_limit;
            if let Some(ref key) = cfg.context7_api_key {
                if !key.is_empty() {
                    deepx_tools::bridge::set_context7_key(key);
                }
            }
            deepx_tools::bridge::load_workspace(&self.agent.session.seed);
        }
    }

    fn handle_tool_call(&mut self, id: &str, name: &str, action: &str, args: &serde_json::Value) {
        let result = deepx_tools::bridge::execute_tool_with_id(name, action, &args.to_string(), id);
        let success = !result.starts_with("[ERROR]") && !result.starts_with("[FAIL]");
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::ToolResults {
            turn_id: "headless".into(),
            round_num: 0,
            results: vec![deepx_proto::ToolResultDef {
                tool_call_id: id.to_string(),
                output: result,
                success,
                file: None,
            }],
        });
    }

    fn handle_undo_turn(&mut self, turn_id: &str) {
        eprintln!("[AGENT] UndoTurn {turn_id} — turns before: {}", self.agent.msg.turn_count());
        if self.agent.msg.truncate_before_turn(turn_id) {
            eprintln!("[AGENT] UndoTurn — truncated, turns after: {}", self.agent.msg.turn_count());
            self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
            let all_turns = build_turns_from_context(&self.agent);
            let total = all_turns.len() as u32;
            let start = total.saturating_sub(INITIAL_LOAD_COUNT as u32) as usize;
            let recent: Vec<_> = all_turns[start..].to_vec();
            let has_more = start > 0;
            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::SessionRestored {
                seed: self.agent.session.seed.clone(),
                turns: recent,
                tokens_used: 0,
                cache_hit_pct: 0.0,
                total_turns: total,
                has_more,
            });
        } else {
            eprintln!("[AGENT] UndoTurn — truncate_before_turn returned false");
        }
    }

    fn handle_compact(&mut self) {
        const KEEP: usize = 5;
        eprintln!("[AGENT] handle_compact: {} turns", self.agent.msg.turn_count());
        if self.agent.msg.turn_count() <= KEEP {
            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::ToolNotice {
                message: format!("Compact skipped: need >{} turns (have {})", KEEP, self.agent.msg.turn_count()),
                level: "info".into(),
            });
            return;
        }

        let compact_count = self.agent.msg.turn_count() - KEEP;
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::CompactStart {
            turns_total: self.agent.msg.turn_count() as u32,
            turns_keeping: KEEP as u32,
        });

        let contexts: Vec<String> = {
            let all = self.agent.msg.build_context_for_gate("", &[]);
            all.iter()
                .filter(|m| m.role != "system")
                .take(compact_count * 3) // rough: ~3 msgs per turn
                .map(|m| {
                    let text: String = m.content.iter().filter_map(|b| match b {
                        deepx_types::ContentBlock::Text { text } => Some(text.clone()),
                        deepx_types::ContentBlock::ToolUse { name, input, .. } =>
                            Some(format!("[ToolCall {} args={}]", name, input)),
                        deepx_types::ContentBlock::ToolResult { content, .. } =>
                            Some(format!("[ToolResult {}]", &content[..content.len().min(300)])),
                        _ => None,
                    }).collect::<Vec<_>>().join("\n");
                    format!("[{}]: {}", m.role, &text[..text.len().min(1000)])
                })
                .collect()
        };
        if contexts.is_empty() { return; }

        let prompt = build_compact_prompt(&contexts);
        let provider = deepx_gate::ProviderConfig::openai(
            &self.agent.config.base_url, &self.agent.config.api_key,
            &self.agent.config.model, None, None, None,
            Default::default(), Default::default(), false, false,
        );
        let msgs = vec![deepx_types::Message::user(&prompt)];
        let summary = match deepx_gate::chat_sync(&provider, msgs, 2048) {
            Ok(s) if !s.trim().is_empty() => s,
            Ok(_) => {
                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Error {
                    message: "Compact failed: model returned empty response. Try again.".into(),
                });
                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::CompactEnd { summary_chars: 0, turns_compacted: 0 });
                return;
            }
            Err(e) => {
                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Error { message: e });
                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::CompactEnd { summary_chars: 0, turns_compacted: 0 });
                return;
            }
        };

        let chars = summary.chars().count();
        self.agent.msg.apply_compact(&summary, KEEP);
        self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::CompactEnd {
            summary_chars: chars, turns_compacted: compact_count as u32,
        });
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::ToolNotice {
            message: format!("Compacted {} turns → {} chars summary", compact_count, chars),
            level: "info".into(),
        });
        self.emit_dashboard();
    }

    // ── User input handler ──

    fn handle_user_input(&mut self, text: &str) {
        if self.agent.session.seed.is_empty() {
            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Error {
                message: "No session — create one first".into(),
            });
            return;
        }

        self.cancel.clear();
        // turn_state deleted — cancel handled by CancelToken
        deepx_tools::CANCEL.store(false, Ordering::SeqCst);
        
        self.agent.msg.push_user(text);

        let turn_id = format!("t{}", self.agent.msg.turn_count());
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::TurnStart {
            turn_id: turn_id.clone(),
            user_text: text.to_string(),
        });

        let ep = deepx_config::registry::find_endpoint(&self.agent.config.provider_id, &self.agent.config.endpoint);
        let provider = deepx_gate::ProviderConfig::openai(
            &self.agent.config.base_url,
            &self.agent.config.api_key,
            &self.agent.config.model,
            ep.as_ref().and_then(|e| e.user_id_mode.clone()),
            ep.as_ref().and_then(|e| e.chat_path.clone()),
            ep.as_ref().and_then(|e| e.balance_path.clone()),
            ep.as_ref().map(|e| e.thinking_mode.clone()).unwrap_or_default(),
            ep.as_ref().map(|e| e.cache_field.clone()).unwrap_or_default(),
            ep.as_ref().map(|e| e.has_balance).unwrap_or(true),
            ep.as_ref().map(|e| e.supports_thinking).unwrap_or(true),
        );

          let mut round_num = 0u32;
          let mut last_usage: Option<deepx_types::UsageInfo> = None;

          // Delta batching: accumulate deltas and flush every ~30ms
          let mut answer_buf = String::new();
          let mut think_buf = String::new();
          let mut last_flush = std::time::Instant::now();
          const FLUSH_INTERVAL_MS: u64 = 30;
          const FLUSH_CHAR_THRESHOLD: usize = 20;

        loop {
            if self.cancel.is_set() || deepx_tools::CANCEL.load(Ordering::SeqCst) {
                self.agent.msg.remove_last_step_if_incomplete();
                self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
                break;
            }

            let messages = self.agent.build_context();

            let tools = Some(self.agent.tool_defs.clone());
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut tool_calls_raw = serde_json::Value::Null;
            let mut had_error = false;

            self.phase = LoopPhase::GateRunning;
        let result = deepx_gate::chat_stream(
                &provider,
                messages,
                tools,
                self.agent.config.max_tokens,
                Some(self.agent.config.reasoning_effort.clone()),
                Some(self.agent.session.seed.clone()),
                &mut |event| {
                    match event {
                        deepx_gate::StreamEvent::ContentDelta(d) => {
                            if self.cancel.is_set() { return; }
                            content.push_str(&d);
                            answer_buf.push_str(&d);
                            if last_flush.elapsed().as_millis() as u64 >= FLUSH_INTERVAL_MS
                                || answer_buf.len() >= FLUSH_CHAR_THRESHOLD
                            {
                                if !think_buf.is_empty() {
                                    let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::RoundDelta {
                                        turn_id: turn_id.clone(), round_num,
                                        kind: RoundDeltaKind::Thinking,
                                        delta: std::mem::take(&mut think_buf),
                                    });
                                }
                                if !answer_buf.is_empty() {
                                    let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::RoundDelta {
                                        turn_id: turn_id.clone(), round_num,
                                        kind: RoundDeltaKind::Answering,
                                        delta: std::mem::take(&mut answer_buf),
                                    });
                                }
                                last_flush = std::time::Instant::now();
                            }
                        }
                        deepx_gate::StreamEvent::ReasoningDelta(r) => {
                            if self.cancel.is_set() { return; }
                            reasoning.push_str(&r);
                            think_buf.push_str(&r);
                            if last_flush.elapsed().as_millis() as u64 >= FLUSH_INTERVAL_MS
                                || think_buf.len() >= FLUSH_CHAR_THRESHOLD
                            {
                                if !think_buf.is_empty() {
                                    let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::RoundDelta {
                                        turn_id: turn_id.clone(), round_num,
                                        kind: RoundDeltaKind::Thinking,
                                        delta: std::mem::take(&mut think_buf),
                                    });
                                }
                                if !answer_buf.is_empty() {
                                    let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::RoundDelta {
                                        turn_id: turn_id.clone(), round_num,
                                        kind: RoundDeltaKind::Answering,
                                        delta: std::mem::take(&mut answer_buf),
                                    });
                                }
                                last_flush = std::time::Instant::now();
                            }
                        }
                        deepx_gate::StreamEvent::Done { raw_message, usage, .. } => {
                            // Flush buffered deltas before processing completion
                            if !think_buf.is_empty() {
                                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::RoundDelta {
                                    turn_id: turn_id.clone(), round_num,
                                    kind: RoundDeltaKind::Thinking,
                                    delta: std::mem::take(&mut think_buf),
                                });
                            }
                            if !answer_buf.is_empty() {
                                let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::RoundDelta {
                                    turn_id: turn_id.clone(), round_num,
                                    kind: RoundDeltaKind::Answering,
                                    delta: std::mem::take(&mut answer_buf),
                                });
                            }
                            if let Some(ref u) = usage {
                                self.agent.session.tokens += u.total_tokens as u64;
                                last_usage = usage.clone();
                            }
                            content.clear();
                            reasoning.clear();
                            let mut blocks: Vec<serde_json::Value> = Vec::new();
                            for block in &raw_message.content {
                                match block {
                                    deepx_types::ContentBlock::Text { text } => content.push_str(text),
                                    deepx_types::ContentBlock::Reasoning { reasoning: r } => reasoning.push_str(r),
                                    deepx_types::ContentBlock::ToolUse { id, name, input } => {
                                        blocks.push(serde_json::json!({
                                            "id": id,
                                            "name": name,
                                            "arguments": input.to_string(),
                                        }));
                                    }
                                    _ => {}
                                }
                            }
                            if !blocks.is_empty() {
                                tool_calls_raw = serde_json::Value::Array(blocks);
                            }
                        }
                        deepx_gate::StreamEvent::Retrying { attempt, max_retries, delay_secs, error } => {
                            let msg = format!("API error, retrying ({attempt}/{max_retries}) in {delay_secs}s: {error}");
                            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Error { message: msg });
                        }
                        deepx_gate::StreamEvent::Error(msg) => {
                            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Error { message: msg });
                            had_error = true;
                        }
                        _ => {}
                    }
                },
            );

            if had_error || result.is_err() {
                self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
                break;
            }

            let parsed = parse_tool_calls_from_response(&content, &reasoning, &tool_calls_raw, &self.agent);
            let assistant_msg = build_assistant_message(&content, &reasoning, &parsed);
            let effect = self.agent.msg.push_assistant(assistant_msg.clone());

            emit_round_complete(&self.agent, &mut self.output, &turn_id, round_num, &assistant_msg, &content, &reasoning, &parsed);

            match effect {
                Effect::None => {
                    self.phase = LoopPhase::ToolsRunning;
                    self.agent.msg.execute_tools_batch();
                    let results = self.agent.msg.last_step_tool_results();
                    let mut tool_defs = Vec::new();
                    for (tc_id, tool_name, result_content, success) in &results {
                        tool_defs.push(deepx_proto::ToolResultDef {
                            tool_call_id: tc_id.clone(),
                            output: result_content.clone(),
                            success: *success,
                            file: None,
                        });
                        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::AuditRecord {
                            tool_name: tool_name.clone(),
                            result_summary: result_content.lines().next().unwrap_or("").chars().take(120).collect(),
                            success: *success,
                        });
                    }
                    if !tool_defs.is_empty() {
                        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::ToolResults {
                            turn_id: turn_id.clone(),
                            round_num,
                            results: tool_defs,
                        });
                    }

                    round_num += 1;
                    continue;
                }
                Effect::TurnComplete => {}
                _ => {}
            }

            self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);

            let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::TurnEnd {
                turn_id: turn_id.clone(),
                stop_reason: None,
                usage: last_usage.clone(),
            });

            break;
        }

        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Done);
    }

    // ── Dashboard ──

    fn emit_dashboard(&mut self) {
        let _ = deepx_proto::write_frame(&mut self.output, &Agent2Ui::Dashboard {
            hp_connected: true,
            session_seed: self.agent.session.seed.clone(),
            context_limit: self.agent.config.context_limit,
            tool_calls_total: 0,
            tool_failures: 0,
            current_phase: "single".into(),
            streaming: false,
            dsml_compat_count: self.agent.dsml_compat_count,
            documents: build_documents(&self.agent),
            recent_edits: build_recent_edits(&self.agent),
            tasks: build_tasks(&self.agent),
            session_title: self.agent.session.title.clone(),
            usage: None,
            model: Some(self.agent.config.model.clone()),
        });
    }
}

// ═══════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════

fn parse_tool_calls_from_response(
    content: &str, _reasoning: &str, tool_calls_raw: &serde_json::Value,
    agent: &AgentState,
) -> Vec<deepx_types::ToolCall> {
    let mut parsed = deepx_gate::tool_parser::parse_tool_calls(tool_calls_raw);
    if parsed.is_empty() {
        let stripped = deepx_gate::tool_parser::strip_fenced_code(content);
        if deepx_gate::tool_parser::has_dsml(&stripped) {
            let (_, dsml) = deepx_gate::tool_parser::parse_dsml_tool_calls(&stripped, &agent.tool_defs);
            if !dsml.is_empty() { parsed = dsml; }
        }
        if parsed.is_empty() && has_xml(content) {
            let names: Vec<String> = agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let stripped2 = deepx_gate::tool_parser::strip_fenced_code(content);
            let (_, xml) = deepx_gate::tool_parser::parse_xml_tool_calls(&stripped2, &names);
            if !xml.is_empty() { parsed = xml; }
        }
    }
    parsed
}

fn has_xml(s: &str) -> bool {
    s.contains("<tool_use>") || s.contains("<invoke ") || s.contains("<tool_calls>")
}

fn build_assistant_message(
    content: &str, reasoning: &str, parsed: &[deepx_types::ToolCall],
) -> deepx_types::Message {
    use deepx_types::{ContentBlock, Message};
    let mut blocks = Vec::new();
    if !reasoning.is_empty() {
        blocks.push(ContentBlock::Reasoning { reasoning: reasoning.to_string() });
    }
    if !content.is_empty() {
        blocks.push(ContentBlock::Text { text: content.to_string() });
    }
    for tc in parsed {
        let input: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap_or_default();
        blocks.push(ContentBlock::ToolUse { id: tc.id.clone(), name: tc.function.name.clone(), input });
    }
    Message { role: "assistant".into(), name: None, content: blocks }
}

fn emit_round_complete(
    _agent: &AgentState, output: &mut impl Write,
    turn_id: &str, round_num: u32, assistant_msg: &deepx_types::Message,
    content: &str, reasoning: &str, _parsed: &[deepx_types::ToolCall],
) {
    use deepx_types::ContentBlock;
    let mut blocks = Vec::new();
    let mut tool_calls = Vec::new();
    for cb in &assistant_msg.content {
        match cb {
            ContentBlock::Reasoning { reasoning } if !reasoning.is_empty() => {
                blocks.push(deepx_proto::RoundBlock::Reasoning { content: reasoning.clone() });
            }
            ContentBlock::Text { text } if !text.is_empty() => {
                blocks.push(deepx_proto::RoundBlock::Text { content: text.clone() });
            }
            ContentBlock::ToolUse { id, name, input } => {
                let display = if name == "ask_user" {
                    input.get("question")
                        .and_then(|v| v.as_str())
                        .map(|q| q.to_string())
                        .unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                };
                tool_calls.push(deepx_proto::ToolCallDef {
                    id: id.clone(), name: name.clone(),
                    args_display: display.clone(), args_json: input.to_string(),
                });
                blocks.push(deepx_proto::RoundBlock::Tool {
                    card: deepx_proto::ToolCallDef {
                        id: id.clone(), name: name.clone(),
                        args_display: display, args_json: input.to_string(),
                    },
                });
            }
            _ => {}
        }
    }
    let _ = deepx_proto::write_frame(output, &Agent2Ui::RoundComplete {
        turn_id: turn_id.into(),
        round_num,
        thinking: if reasoning.is_empty() { None } else { Some(reasoning.into()) },
        answer: if content.is_empty() { None } else { Some(content.into()) },
        tool_calls: tool_calls.clone(),
        blocks,
        is_final: tool_calls.is_empty(),
    });
}

fn build_turns_from_context(agent: &AgentState) -> Vec<deepx_proto::TurnData> {
    use deepx_types::ContentBlock;
    let mut turns = Vec::new();
    for (ti, turn) in agent.msg.turns().iter().enumerate() {
        let mut rounds = Vec::new();
        for (ri, step) in turn.steps.iter().enumerate() {
            let thinking = step.assistant.content.iter().find_map(|b| {
                if let ContentBlock::Reasoning { reasoning } = b { Some(reasoning.clone()) } else { None }
            });
            let answer = step.assistant.content.iter().find_map(|b| {
                if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
            });
            let tcs: Vec<deepx_proto::ToolCallDef> = step.assistant.content.iter().filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some(deepx_proto::ToolCallDef {
                        id: id.clone(), name: name.clone(),
                        args_display: name.clone(), args_json: input.to_string(),
                    })
                } else { None }
            }).collect();
            let trs: Vec<deepx_proto::ToolResultDef> = step.tool_results.iter().filter_map(|tr| {
                tr.content.iter().find_map(|b| {
                    if let ContentBlock::ToolResult { tool_use_id, content } = b {
                        Some(deepx_proto::ToolResultDef {
                            tool_call_id: tool_use_id.clone(),
                            output: content.clone(), success: true, file: None,
                        })
                    } else { None }
                })
            }).collect();
            rounds.push(deepx_proto::RoundData {
                round_num: ri as u32, thinking, answer, tool_calls: tcs, tool_results: trs,
            });
        }
        let user_text = turn.user.content.iter().find_map(|b| {
            if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
        }).unwrap_or_default();
        turns.push(deepx_proto::TurnData {
            turn_id: format!("t{}", ti + 1), user_text, rounds,
        });
    }
    turns
}

fn build_compact_prompt(contexts: &[String]) -> String {
    let conv = contexts.join("\n");
    format!(
        "Summarize this conversation history into a compact summary.\n\
        Keep: user intents, operations performed (tool calls + results), files changed, unfinished tasks.\n\
        Drop: verbatim code, full tool outputs, thinking details.\n\
        Use concise bullet points under 1500 characters.\n\n\
        {}\n\nSummary:", conv
    )
}