use crate::agent::AgentState;
use crate::tokenizer;
use crate::tokenizer::TokenBreakdown;
use dsx_types::Message;

/// Errors the assembler can surface when invariants would be violated.
#[derive(Debug, Clone, PartialEq)]
pub enum AssemblerError {
    /// Attempted to push a user message while the current turn has incomplete steps.
    TurnIncomplete { missing: String },
    /// Attempted to push an assistant while no user message is pending.
    NoUserPending,
    /// Attempted to push a tool result referencing a tool_call_id not present
    /// in the current step's tool_calls.
    OrphanToolResult { tool_call_id: String },
    /// Attempted to push tool result but current step has no tool_calls.
    NoToolUseInStep,
}

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

    fn missing_tool_count(&self) -> usize {
        let ids = self.assistant_tool_ids();
        ids.iter().filter(|id| !self.tool_result_has_id(id)).count()
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
        Self {
            user,
            steps: Vec::new(),
        }
    }

    /// The current step (last in the chain).
    fn current_step(&self) -> Option<&Step> {
        self.steps.last()
    }

    fn current_step_mut(&mut self) -> Option<&mut Step> {
        self.steps.last_mut()
    }

    /// All steps complete? (every step's tool_uses satisfied)
    fn all_steps_satisfied(&self) -> bool {
        self.steps.iter().all(|s| s.all_tools_satisfied())
    }

    /// Total missing tool results across all incomplete steps.
    fn total_missing_tools(&self) -> usize {
        self.steps.iter().map(|s| s.missing_tool_count()).sum()
    }

    fn find_step_for_mut(&mut self, tool_call_id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.has_tool_call(tool_call_id))
    }
}

// ── ContextAssembler ──

/// The canonical message container.
///
/// All message mutations go through this assembler. It is the single source of
/// truth for message state and the only path to produce Anthropic-format output.
#[derive(Debug, Clone)]
pub struct ContextAssembler {
    system_messages: Vec<Message>,
    turns: Vec<Turn>,
}

impl ContextAssembler {
    // ── Construction ──

    pub fn new() -> Self {
        Self {
            system_messages: Vec::new(),
            turns: Vec::new(),
        }
    }

    // ── System messages ──

    pub fn push_system(&mut self, msg: Message) {
        assert_eq!(msg.role, "system", "push_system requires role=system");
        self.system_messages.push(msg);
        
    }

    // ── User messages ──

    /// Push a user message. Actively rejects if the current turn has unfulfilled tool calls.
    pub fn push_user(&mut self, text: &str) -> Result<(), AssemblerError> {
        if self.has_unfulfilled_tool_calls() {
            return Err(AssemblerError::TurnIncomplete {
                missing: format!(
                    "{} step(s) with missing tool results — cannot push user yet",
                    self.turns.last().map(|t| t.total_missing_tools()).unwrap_or(0)
                ),
            });
        }
        if let Some(last) = self.turns.last() {
            if last.steps.is_empty() {
                return Err(AssemblerError::TurnIncomplete {
                    missing: "current turn has no assistant response yet".into(),
                });
            }
        }
        self.turns.push(Turn::new(Message::user(text)));
        
        Ok(())
    }

    /// Push user unconditionally — RESTORE ONLY. Do not use in normal flow.
    /// Skipped checks: turn completeness, alternation.
    #[doc(hidden)]
    pub fn push_user_restore(&mut self, text: &str) {
        if let Some(last) = self.turns.last_mut() {
            if last.steps.is_empty() {
                let prefix = last.user.content.iter().find_map(|b| {
                    if let dsx_types::ContentBlock::Text { text } = b {
                        Some(text.clone())
                    } else {
                        None
                    }
                }).unwrap_or_default();
                last.user.content = vec![dsx_types::ContentBlock::text(&format!("{}\n\n{}", prefix, text))];
                
                return;
            }
        }
        self.turns.push(Turn::new(Message::user(text)));
        
    }

    // ── Assistant messages ──

    /// Push an assistant response. Creates a new Step in the current turn.
    /// Actively rejects if current turn has unfulfilled tool calls.
    pub fn push_assistant(&mut self, msg: Message) -> Result<(), AssemblerError> {
        assert_eq!(msg.role, "assistant");
        // Check before mutable borrow
        let has_unfulfilled = self.has_unfulfilled_tool_calls();
        let turn = self.turns.last_mut().ok_or(AssemblerError::NoUserPending)?;
        // Previous step must be satisfied before starting a new one
        if has_unfulfilled {
            if let Some(last_step) = turn.current_step() {
                return Err(AssemblerError::TurnIncomplete {
                    missing: format!(
                        "{} tool result(s) missing from previous step",
                        last_step.missing_tool_count()
                    ),
                });
            }
        }
        turn.steps.push(Step::new(msg));
        
        Ok(())
    }

