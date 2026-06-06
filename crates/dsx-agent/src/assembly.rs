use dsx_types::Message;

// ── Data model ──

/// One assistant response + its tool results.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub(crate) assistant: Message,
    pub(crate) tool_results: Vec<Message>,
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
}

/// A single conversation turn: one user message + a chain of assistant steps.
#[derive(Debug, Clone)]
pub struct Turn {
    pub(crate) user: Message,
    pub(crate) steps: Vec<Step>,
}

impl Turn {
    fn new(user: Message) -> Self {
        Self { user, steps: Vec::new() }
    }

    fn find_step_for_mut(&mut self, tool_call_id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.has_tool_call(tool_call_id))
    }
}

// ── ContextAssembler ──

/// The canonical message container.
///
/// All message mutations go through this assembler. It is the single source of
/// truth for message state and the only path to produce context for the API.
///
/// Methods are infallible — inconsistencies are auto-repaired with a log warning
/// rather than rejected. The LLM API itself enforces message alternation.
#[derive(Debug, Clone)]
pub struct ContextAssembler {
    system_messages: Vec<Message>,
    turns: Vec<Turn>,
}

impl ContextAssembler {
    pub fn new() -> Self {
        Self { system_messages: Vec::new(), turns: Vec::new() }
    }

    // ── System messages ──

    pub fn system_messages(&self) -> &[Message] {
        &self.system_messages
    }

    pub fn push_system(&mut self, msg: Message) {
        debug_assert_eq!(msg.role, "system", "push_system requires role=system");
        self.system_messages.push(msg);
    }

    // ── User messages ──

    /// Push a user message. If the previous step has unfulfilled tool calls,
    /// auto-complete them with a note (cancel recovery).
    pub fn push_user(&mut self, text: &str) {
        if let Some(turn) = self.turns.last_mut() {
            if let Some(step) = turn.steps.last_mut() {
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
                    log::warn!("push_user: auto-completing {} unfulfilled tool(s) in previous step", missing.len());
                    for (id, name) in missing {
                        step.tool_results.push(Message::tool(&id, &format!(
                            "[CANCELLED] Tool '{}' was not executed (user interrupted).", name
                        )));
                    }
                }
            }
        }
        self.turns.push(Turn::new(Message::user(text)));
    }

    // ── Assistant messages ──

    /// Push an assistant response. Creates a new Step in the current turn.
    /// If no turn exists, auto-creates one with an empty user message (restore path).
    /// If previous step has unfulfilled tools, auto-complete them.
    pub fn push_assistant(&mut self, msg: Message) {
        debug_assert_eq!(msg.role, "assistant", "push_assistant requires role=assistant");

        if self.turns.is_empty() {
            log::warn!("push_assistant: no turn exists, auto-creating empty user turn");
            self.turns.push(Turn::new(Message::user("")));
        }

        let turn = self.turns.last_mut().expect("turns non-empty after guarantee");

        // Auto-complete any unfulfilled tools in the current step
        if let Some(step) = turn.steps.last_mut() {
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
                log::warn!("push_assistant: auto-completing {} unfulfilled tool(s) from previous step", missing.len());
                for (id, name) in missing {
                    step.tool_results.push(Message::tool(&id, &format!(
                        "[AUTO] Tool '{}' was not executed before next assistant response.", name
                    )));
                }
            }
        }

        turn.steps.push(Step::new(msg));
    }

    // ── Tool results ──

    /// Push a tool result, searching across all turns for the matching tool_call_id.
    /// If no matching tool use is found, logs a warning and appends to the last step.
    pub fn push_tool_result(&mut self, tool_call_id: &str, result: &str) {
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

    /// Replace an existing tool result's content (for interrupt replies like ask_user).
    pub fn replace_tool_result(&mut self, tool_call_id: &str, result: &str) {
        for turn in self.turns.iter_mut().rev() {
            if let Some(step) = turn.find_step_for_mut(tool_call_id) {
                // Remove old result(s) for this tool_call_id, then push new one
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

    // ── Serialization ──

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

    /// Return conversation messages (user/assistant/tool), system messages stripped.
    pub fn build(&self) -> Vec<Message> {
        let mut msgs = self.to_vec();
        msgs.retain(|m| m.role != "system");
        msgs
    }

    // ── Import from legacy Vec<Message> (session restore) ──

    pub fn from_legacy(msgs: Vec<Message>) -> (Self, Vec<String>) {
        let mut assembler = Self::new();
        let mut repairs = Vec::new();
        let mut i = 0;

        while i < msgs.len() && msgs[i].role == "system" {
            assembler.system_messages.push(msgs[i].clone());
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
                    assembler.push_user(&text);
                    i += 1;
                }
                "assistant" => {
                    assembler.push_assistant(msgs[i].clone());
                    i += 1;
                }
                "tool" => {
                    let (tc_id, result) = msgs[i].content.iter().find_map(|b| {
                        if let dsx_types::ContentBlock::ToolResult { tool_use_id, content, .. } = b {
                            Some((tool_use_id.clone(), content.clone()))
                        } else { None }
                    }).unwrap_or_default();
                    assembler.push_tool_result(&tc_id, &result);
                    i += 1;
                }
                _ => { i += 1; }
            }
        }

        // Repair: inject neutral [RESTORE] note for orphan tool_uses
        for turn in assembler.turns.iter_mut() {
            for step in turn.steps.iter_mut() {
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
                if missing.is_empty() { continue; }
                for (id, name) in missing {
                    let note = format!(
                        "[RESTORE] Tool '{}' had no result when session was saved — not executed.\n[HINT] Do NOT retry.",
                        name
                    );
                    step.tool_results.push(Message::tool(&id, &note));
                    repairs.push(format!("injected [RESTORE] for orphan tool_use {}", id));
                }
            }
        }

        (assembler, repairs)
    }

    // ── Stream cancel recovery ──

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
