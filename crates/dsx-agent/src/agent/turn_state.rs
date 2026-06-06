//! TurnState: per-turn mutable scratch state.
//!
//! Reset at the start of each turn. Annotations are collected during
//! tool execution and rendered into the system prompt tail by build_context().

#[derive(Debug, Clone)]
pub struct TurnState {
    /// Per-turn annotations collected during tool execution,
    /// rendered into the system prompt tail, then cleared after build_context().
    pub annotations: Vec<String>,

    /// Cumulative tool failures in the current turn.
    pub tool_failures: u32,

    /// Number of tool calls made in the current turn.
    pub tool_calls_this_turn: u32,

    /// Set by the UI to cancel the current streaming response.
    pub stream_cancelled: bool,
}

impl TurnState {
    pub fn new() -> Self {
        Self {
            annotations: Vec::new(),
            tool_failures: 0,
            tool_calls_this_turn: 0,
            stream_cancelled: false,
        }
    }

    /// Reset per-turn counters (called at start of each user input).
    pub fn reset(&mut self) {
        self.annotations.clear();
        self.tool_failures = 0;
        self.tool_calls_this_turn = 0;
        self.stream_cancelled = false;
    }

    /// Add a system note annotation.
    pub fn note(&mut self, tag: &str, msg: String) {
        self.annotations.push(format!("[{tag}] {msg}"));
    }
}

impl Default for TurnState {
    fn default() -> Self {
        Self::new()
    }
}