    /// Push assistant without validation — RESTORE ONLY. Do not use in normal flow.
    /// Skipped checks: turn existence, alternation, pending tools.
    #[doc(hidden)]
    pub fn push_assistant_restore(&mut self, msg: Message) {
        if self.turns.is_empty() {
            self.turns.push(Turn::new(Message::user("")));
        }
        let turn = self.turns.last_mut().unwrap();
        turn.steps.push(Step::new(msg));
        
    }

    // ── Tool results ──

    /// Push a tool result to the CURRENT step. Validates tool_call_id exists.
    pub fn push_tool_result(&mut self, tool_call_id: &str, result: &str) -> Result<(), AssemblerError> {
        let turn = self.turns.last_mut()
            .ok_or(AssemblerError::OrphanToolResult { tool_call_id: tool_call_id.into() })?;
        let step = turn.current_step_mut()
            .ok_or(AssemblerError::NoToolUseInStep)?;

        if !step.has_tool_call(tool_call_id) {
            return Err(AssemblerError::OrphanToolResult { tool_call_id: tool_call_id.into() });
        }

        if !step.tool_result_has_id(tool_call_id) {
            step.tool_results.push(Message::tool(tool_call_id, result));
            
        }
        Ok(())
    }

    /// Push a tool result by searching all turns for the matching assistant.
    /// Used by async exec results that may arrive across turns.
    pub fn push_tool_result_for(&mut self, tool_call_id: &str, result: &str) -> Result<(), AssemblerError> {
        for turn in self.turns.iter_mut().rev() {
            if let Some(step) = turn.find_step_for_mut(tool_call_id) {
                if !step.tool_result_has_id(tool_call_id) {
                    step.tool_results.push(Message::tool(tool_call_id, result));
                    
                }
                return Ok(());
            }
        }
        Err(AssemblerError::OrphanToolResult { tool_call_id: tool_call_id.into() })
    }

    // ── Validation ──

    pub fn validate(&self) -> Result<(), String> {
        if self.turns.is_empty() && self.system_messages.is_empty() {
            return Ok(()); // empty is valid
        }
        for (i, turn) in self.turns.iter().enumerate() {
            if turn.steps.is_empty() && i < self.turns.len() - 1 {
                return Err(format!("Turn {}: no assistant steps (not last turn)", i));
            }
            for (j, step) in turn.steps.iter().enumerate() {
                if !step.all_tools_satisfied() && j < turn.steps.len() - 1 {
                    return Err(format!("Turn {} step {}: incomplete but not last step", i, j));
                }
                for tr in &step.tool_results {
                    let tid = tr.content.iter().find_map(|b| {
                        if let dsx_types::ContentBlock::ToolResult { tool_use_id, .. } = b {
                            Some(tool_use_id.as_str())
                        } else {
                            None
                        }
                    });
                    let Some(tid) = tid else { continue };
                    if !step.has_tool_call(tid) {
                        return Err(format!("Turn {} step {}: orphan tool_result {}", i, j, tid));
                    }
                }
            }
        }
        for (i, m) in self.system_messages.iter().enumerate() {
            if m.role != "system" {
                return Err(format!("System[{}]: expected role=system, got {}", i, m.role));
            }
        }
        Ok(())
    }

    // ── Flat view ──

    /// Collect all messages into a flat Vec for UI rendering and session persistence.
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

