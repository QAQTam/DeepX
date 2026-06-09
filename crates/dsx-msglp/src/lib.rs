//! dsx-msglp: minimal message-loop driver.
//!
//! Responsibilities:
//!   1. Receive Ui2Agent events from the frontend
//!   2. Drive `UserInput` through gate → message → tools
//!   3. Propagate `Cancel` to all modules via `Arc<AtomicBool>`
//!   4. Handle session lifecycle (CreateSession, Shutdown)

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dsx_agent::agent::AgentState;
use dsx_agent::runner::lifecycle;
use dsx_message::{Effect, ToolExecRequest, ToolExecReport};
use dsx_proto::{Agent2Ui, Ui2Agent, RoundDeltaKind};

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
// Loop — the minimal driver
// ═══════════════════════════════════════════════════════

pub struct Loop {
    agent: AgentState,
    ui_rx: mpsc::Receiver<Ui2Agent>,
    ui_tx: mpsc::Sender<Agent2Ui>,
    cancel: CancelToken,
}

impl Loop {
    pub fn new(
        agent: AgentState,
        ui_rx: mpsc::Receiver<Ui2Agent>,
        ui_tx: mpsc::Sender<Agent2Ui>,
    ) -> Self {
        let cancel = CancelToken::new();
        Self { agent, ui_rx, ui_tx, cancel }
    }

