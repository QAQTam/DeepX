use deepx_types::Message;
use crate::effect::{Effect, PendingTool, ToolExecRequest, ToolExecutorFn};
use deepx_session::SessionManager;
use deepx_types::SessionFile;

/// Truncate tool result for LLM context. Tools return full output for the user,
/// but long results are trimmed here before storage to keep KV-cache prefixes
/// stable across turns.
fn truncate_tool_result(tool_name: &str, result: &str) -> String {
    let limit = match tool_name {
        "read_file" => 6000,
        "web_fetch" => 8000,
        "exec" => 5000,
        "search" | "grep" => 4000,
        _ => return result.to_string(),
    };
    if result.len() <= limit {
        return result.to_string();
    }
    let cut = result.floor_char_boundary(limit);
    let mut out = result[..cut].to_string();
    out.push_str(&format!("\n... [truncated: {} total chars]", result.len()));
    out
}

#[derive(Debug, Clone)]
pub struct Step {
    pub assistant: Message,
    pub tool_results: Vec<Message>,
}

impl Step {
    pub fn new(assistant: Message) -> Self {
        Self { assistant, tool_results: Vec::new() }
    }

    pub fn assistant_tool_ids(&self) -> Vec<String> {
        self.assistant.content.iter()
            .filter_map(|b| {
                if let deepx_types::ContentBlock::ToolUse { id, .. } = b {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn tool_result_has_id(&self, id: &str) -> bool {
        self.tool_results.iter().any(|tr| {
            tr.content.iter().any(|b| {
                matches!(b, deepx_types::ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == id)
            })
        })
    }

    pub fn has_tool_call(&self, id: &str) -> bool {
        self.assistant_tool_ids().iter().any(|tid| tid == id)
    }

    pub fn all_tools_satisfied(&self) -> bool {
        let ids = self.assistant_tool_ids();
        if ids.is_empty() { return true; }
        ids.iter().all(|id| self.tool_result_has_id(id))
    }

    pub fn pending_tools(&self) -> Vec<PendingTool> {
        self.assistant.content.iter()
            .filter_map(|b| {
                if let deepx_types::ContentBlock::ToolUse { id, name, input } = b {
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

    fn has_tool_use(&self) -> bool {
        self.assistant.content.iter().any(|b| matches!(b, deepx_types::ContentBlock::ToolUse { .. }))
    }
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub user: Message,
    pub steps: Vec<Step>,
}

impl Turn {
    pub fn new(user: Message) -> Self {
        Self { user, steps: Vec::new() }
    }

    pub fn find_step_for_mut(&mut self, tool_call_id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.has_tool_call(tool_call_id))
    }
}

#[allow(clippy::type_complexity)]
pub struct MessageStore {
    seed: String,
    system_messages: Vec<Message>,
    turns: Vec<Turn>,
    cancelled: bool,
    tool_executor: Option<ToolExecutorFn>,
    /// Number of earliest turns that have been compacted (skipped in LLM context).
    compact_skip: usize,
}

impl std::fmt::Debug for MessageStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageStore")
            .field("seed", &self.seed)
            .field("turns", &self.turns.len())
            .field("cancelled", &self.cancelled)
            .field("has_executor", &self.tool_executor.is_some())
            .finish()
    }
}

impl Clone for MessageStore {
    fn clone(&self) -> Self {
        Self {
            seed: self.seed.clone(),
            system_messages: self.system_messages.clone(),
            turns: self.turns.clone(),
            cancelled: self.cancelled,
            tool_executor: None,
            compact_skip: self.compact_skip,
        }
    }
}

impl MessageStore {
    pub fn new(seed: &str) -> Self {
        Self {
            seed: seed.to_string(),
            system_messages: Vec::new(),
            turns: Vec::new(),
            cancelled: false,
            tool_executor: None,
            compact_skip: 0,
        }
    }

    pub fn seed(&self) -> &str {
        &self.seed
    }

    pub fn switch_seed(&mut self, new_seed: &str) {
        self.seed = new_seed.to_string();
        self.system_messages.clear();
        self.turns.clear();
        self.cancelled = false;
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn push_system(&mut self, msg: Message) -> Effect {
        debug_assert_eq!(msg.role, "system", "push_system requires role=system");
        self.system_messages.push(msg);
        Effect::None
    }

    pub fn push_user(&mut self, text: &str) -> Effect {
        if let Some(turn) = self.turns.last_mut() {
            if let Some(step) = turn.steps.last_mut() {
                auto_complete_unfulfilled(step, "[CANCELLED] Tool was not executed (user interrupted).");
            }
        }
        self.turns.push(Turn::new(Message::user(text)));
        Effect::None
    }

    pub fn push_assistant(&mut self, msg: Message) -> Effect {
        debug_assert_eq!(msg.role, "assistant", "push_assistant requires role=assistant");

        if self.turns.is_empty() {
            log::warn!("push_assistant: no turn exists, auto-creating empty user turn");
            self.turns.push(Turn::new(Message::user("")));
        }

        let turn = self.turns.last_mut().expect("turns non-empty after guarantee");

        if let Some(step) = turn.steps.last_mut() {
            auto_complete_unfulfilled(step, "[AUTO] Tool was not executed before next assistant response.");
        }

        let step = Step::new(msg);
        let has_tools = step.has_tool_use();
        turn.steps.push(step);

        if has_tools {
            self.execute_tools_batch();
            Effect::None
        } else {
            Effect::TurnComplete
        }
    }

    pub fn push_tool_result(&mut self, tool_call_id: &str, result: &str) -> Effect {
        self.push_tool_result_inner(tool_call_id, result);

        if let Some(turn) = self.turns.last() {
            if let Some(step) = turn.steps.last() {
                if step.all_tools_satisfied() {
                    return if step.pending_tools().is_empty() {
                        Effect::TurnComplete
                    } else {
                        Effect::None
                    };
                }
            }
        }
        Effect::None
    }

    pub fn push_tool_results_batch(&mut self, results: &[(String, String)]) -> Effect {
        for (tc_id, result) in results {
            self.push_tool_result_inner(tc_id, result);
        }

        if let Some(turn) = self.turns.last() {
            if let Some(step) = turn.steps.last() {
                if step.all_tools_satisfied() {
                    return if step.pending_tools().is_empty() {
                        Effect::TurnComplete
                    } else {
                        Effect::None
                    };
                }
            }
        }
        Effect::None
    }

    fn push_tool_result_inner(&mut self, tool_call_id: &str, result: &str) {
        // Look up tool name from any step that owns this tool_call_id.
        let tool_name: Option<String> = self.turns.iter().rev().find_map(|turn| {
            turn.steps.iter().rev().find_map(|step| {
                step.assistant.content.iter().find_map(|b| {
                    if let deepx_types::ContentBlock::ToolUse { id, name, .. } = b {
                        if id == tool_call_id { Some(name.clone()) } else { None }
                    } else { None }
                })
            })
        });
        let final_result = tool_name.as_deref()
            .map(|name| truncate_tool_result(name, result))
            .unwrap_or_else(|| result.to_string());

        for turn in self.turns.iter_mut().rev() {
            if let Some(step) = turn.find_step_for_mut(tool_call_id) {
                if !step.tool_result_has_id(tool_call_id) {
                    step.tool_results.push(Message::tool(tool_call_id, &final_result));
                }
                return;
            }
        }
        if let Some(turn) = self.turns.last_mut() {
            if let Some(step) = turn.steps.last_mut() {
                log::warn!("push_tool_result: orphan tool_result {} — appending to last step", tool_call_id);
                step.tool_results.push(Message::tool(tool_call_id, &final_result));
                return;
            }
        }
        log::error!("push_tool_result: orphan tool_result {} — nowhere to place, dropped", tool_call_id);
    }

    pub fn replace_tool_result(&mut self, tool_call_id: &str, result: &str) {
        // Same truncation for replace path.
        let tool_name: Option<String> = self.turns.iter().rev().find_map(|turn| {
            turn.steps.iter().rev().find_map(|step| {
                step.assistant.content.iter().find_map(|b| {
                    if let deepx_types::ContentBlock::ToolUse { id, name, .. } = b {
                        if id == tool_call_id { Some(name.clone()) } else { None }
                    } else { None }
                })
            })
        });
        let final_result = tool_name.as_deref()
            .map(|name| truncate_tool_result(name, result))
            .unwrap_or_else(|| result.to_string());

        for turn in self.turns.iter_mut().rev() {
            if let Some(step) = turn.find_step_for_mut(tool_call_id) {
                step.tool_results.retain(|tr| {
                    !tr.content.iter().any(|b| {
                        matches!(b, deepx_types::ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == tool_call_id)
                    })
                });
                step.tool_results.push(Message::tool(tool_call_id, &final_result));
                return;
            }
        }
        log::error!("replace_tool_result: tool_call_id {} not found in any turn", tool_call_id);
    }

    pub fn build_context_for_gate(
        &mut self,
        system_prompt: &str,
        annotations: &[String],
    ) -> Vec<Message> {
        let mut full: Vec<Message> = {
            let mut v = Vec::new();
            if !system_prompt.is_empty() {
                v.push(Message::system(system_prompt));
            }
            v.extend(self.system_messages.clone());
            for (i, turn) in self.turns.iter().enumerate() {
                if i < self.compact_skip { continue; }
                v.push(turn.user.clone());
                for step in &turn.steps {
                    v.push(step.assistant.clone());
                    v.extend(step.tool_results.clone());
                }
            }
            v
        };

        if !annotations.is_empty() {
            let ann_text = annotations.join("\n");
            if let Some(last_user) = full.iter_mut().rev().find(|m| m.role == "user") {
                let existing = last_user.content.iter_mut().find_map(|b| {
                    if let deepx_types::ContentBlock::Text { ref mut text } = b {
                        Some(text)
                    } else {
                        None
                    }
                });
                if let Some(text) = existing {
                    text.push_str("\n\n## Notes\n");
                    text.push_str(&ann_text);
                } else {
                    last_user.content.push(deepx_types::ContentBlock::text(&ann_text));
                }
            }
        }

        full
    }

    /// Execute all pending tools in the current step. When `tool_executor` is None
    /// (e.g. during session restore), returns early without injecting errors.
    pub fn execute_tools_batch(&mut self) -> Effect {
        let executor = match &self.tool_executor {
            Some(e) => e,
            None => {
                log::warn!("execute_tools_batch: no tool executor set — skipping tool execution");
                return Effect::None;
            }
        };

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
        for (tc_id, content) in reports {
            self.push_tool_result_inner(&tc_id, &content);
        }

        // Tools executed; caller re-evaluates (build context → gate → push_assistant)
        Effect::None
    }

    pub fn last_step_tool_results(&self) -> Vec<(String, String, String, bool)> {
        let step = match self.turns.last().and_then(|t| t.steps.last()) {
            Some(s) => s,
            None => return Vec::new(),
        };
        let mut results = Vec::new();
        for tr in &step.tool_results {
            if let Some(tb) = tr.content.iter().find_map(|b| {
                if let deepx_types::ContentBlock::ToolResult { tool_use_id, content } = b {
                    Some((tool_use_id.clone(), content.clone()))
                } else { None }
            }) {
                let tool_name = step.assistant.content.iter().find_map(|b| {
                    if let deepx_types::ContentBlock::ToolUse { id, name, .. } = b {
                        if id == &tb.0 { Some(name.clone()) } else { None }
                    } else { None }
                }).unwrap_or_default();
                let success = !tb.1.starts_with("[ERROR]") && !tb.1.starts_with("[FAIL]");
                results.push((tb.0, tool_name, tb.1, success));
            }
        }
        results
    }

    pub fn tool_call_args(&self, tool_call_id: &str) -> Option<serde_json::Value> {
        let step = self.turns.last().and_then(|t| t.steps.last())?;
        step.assistant.content.iter().find_map(|b| {
            if let deepx_types::ContentBlock::ToolUse { id, input, .. } = b {
                if id == tool_call_id { Some(input.clone()) } else { None }
            } else { None }
        })
    }

    pub fn has_pending_tools(&self) -> bool {
        self.turns.last()
            .and_then(|t| t.steps.last())
            .map(|s| !s.all_tools_satisfied())
            .unwrap_or(false)
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    pub fn message_count(&self) -> usize {
        self.system_messages.len()
            + self.turns.iter().map(|t| 1 + t.steps.iter().map(|s| 1 + s.tool_results.len()).sum::<usize>()).sum::<usize>()
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn system_messages(&self) -> &[Message] {
        &self.system_messages
    }

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

    pub fn set_tool_executor(&mut self, executor: ToolExecutorFn) {
        self.tool_executor = Some(executor);
    }

    pub fn snapshot(&self, model: &str, effort: &str) {
        let msgs = self.to_vec();
        if !self.seed.is_empty() {
            SessionManager::global().save(&self.seed, &msgs, model, Some(effort));
        }
    }

    /// Reconstruct the internal turn/step structure by replaying saved messages
    /// through `push_user` / `push_assistant` / `push_tool_result`.
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
                        if let deepx_types::ContentBlock::Text { text } = b {
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
                        if let deepx_types::ContentBlock::ToolResult { tool_use_id, content, .. } = b {
                            Some((tool_use_id.clone(), content.clone()))
                        } else { None }
                    }).unwrap_or_default();
                    store.push_tool_result(&tc_id, &result);
                    i += 1;
                }
                _ => { i += 1; }
            }
        }

        for turn in store.turns.iter_mut() {
            for step in turn.steps.iter_mut() {
                let missing_ids: Vec<(String, String)> = {
                    let tool_ids = step.assistant_tool_ids();
                    tool_ids.iter()
                        .filter(|id| !step.tool_result_has_id(id))
                        .map(|id| {
                            let name = step.assistant.content.iter().find_map(|b| {
                                if let deepx_types::ContentBlock::ToolUse { id: tid, name, .. } = b {
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

    pub fn truncate_before_turn(&mut self, turn_id: &str) -> bool {
        let idx: usize = match turn_id.strip_prefix('t').and_then(|n| n.parse::<usize>().ok()) {
            Some(n) if n > 0 => n.saturating_sub(1),
            _ => return false,
        };
        if idx >= self.turns.len() { return false; }
        self.turns.truncate(idx);
        true
    }

    /// Compact: keep `keep` recent turns in LLM context, skip older ones.
    /// Replaces any prior compact messages with a single consolidated summary.
    pub fn apply_compact(&mut self, summary: &str, keep: usize) {
        let skip = self.turns.len().saturating_sub(keep);
        if skip == 0 { return; }
        self.compact_skip = skip;
        self.system_messages.retain(|m| {
            !m.content.iter().any(|b| matches!(b, deepx_types::ContentBlock::Text { text } if text.starts_with("[COMPACT")))
        });
        self.system_messages.push(Message::system(
            &format!("[COMPACT {} turns] Summary of earlier conversation:\n{summary}", skip)
        ));
    }
}

fn auto_complete_unfulfilled(step: &mut Step, reason: &str) {
    let missing: Vec<(String, String)> = {
        let tool_ids = step.assistant_tool_ids();
        tool_ids.iter()
            .filter(|id| !step.tool_result_has_id(id))
            .map(|id| {
                let name = step.assistant.content.iter().find_map(|b| {
                    if let deepx_types::ContentBlock::ToolUse { id: tid, name, .. } = b {
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
