//! Agent health tracking: context pressure, turn counting, tool call stats.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTier {
    Premium,
    Healthy,
    Danger,
}

impl ContextTier {
    pub fn from_tokens(tokens: u32, limit: u32) -> Self {
        let pct = if limit > 0 {
            tokens as f64 / limit as f64
        } else {
            0.0
        };
        if pct > 0.80 || tokens >= 400_000 {
            ContextTier::Danger
        } else if pct > 0.30 || tokens >= 128_000 {
            ContextTier::Healthy
        } else {
            ContextTier::Premium
        }
    }
}

#[derive(Debug, Clone)]
pub struct DsAgentsHealthPlatform {
    pub tool_calls_this_turn: u32,
    pub context_tier: ContextTier,
    pub context_tokens: u32,
    pub context_limit: u32,
    pub turn: u32,
}

impl DsAgentsHealthPlatform {
    pub fn new() -> Self {
        DsAgentsHealthPlatform {
            tool_calls_this_turn: 0,
            context_tier: ContextTier::Premium,
            context_tokens: 0,
            context_limit: 0,
            turn: 0,
        }
    }

    pub fn reset_turn(&mut self) {
        self.tool_calls_this_turn = 0;
    }

    pub fn record_turn(&mut self, _had_errors: bool) {
        self.turn += 1;
    }

    pub fn record_tool_call(&mut self, _name: &str) {
        self.tool_calls_this_turn += 1;
    }
}