    pub fn message_count(&self) -> usize {
        self.to_vec().len()
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    // ── State queries ──

    /// Read-only access to all turns for compaction.
    /// Turn+Step structure is preserved — no flatten round-trip needed.
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// True if the current turn has any unfulfilled tool calls in any step.
    /// Used by push_user and push_assistant to actively reject invalid state transitions.
    pub fn has_unfulfilled_tool_calls(&self) -> bool {
        self.turns.last()
            .map(|t| !t.all_steps_satisfied())
            .unwrap_or(false)
    }

    /// Whether the last step has unsatisfied tool calls.
    pub fn has_pending_tools(&self) -> bool {
        self.turns.last()
            .and_then(|t| t.current_step())
            .map(|s| !s.all_tools_satisfied())
            .unwrap_or(false)
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
                        } else {
                            None
                        }
                    }).unwrap_or_default();
                    let _ = assembler.push_user(&text);
                    i += 1;
                }
                "assistant" => {
                    let msg = msgs[i].clone();
                    assembler.push_assistant_restore(msg);
                    i += 1;
                }
                "tool" => {
                    let (tc_id, result) = msgs[i].content.iter().find_map(|b| {
                        if let dsx_types::ContentBlock::ToolResult { tool_use_id, content, .. } = b {
                            Some((tool_use_id.clone(), content.clone()))
                        } else {
                            None
                        }
                    }).unwrap_or_default();
                    match assembler.push_tool_result_for(&tc_id, &result) {
                        Ok(()) => {}
                        Err(AssemblerError::OrphanToolResult { .. }) => {
                            repairs.push(format!("orphan tool_result {} deleted on import", tc_id));
                        }
                        Err(e) => {
                            repairs.push(format!("import error: {:?}", e));
                        }
                    }
                    i += 1;
                }
                _ => { i += 1; }
            }
        }

        // Repair: inject neutral [RESTORE] note for orphan tool_uses
        // (satisfies API tool_call/tool_result alternation without faking errors)
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
                                } else {
                                    None
                                }
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

    // ── Remove last incomplete step (for stream cancel recovery) ──

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

    // ── Build: conversation messages ──

    /// Return conversation messages (user/assistant/tool), system messages stripped.
    pub fn build(&self, _context_limit: u32) -> Vec<Message> {
        let mut msgs = self.to_vec();
        msgs.retain(|m| m.role != "system");
        msgs
    }
}

// ── build_context ──

/// Build context for the next API request.
///
/// # Cache-friendly design for DeepSeek KV cache
///
/// DeepSeek caches via exact-match prefix detection. To maximise reuse,
/// the system prompt contains ONLY stable content. Dynamic per-round
/// content (annotations) is appended to the LAST user message
/// so it sits at the very end of the combined prefix and never shifts
/// the offset of history messages.
///
/// ```
/// System prompt (stable — fully cached):
///   1. Base prompt          ← static
///   2. Tool definitions     ← stable per session
///   3. Phase prompt         ← stable per phase
///   Static reminder         ← stable
///
/// Messages (history cached, only tail misses):
///   Conversation history    ← stable prefix (cached)
///   Last user message      ← appended with dynamic annotations (uncached suffix)
/// ```
pub fn build_context(state: &mut AgentState) -> (String, Vec<Message>, TokenBreakdown) {

    let base_prompt = crate::config::system_prompt();
    let base_tokens = tokenizer::count_tokens(&base_prompt);

    // === System prompt: STABLE layers only (identical across consecutive requests) ===
    let mut system = base_prompt;

    // Layer 2: Phase prompt (stable per phase)
    let phase = crate::router::read_phase();
    let phase_tokens = if let Some(suffix) = crate::router::phase_prompt_suffix(phase) {
        system.push_str("\n\n");
        system.push_str(suffix);
        tokenizer::count_tokens(suffix)
    } else { 0 };

    // === Messages: conversation from ctx, dynamic content appended to LAST user message ===
    let mut messages = state.ctx.build(state.config.context_limit);

    // Turn annotations → appended to last user message (dynamic, per round)
    let mut dyn_suffix = String::new();
    if !state.turn_annotations.is_empty() {
        let ann = state.turn_annotations.join("\n");
        dyn_suffix.push_str("\n\n## Notes\n");
        dyn_suffix.push_str(&ann);
    }

    // Append dynamic suffix to the last user message in the copy (source ctx unchanged)
    if !dyn_suffix.is_empty() {
        if let Some(last_user) = messages.iter_mut().rev().find(|m| m.role == "user") {
            let existing = last_user.content.iter_mut().find_map(|b| {
                if let dsx_types::ContentBlock::Text { ref mut text } = b {
                    Some(text)
                } else {
                    None
                }
            });
            if let Some(text) = existing {
                text.push_str(&dyn_suffix);
            } else {
                last_user.content.push(dsx_types::ContentBlock::text(&dyn_suffix));
            }
        }
    }

    // Clear per-turn annotations for next round
    state.turn_annotations.clear();

    // === Token breakdown ===
    let mut bd = TokenBreakdown::default();
    bd.system = base_tokens + phase_tokens;
    bd.episodic = tokenizer::estimate_messages_tokens(&messages);
    bd.total = bd.system + bd.episodic;

    state.token_estimate = bd.total;
    state.health.context_tokens = state.tokens_used();
    state.health.context_tier = crate::health::ContextTier::from_tokens(
        state.health.context_tokens, state.config.context_limit,
    );

    // Predict KV cache hit rate
    let report = state.cache_analyzer.record(&system, &messages);
    state.predicted_cache_hit_pct = report.hit_rate;

    log::info!(
        "context (tokens): base={} phase={} messages={} total={}",
        base_tokens, phase_tokens, bd.episodic, bd.total,
    );

    (system, messages, bd)
}