    pub fn run(&mut self) {
        // ── Inject UI sender + cancel + tool executor into MessageStore ──
        self.agent.msg.set_ui_tx(self.ui_tx.clone());
        self.agent.msg.set_cancel(self.cancel.arc());
        self.agent.msg.set_tool_executor(Box::new(|req: ToolExecRequest| {
            let result = dsx_agent::tools::execute_tool_with_id(&req.name, "", &req.args.to_string(), &req.id);
            let success = !result.starts_with("[ERROR]") && !result.starts_with("[FAIL]");
            ToolExecReport { content: result, success, files_affected: Vec::new() }
        }));

        // ── Emit initial dashboard ──
        self.emit_dashboard();

        // ── Auto-resume from seed ──
        if self.agent.session.seed.is_empty()
            && self.agent.session.resume_seed.is_some()
        {
            let seed = self.agent.session.resume_seed.clone();
            if lifecycle::init_session(&mut self.agent, seed.as_deref()) {
                let _ = self.ui_tx.send(Agent2Ui::SessionRestored {
                    seed: self.agent.session.seed.clone(),
                    turns: build_turns_from_context(&self.agent),
                    tokens_used: 0,
                    cache_hit_pct: 0.0,
                });
            }
        }

        // ── Main event loop ──
        loop {
            let frame = match self.ui_rx.recv() {
                Ok(f) => f,
                Err(_) => break,
            };

            match frame {
                Ui2Agent::UserInput { text } => {
                    self.handle_user_input(&text);
                }

                Ui2Agent::Cancel => {
                    self.cancel.set();
                    dsx_tools::CANCEL.store(true, Ordering::SeqCst);
                    self.agent.turn.stream_cancelled = true;
                    dsx_agent::tools::cancel_current_tool();
                    let _ = self.ui_tx.send(Agent2Ui::Cancelled);
                }

                Ui2Agent::CreateSession => {
                    lifecycle::create_session(&mut self.agent);
                    let _ = self.ui_tx.send(Agent2Ui::SessionCreated {
                        seed: self.agent.session.seed.clone(),
                    });
                }

                Ui2Agent::ReloadConfig => {
                    if let Ok(cfg) = dsx_agent::config::Config::load() {
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
                                dsx_agent::tools::set_context7_key(key);
                            }
                        }
                        dsx_agent::tools::load_workspace(&self.agent.session.seed);
                    }
                }

                Ui2Agent::Shutdown => {
                    self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
                    let _ = self.ui_tx.send(Agent2Ui::ShutdownAck);
                    break;
                }

                Ui2Agent::ToolCall { id, name, action, args } => {
                    let result = dsx_agent::tools::execute_tool_with_id(&name, &action, &args.to_string(), &id);
                    let success = !result.starts_with("[ERROR]") && !result.starts_with("[FAIL]");
                    let _ = self.ui_tx.send(Agent2Ui::ToolResults {
                        turn_id: "headless".into(),
                        round_num: 0,
                        results: vec![dsx_proto::ToolResultDef {
                            tool_call_id: id,
                            output: result,
                            success,
                            file: None,
                        }],
                    });
                }

                _ => {}
            }
        }

        dsx_agent::tools::shutdown_tools();
        self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);
        log::info!(
            "dsx-msglp: shutdown complete (session {}, {} turns, {} tokens)",
            self.agent.session.seed,
            self.agent.msg.turn_count(),
            self.agent.session.tokens
        );
    }

    // ── User input handler ──

    fn handle_user_input(&mut self, text: &str) {
        if self.agent.session.seed.is_empty() {
            let _ = self.ui_tx.send(Agent2Ui::Error {
                message: "No session — create one first".into(),
            });
            return;
        }

        self.cancel.clear();
        self.agent.turn.stream_cancelled = false;
        dsx_tools::CANCEL.store(false, Ordering::SeqCst);
        self.agent.refresh_progress_context();

        self.agent.msg.push_user(text);

        let turn_id = format!("t{}", self.agent.msg.turn_count());
        let _ = self.ui_tx.send(Agent2Ui::TurnStart {
            turn_id: turn_id.clone(),
            user_text: text.to_string(),
        });

        let provider = dsx_agent::gate::ProviderConfig::openai(
            &self.agent.config.base_url,
            &self.agent.config.api_key,
            &self.agent.config.model,
            None,
        );

        let mut round_num = 0u32;
        let mut last_usage: Option<dsx_types::UsageInfo> = None;

        loop {
            if self.cancel.is_set() || dsx_tools::CANCEL.load(Ordering::SeqCst) {
                self.agent.msg.remove_last_step_if_incomplete();
                break;
            }

            let messages = self.agent.build_context();

            let tools = Some(self.agent.tool_defs.clone());
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut tool_calls_raw = serde_json::Value::Null;
            let mut had_error = false;

            let result = dsx_agent::gate::chat_stream(
                &provider,
                messages,
                tools,
                self.agent.config.max_tokens,
                Some(self.agent.config.reasoning_effort.clone()),
                Some(self.agent.session.seed.clone()),
                &mut |event| {
                    match event {
                        dsx_agent::gate::StreamEvent::ContentDelta(d) => {
                            if self.cancel.is_set() { return; }
                            content.push_str(&d);
                            let _ = self.ui_tx.send(Agent2Ui::RoundDelta {
                                turn_id: turn_id.clone(),
                                round_num,
                                kind: RoundDeltaKind::Answering,
                                delta: d,
                            });
                        }
                        dsx_agent::gate::StreamEvent::ReasoningDelta(r) => {
                            if self.cancel.is_set() { return; }
                            reasoning.push_str(&r);
                        }
                        dsx_agent::gate::StreamEvent::Done { raw_message, usage, .. } => {
                            if let Some(ref u) = usage {
                                self.agent.session.tokens += u.total_tokens as u64;
                                last_usage = usage.clone();
                            }
                            content.clear();
                            reasoning.clear();
                            let mut blocks: Vec<serde_json::Value> = Vec::new();
                            for block in &raw_message.content {
                                match block {
                                    dsx_types::ContentBlock::Text { text } => content.push_str(text),
                                    dsx_types::ContentBlock::Reasoning { reasoning: r } => reasoning.push_str(r),
                                    dsx_types::ContentBlock::ToolUse { id, name, input } => {
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
                        dsx_agent::gate::StreamEvent::Error(msg) => {
                            let _ = self.ui_tx.send(Agent2Ui::Error { message: msg });
                            had_error = true;
                        }
                        _ => {}
                    }
                },
            );

            if had_error || result.is_err() {
                break;
            }

            let parsed = parse_tool_calls_from_response(&content, &reasoning, &tool_calls_raw, &self.agent);
            let assistant_msg = build_assistant_message(&content, &reasoning, &parsed);
            let effect = self.agent.msg.push_assistant(assistant_msg.clone());

            emit_round_complete(&self.agent, &self.ui_tx, &turn_id, round_num, &assistant_msg, &content, &reasoning, &parsed);

            match effect {
                Effect::None => {
                    self.agent.msg.execute_tools_batch();
                    let results = self.agent.msg.last_step_tool_results();
                    let mut tool_defs = Vec::new();
                    for (tc_id, tool_name, result_content, success) in &results {
                        tool_defs.push(dsx_proto::ToolResultDef {
                            tool_call_id: tc_id.clone(),
                            output: result_content.clone(),
                            success: *success,
                            file: None,
                        });
                        let _ = self.ui_tx.send(Agent2Ui::AuditRecord {
                            tool_name: tool_name.clone(),
                            result_summary: result_content.lines().next().unwrap_or("").chars().take(120).collect(),
                            success: *success,
                        });
                    }
                    if !tool_defs.is_empty() {
                        let _ = self.ui_tx.send(Agent2Ui::ToolResults {
                            turn_id: turn_id.clone(),
                            round_num,
                            results: tool_defs,
                        });
                    }

                    if self.agent.msg.has_pending_tools() {
                        round_num += 1;
                        continue;
                    }
                }
                Effect::TurnComplete | Effect::None => {}
                _ => {}
            }

            self.agent.msg.snapshot(&self.agent.config.model, &self.agent.config.reasoning_effort);

            let _ = self.ui_tx.send(Agent2Ui::TurnEnd {
                turn_id: turn_id.clone(),
                stop_reason: None,
                usage: last_usage.clone(),
            });

            break;
        }

        let _ = self.ui_tx.send(Agent2Ui::Done);
    }

    // ── Dashboard ──

    fn emit_dashboard(&self) {
        let _ = self.ui_tx.send(Agent2Ui::Dashboard {
            hp_connected: true,
            session_seed: self.agent.session.seed.clone(),
            context_limit: self.agent.config.context_limit,
            tool_calls_total: 0,
            tool_failures: 0,
            current_phase: "single".into(),
            streaming: false,
            dsml_compat_count: self.agent.dsml_compat_count,
            documents: dsx_agent::runner::build_documents(&self.agent),
            recent_edits: dsx_agent::runner::build_recent_edits(&self.agent),
            tasks: dsx_agent::runner::build_tasks(&self.agent),
            session_title: self.agent.session.title.clone(),
            usage: None,
        });
    }
}

// ═══════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════

fn parse_tool_calls_from_response(
    content: &str, _reasoning: &str, tool_calls_raw: &serde_json::Value,
    agent: &AgentState,
) -> Vec<dsx_types::ToolCall> {
    let mut parsed = dsx_agent::tool_parser::parse_tool_calls(tool_calls_raw);
    if parsed.is_empty() {
        let stripped = dsx_agent::tool_parser::strip_fenced_code(content);
        if dsx_agent::tool_parser::has_dsml(&stripped) {
            let (_, dsml) = dsx_agent::tool_parser::parse_dsml_tool_calls(&stripped, &agent.tool_defs);
            if !dsml.is_empty() { parsed = dsml; }
        }
        if parsed.is_empty() && has_xml(content) {
            let names: Vec<String> = agent.tool_defs.iter().map(|t| t.function.name.clone()).collect();
            let stripped2 = dsx_agent::tool_parser::strip_fenced_code(content);
            let (_, xml) = dsx_agent::tool_parser::parse_xml_tool_calls(&stripped2, &names);
            if !xml.is_empty() { parsed = xml; }
        }
    }
    parsed
}

fn has_xml(s: &str) -> bool {
    s.contains("<tool_use>") || s.contains("<invoke ") || s.contains("<tool_calls>")
}

fn build_assistant_message(
    content: &str, reasoning: &str, parsed: &[dsx_types::ToolCall],
) -> dsx_types::Message {
    use dsx_types::{ContentBlock, Message};
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
    _agent: &AgentState, ui_tx: &mpsc::Sender<Agent2Ui>,
    turn_id: &str, round_num: u32, assistant_msg: &dsx_types::Message,
    content: &str, reasoning: &str, _parsed: &[dsx_types::ToolCall],
) {
    use dsx_types::ContentBlock;
    let mut blocks = Vec::new();
    let mut tool_calls = Vec::new();
    for cb in &assistant_msg.content {
        match cb {
            ContentBlock::Reasoning { reasoning } if !reasoning.is_empty() => {
                blocks.push(dsx_proto::RoundBlock::Reasoning { content: reasoning.clone() });
            }
            ContentBlock::Text { text } if !text.is_empty() => {
                blocks.push(dsx_proto::RoundBlock::Text { content: text.clone() });
            }
            ContentBlock::ToolUse { id, name, input } if name != "ask_user" => {
                tool_calls.push(dsx_proto::ToolCallDef {
                    id: id.clone(), name: name.clone(),
                    args_display: name.clone(), args_json: input.to_string(),
                });
                blocks.push(dsx_proto::RoundBlock::Tool {
                    card: dsx_proto::ToolCallDef {
                        id: id.clone(), name: name.clone(),
                        args_display: name.clone(), args_json: input.to_string(),
                    },
                });
            }
            _ => {}
        }
    }
    let _ = ui_tx.send(Agent2Ui::RoundComplete {
        turn_id: turn_id.into(),
        round_num,
        thinking: if reasoning.is_empty() { None } else { Some(reasoning.into()) },
        answer: if content.is_empty() { None } else { Some(content.into()) },
        tool_calls: tool_calls.clone(),
        blocks,
        is_final: tool_calls.is_empty(),
    });
}

fn build_turns_from_context(agent: &AgentState) -> Vec<dsx_proto::TurnData> {
    use dsx_types::ContentBlock;
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
            let tcs: Vec<dsx_proto::ToolCallDef> = step.assistant.content.iter().filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some(dsx_proto::ToolCallDef {
                        id: id.clone(), name: name.clone(),
                        args_display: name.clone(), args_json: input.to_string(),
                    })
                } else { None }
            }).collect();
            let trs: Vec<dsx_proto::ToolResultDef> = step.tool_results.iter().filter_map(|tr| {
                tr.content.iter().find_map(|b| {
                    if let ContentBlock::ToolResult { tool_use_id, content } = b {
                        Some(dsx_proto::ToolResultDef {
                            tool_call_id: tool_use_id.clone(),
                            output: content.clone(), success: true, file: None,
                        })
                    } else { None }
                })
            }).collect();
            rounds.push(dsx_proto::RoundData {
                round_num: ri as u32, thinking, answer, tool_calls: tcs, tool_results: trs,
            });
        }
        let user_text = turn.user.content.iter().find_map(|b| {
            if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
        }).unwrap_or_default();
        turns.push(dsx_proto::TurnData {
            turn_id: format!("t{}", ti + 1), user_text, rounds,
        });
    }
    turns
}
