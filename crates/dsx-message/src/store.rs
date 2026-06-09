use dsx_types::Message;
use crate::effect::{Effect, PendingTool, ToolExecRequest, ToolExecutorFn};
use dsx_session::SessionManager;
use dsx_types::SessionFile;
 

// Data model

/// One assistant response + its tool results.
#[derive(Debug, Clone)]
pub struct Step {
    pub assistant: Message,
    pub tool_results: Vec<Message>,
}

impl Step {
    fn new(assistant: Message) -> Self {
        Self { assistant, tool_results: Vec::new() }
    }

    fn assistant_tool_ids(&self) -> Vec<String> {
        self.assistant.content.iter()
            .filter_map(|b| {
                if let dsx_types::ContentBlock::ToolUse { id, .. } = b {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn tool_result_has_id(&self, id: &str) -> bool {
        self.tool_results.iter().any(|tr| {
            tr.content.iter().any(|b| {
                matches!(b, dsx_types::ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == id)
            })
        })
    }

    fn has_tool_call(&self, id: &str) -> bool {
        self.assistant_tool_ids().iter().any(|tid| tid == id)
    }

    fn all_tools_satisfied(&self) -> bool {
        let ids = self.assistant_tool_ids();
        if ids.is_empty() { return true; }
        ids.iter().all(|id| self.tool_result_has_id(id))
    }

    /// Extract PendingTool list from this step.
    fn pending_tools(&self) -> Vec<PendingTool> {
        self.assistant.content.iter()
            .filter_map(|b| {
                if let dsx_types::ContentBlock::ToolUse { id, name, input } = b {
                    Some(PendingTool {
                        id: id.clone(),
                        name: name.clone(),
                        args: input.clone(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

/// A single conversation turn: one user message + a chain of assistant steps.
#[derive(Debug, Clone)]
pub struct Turn {
    pub user: Message,
    pub steps: Vec<Step>,
}

impl Turn {
    fn new(user: Message) -> Self {
        Self { user, steps: Vec::new() }
    }

    fn find_step_for_mut(&mut self, tool_call_id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.has_tool_call(tool_call_id))
    }
}

// MessageStore

/// The canonical message container with state-machine lifecycle.
///
/// All message mutations go through this store. Every `push_*` returns
/// an [`Effect`] telling the caller what to do next.
///
/// Methods are infallible — inconsistencies are auto-repaired with a log
/// warning rather than rejected. The LLM API itself enforces message
/// alternation.
#[allow(clippy::type_complexity)]
pub struct MessageStore {
    /// Session identifier (shared with SessionManager).
    seed: String,
    /// System messages (KV-cached, not in regular turns).
    system_messages: Vec<Message>,
    /// Conversation turns.
    turns: Vec<Turn>,
    /// Index into to_vec() for incremental gate context.
    /// Reset to 0 on each new turn.
    gate_cursor: usize,
    /// Set by cancel() — push_* methods check this.
    cancelled: bool,
    /// Optional tool executor (injected by runner at startup).
    /// When set, `execute_tools_batch()` dispatches tools through this callback.
    #[allow(clippy::type_complexity)]
    tool_executor: Option<ToolExecutorFn>,
    /// UI sender for emitting Agent2Ui events (injected by msglp at startup).
    ui_tx: Option<std::sync::mpsc::Sender<dsx_proto::Agent2Ui>>,
    /// Shared cancel flag (injected by msglp at startup).
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}


impl std::fmt::Debug for MessageStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageStore")
            .field("seed", &self.seed)
            .field("turns", &self.turns.len())
            .field("gate_cursor", &self.gate_cursor)
            .field("cancelled", &self.cancelled)
            .field("has_executor", &self.tool_executor.is_some())
            .finish()
    }
}

// Manual Clone: skip tool_executor (not cloneable)
impl Clone for MessageStore {
    fn clone(&self) -> Self {
        Self {
            seed: self.seed.clone(),
            system_messages: self.system_messages.clone(),
            turns: self.turns.clone(),
            gate_cursor: self.gate_cursor,
            cancelled: self.cancelled,
            tool_executor: None,
            ui_tx: None,
            cancel_flag: None,
        }
    }
}

impl MessageStore {
    pub fn new(seed: &str) -> Self {
        Self {
            seed: seed.to_string(),
            system_messages: Vec::new(),
            turns: Vec::new(),
            gate_cursor: 0,
            cancelled: false,
            tool_executor: None,
            ui_tx: None,
            cancel_flag: None,
        }
    }

    // Seed management

    pub fn seed(&self) -> &str {
        &self.seed
    }

    /// Inject the tool executor callback.
    /// Inject the UI sender — MessageStore will emit Agent2Ui events directly.
    pub fn set_ui_tx(&mut self, tx: std::sync::mpsc::Sender<dsx_proto::Agent2Ui>) {
        self.ui_tx = Some(tx);
    }

    /// Inject a shared cancel flag.
    pub fn set_cancel(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.cancel_flag = Some(flag);
    }

    /// Check if cancellation has been requested.
    fn is_cancelled_by_flag(&self) -> bool {
        self.cancelled || self.cancel_flag.as_ref().map_or(false, |f| f.load(std::sync::atomic::Ordering::SeqCst))
    }

    pub fn set_tool_executor(&mut self, executor: ToolExecutorFn) {
        pub fn set_tool_executor(&mut self, executor: ToolExecutorFn) {
        self.tool_executor = Some(executor);
    }

    /// Switch to a new session: reset all state, adopt new seed.
    pub fn switch_seed(&mut self, new_seed: &str) {
        self.seed = new_seed.to_string();
        self.system_messages.clear();
        self.turns.clear();
        self.gate_cursor = 0;
        self.cancelled = false;
    }

    // Cancel

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    // System messages

    pub fn system_messages(&self) -> &[Message] {
        &self.system_messages
    }

    pub fn push_system(&mut self, msg: Message) -> Effect {
        debug_assert_eq!(msg.role, "system", "push_system requires role=system");
        self.system_messages.push(msg);
        Effect::None
    }

    // User messages

    /// Push a user message. Returns Effect::CallGate with full context.
    pub fn push_user(&mut self, text: &str) -> Effect {
        // Auto-complete unfulfilled tools in previous step
        if let Some(turn) = self.turns.last_mut() {
            if let Some(step) = turn.steps.last_mut() {
                auto_complete_unfulfilled(step, "[CANCELLED] Tool was not executed (user interrupted).");
            }
        }

        self.gate_cursor = 0;
        self.turns.push(Turn::new(Message::user(text)));
        Effect::None // caller must call build_context_for_gate to get messages
    }

    // Assistant messages

    /// Push an assistant response. Returns:
    /// - ExecTools if tool_use blocks are present
    /// - CallGate if no tools (continue turn, e.g. stop_reason=length)
    /// - TurnComplete if no tools and this is a terminal response
    pub fn push_assistant(&mut self, msg: Message) -> Effect {
        debug_assert_eq!(msg.role, "assistant", "push_assistant requires role=assistant");

        if self.turns.is_empty() {
            log::warn!("push_assistant: no turn exists, auto-creating empty user turn");
            self.turns.push(Turn::new(Message::user("")));
        }

        let turn = self.turns.last_mut().expect("turns non-empty after guarantee");

        // Auto-complete any unfulfilled tools in the current step
        if let Some(step) = turn.steps.last_mut() {
            auto_complete_unfulfilled(step, "[AUTO] Tool was not executed before next assistant response.");
        }

        turn.steps.push(Step::new(msg));

        let step = turn.steps.last().expect("step just pushed");
        if step.all_tools_satisfied() {
            let tools = step.pending_tools();
            if tools.is_empty() {
                Effect::TurnComplete
            } else {
                Effect::NeedTools
            }
        } else {
            // Shouldn"t happen normally, but handle gracefully
            Effect::None
        }
    }

    // Tool results

    /// Push a single tool result (called by ToolManager callback).
    /// Returns CallGate when all tools in the current step are satisfied.
    pub fn push_tool_result(&mut self, tool_call_id: &str, result: &str) -> Effect {
        self.push_tool_result_inner(tool_call_id, result);

        // Check if current step is now fully satisfied
        if let Some(turn) = self.turns.last() {
            if let Some(step) = turn.steps.last() {
                if step.all_tools_satisfied() {
                    // Determine if we need to continue (more rounds needed)
                    // Currently: after all tools done, go to CallGate for next round
                    if step.pending_tools().is_empty() {
                        return Effect::TurnComplete;
                    }
                    // There were tools and they're all done -> CallGate for next round
                    return Effect::None; // caller builds context manually
                }
            }
        }
        Effect::None
    }

    /// Push a batch of tool results.
    pub fn push_tool_results(&mut self, results: &[(String, String)]) -> Effect {
        for (tc_id, result) in results {
            self.push_tool_result_inner(tc_id, result);
        }
        // After batch, check if all satisfied
        if let Some(turn) = self.turns.last() {
            if let Some(step) = turn.steps.last() {
                if step.all_tools_satisfied() {
                    return Effect::None; // caller re-evaluates
                }
            }
        }
        Effect::None
    }

    fn push_tool_result_inner(&mut self, tool_call_id: &str, result: &str) {
        // Search all turns (most recent first)
        for turn in self.turns.iter_mut().rev() {
            if let Some(step) = turn.find_step_for_mut(tool_call_id) {
                if !step.tool_result_has_id(tool_call_id) {
                    step.tool_results.push(Message::tool(tool_call_id, result));
                }
                return;
            }
        }
        // Fallback: append to last step if possible
        if let Some(turn) = self.turns.last_mut() {
            if let Some(step) = turn.steps.last_mut() {
                log::warn!("push_tool_result: orphan tool_result {} — appending to last step", tool_call_id);
                step.tool_results.push(Message::tool(tool_call_id, result));
                return;
            }
        }
        log::error!("push_tool_result: orphan tool_result {} — nowhere to place, dropped", tool_call_id);
    }

    /// Replace an existing tool result"s content (for interrupt replies like ask_user).
    pub fn replace_tool_result(&mut self, tool_call_id: &str, result: &str) {
        for turn in self.turns.iter_mut().rev() {
            if let Some(step) = turn.find_step_for_mut(tool_call_id) {
                step.tool_results.retain(|tr| {
                    !tr.content.iter().any(|b| {
                        matches!(b, dsx_types::ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == tool_call_id)
                    })
                });
                step.tool_results.push(Message::tool(tool_call_id, result));
                return;
            }
        }
        log::error!("replace_tool_result: tool_call_id {} not found in any turn", tool_call_id);
    }

    // Context building for gate

    /// Build context for the next gate call (incremental).
    ///
    /// `system_prompt` is injected from outside (config).
    /// `annotations` are appended to the last user message.
    ///
    /// On the first call of a turn, returns full context.
    /// Subsequent calls return only new messages since last call.
    pub fn build_context_for_gate(
        &mut self,
        system_prompt: &str,
        annotations: &[String],
    ) -> Vec<Message> {
        let mut full: Vec<Message> = {
            let mut v = vec![Message::system(system_prompt)];
            v.extend(self.system_messages.clone());
            for turn in &self.turns {
                v.push(turn.user.clone());
                for step in &turn.steps {
                    v.push(step.assistant.clone());
                    v.extend(step.tool_results.clone());
                }
            }
            v
        };

        // Inject annotations into last user message
        if !annotations.is_empty() {
            let ann_text = annotations.join("
");
            if let Some(last_user) = full.iter_mut().rev().find(|m| m.role == "user") {
                let existing = last_user.content.iter_mut().find_map(|b| {
                    if let dsx_types::ContentBlock::Text { ref mut text } = b {
                        Some(text)
                    } else {
                        None
                    }
                });
                if let Some(text) = existing {
                    text.push_str("

## Notes
");
                    text.push_str(&ann_text);
                } else {
                    last_user.content.push(dsx_types::ContentBlock::text(&ann_text));
                }
            }
        }

        // Incremental: only return messages after gate_cursor
        let result = if self.gate_cursor == 0 {
            self.gate_cursor = full.len();
            full
        } else if self.gate_cursor < full.len() {
            let new_msgs: Vec<Message> = full[self.gate_cursor..].to_vec();
            self.gate_cursor = full.len();
            // For incremental calls, prepend a minimal system message
            let mut out = vec![Message::system("[continue]")];
            out.extend(new_msgs);
            out
        } else {
            Vec::new()
        };

        result
    }

    // ── Tool execution ──

    /// Execute all pending tools in the current step.
    ///
    /// Dispatches every unsatisfied tool to the injected executor,
    /// collects results, then atomically writes them back.
    /// Returns CallGate when all tools are satisfied.
    pub fn execute_tools_batch(&mut self) -> Effect {
        let executor = match &self.tool_executor {
            Some(e) => e,
            None => {
                log::error!("execute_tools_batch: no tool executor set — tools will not be executed");
                // Auto-complete with errors so the turn can continue
                if let Some(turn) = self.turns.last_mut() {
                    if let Some(step) = turn.steps.last_mut() {
                        let tool_ids = step.assistant_tool_ids();
                        for id in &tool_ids {
                            if !step.tool_result_has_id(id) {
                                step.tool_results.push(Message::tool(id, "[ERROR] No tool executor available."));
                            }
                        }
                    }
                }
                return Effect::TurnComplete;
            }
        };

        // Collect all unsatisfied tools from the current step
        let pending: Vec<PendingTool> = {
            let step = match self.turns.last().and_then(|t| t.steps.last()) {
                Some(s) => s,
                None => return Effect::None,
            };
            let tool_ids = step.assistant_tool_ids();
            step.pending_tools()
                .into_iter()
                .filter(|t| tool_ids.contains(&t.id) && !step.tool_result_has_id(&t.id))
                .collect()
        };

        if pending.is_empty() {
            return Effect::None;
        }

        // Execute all tools, collect results first (avoid borrow conflict)
        let mut reports: Vec<(String, String)> = Vec::new();
        for tool in &pending {
            let req = ToolExecRequest {
                id: tool.id.clone(),
                name: tool.name.clone(),
                args: tool.args.clone(),
            };
            let report = executor(req);
            reports.push((tool.id.clone(), report.content));
        }
        // Atomically push all results
        for (tc_id, content) in reports {
            self.push_tool_result_inner(&tc_id, &content);
        }

        // After all results are stored, determine next state
        if let Some(turn) = self.turns.last() {
            if let Some(step) = turn.steps.last() {
                if step.all_tools_satisfied() {
                    if step.pending_tools().is_empty() {
                        return Effect::TurnComplete;
                    }
                    return Effect::None; // caller re-evaluates
                }
            }
        }
        Effect::None
    }

    // ── Tool result inspection ──

    /// Get the results of the last step"s tool executions.
    /// Returns (tool_call_id, tool_name, content, success).
    pub fn last_step_tool_results(&self) -> Vec<(String, String, String, bool)> {
        let step = match self.turns.last().and_then(|t| t.steps.last()) {
            Some(s) => s,
            None => return Vec::new(),
        };
        let mut results = Vec::new();
        for tr in &step.tool_results {
            if let Some(tb) = tr.content.iter().find_map(|b| {
                if let dsx_types::ContentBlock::ToolResult { tool_use_id, content } = b {
                    Some((tool_use_id.clone(), content.clone()))
                } else { None }
            }) {
                let tool_name = step.assistant.content.iter().find_map(|b| {
                    if let dsx_types::ContentBlock::ToolUse { id, name, .. } = b {
                        if id == &tb.0 { Some(name.clone()) } else { None }
                    } else { None }
                }).unwrap_or_default();
                let success = !tb.1.starts_with("[ERROR]") && !tb.1.starts_with("[FAIL]");
                results.push((tb.0, tool_name, tb.1, success));
            }
        }
        results
    }

    /// Get the args for a specific tool call in the last step.
    pub fn tool_call_args(&self, tool_call_id: &str) -> Option<serde_json::Value> {
        let step = self.turns.last().and_then(|t| t.steps.last())?;
        step.assistant.content.iter().find_map(|b| {
            if let dsx_types::ContentBlock::ToolUse { id, input, .. } = b {
                if id == tool_call_id { Some(input.clone()) } else { None }
            } else { None }
        })
    }


    // ── Queries ──

    /// True if the last step has unsatisfied tool calls.
    pub fn has_pending_tools(&self) -> bool {
        self.turns.last()
            .and_then(|t| t.steps.last())
            .map(|s| !s.all_tools_satisfied())
            .unwrap_or(false)
    }

    /// Number of turns in the conversation.
    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    /// Total message count (system + user + assistant + tool_results).
    pub fn message_count(&self) -> usize {
        self.system_messages.len()
            + self.turns.iter().map(|t| 1 + t.steps.iter().map(|s| 1 + s.tool_results.len()).sum::<usize>()).sum::<usize>()
    }

    /// Read-only access to all turns (for session restore rendering).
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    // Serialization

    /// Flatten all messages into a Vec (system + user + assistant + tool_results interleaved).
    pub fn to_vec(&self) -> Vec<Message> {
        let mut v: Vec<Message> = self.system_messages.clone();
        for turn in &self.turns {
            v.push(turn.user.clone());
            for step in &turn.steps {
                v.push(step.assistant.clone());
                v.extend(step.tool_results.clone());
            }
        }
        v
    }

    // Snapshot

    /// Save current state to SessionManager.
    pub fn snapshot(&self, model: &str, effort: &str) {
        let msgs = self.to_vec();
        if msgs.len() > 1 && !self.seed.is_empty() {
            SessionManager::global().save(
                &self.seed,
                &msgs,
                model,
                Some(effort),
            );
        }
    }

    // Import from session file

    /// Restore from a SessionFile (disk format).
    /// Returns the store + list of repair actions taken.
    pub fn from_session(session_file: &SessionFile) -> (Self, Vec<String>) {
        let mut store = Self::new(&session_file.seed);
        let msgs = &session_file.messages;
        let mut repairs = Vec::new();
        let mut i = 0;

        while i < msgs.len() && msgs[i].role == "system" {
            store.system_messages.push(msgs[i].clone());
            i += 1;
        }

        while i < msgs.len() {
            match msgs[i].role.as_str() {
                "user" => {
                    let text = msgs[i].content.iter().find_map(|b| {
                        if let dsx_types::ContentBlock::Text { text } = b {
                            Some(text.clone())
                        } else { None }
                    }).unwrap_or_default();
                    store.push_user(&text);
                    i += 1;
                }
                "assistant" => {
                    store.push_assistant(msgs[i].clone());
                    i += 1;
                }
                "tool" => {
                    let (tc_id, result) = msgs[i].content.iter().find_map(|b| {
                        if let dsx_types::ContentBlock::ToolResult { tool_use_id, content, .. } = b {
                            Some((tool_use_id.clone(), content.clone()))
                        } else { None }
                    }).unwrap_or_default();
                    store.push_tool_result(&tc_id, &result);
                    i += 1;
                }
                _ => { i += 1; }
            }
        }

        // Repair: inject neutral [RESTORE] note for orphan tool_uses
        for turn in store.turns.iter_mut() {
            for step in turn.steps.iter_mut() {
                let missing_ids: Vec<(String, String)> = {
                    let tool_ids = step.assistant_tool_ids();
                    tool_ids.iter()
                        .filter(|id| !step.tool_result_has_id(id))
                        .map(|id| {
                            let name = step.assistant.content.iter().find_map(|b| {
                                if let dsx_types::ContentBlock::ToolUse { id: tid, name, .. } = b {
                                    if tid == id { Some(name.clone()) } else { None }
                                } else { None }
                            }).unwrap_or_default();
                            (id.clone(), name)
                        })
                        .collect()
                };
                if missing_ids.is_empty() { continue; }
                for (id, name) in missing_ids {
                    let note = format!(
                        "[RESTORE] Tool \"{name}\" had no result when session was saved — not executed.\n[HINT] Do NOT retry."
                    );
                    step.tool_results.push(Message::tool(&id, &note));
                    repairs.push(format!("injected [RESTORE] for orphan tool_use {}", id));
                }
            }
        }

        (store, repairs)
    }

    // Stream cancel recovery

    /// If the last step has unsatisfied tools, remove it.
    /// Used when the user cancels streaming during a tool_use response.
    pub fn remove_last_step_if_incomplete(&mut self) -> bool {
        if let Some(turn) = self.turns.last_mut() {
            if let Some(step) = turn.steps.last() {
                if !step.all_tools_satisfied() {
                    turn.steps.pop();
                    return true;
                }
            }
        }
        false
    }
}

// Shared helper: auto-complete unfulfilled tool calls in a step.
fn auto_complete_unfulfilled(step: &mut Step, reason: &str) {
    let missing: Vec<(String, String)> = {
        let tool_ids = step.assistant_tool_ids();
        tool_ids.iter()
            .filter(|id| !step.tool_result_has_id(id))
            .map(|id| {
                let name = step.assistant.content.iter().find_map(|b| {
                    if let dsx_types::ContentBlock::ToolUse { id: tid, name, .. } = b {
                        if tid == id { Some(name.clone()) } else { None }
                    } else { None }
                }).unwrap_or_default();
                (id.clone(), name)
            })
            .collect()
    };
    if !missing.is_empty() {
        log::warn!("auto-complete: {} unfulfilled tool(s) — {}", missing.len(), reason);
        for (id, name) in missing {
            step.tool_results.push(Message::tool(&id, &format!(
                "{} Tool \"{name}\" was not executed.", reason
            )));
        }
    }
}
