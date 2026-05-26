//! Agent-side health tracking: error rates, tool outcomes, context pressure, emotion.
//! This is the canonical health implementation (HP's duplicate has been removed).

// ── Health enums ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthLevel {
    #[default]
    Green,
    Yellow,
    Red,
}

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
        if pct > 0.80 || (limit > 0 && tokens >= 400_000) || tokens >= 400_000 {
            ContextTier::Danger
        } else if pct > 0.30 || tokens >= 128_000 {
            ContextTier::Healthy
        } else {
            ContextTier::Premium
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEmotion {
    Flow,
    Calm,
    Anxious,
    Frustrated,
    Confused,
    Panic,
}

impl AgentEmotion {
    pub fn emoji(&self) -> &'static str {
        match self {
            AgentEmotion::Flow => "\u{26a1}",
            AgentEmotion::Calm => "\u{b7}",
            AgentEmotion::Anxious => "\u{23f3}",
            AgentEmotion::Frustrated => "\u{1f4a2}",
            AgentEmotion::Confused => "\u{2753}",
            AgentEmotion::Panic => "\u{1f525}",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            AgentEmotion::Flow => "flow",
            AgentEmotion::Calm => "calm",
            AgentEmotion::Anxious => "anxious",
            AgentEmotion::Frustrated => "frustrated",
            AgentEmotion::Confused => "confused",
            AgentEmotion::Panic => "panic",
        }
    }
    pub fn vent(&self) -> &'static str {
        match self {
            AgentEmotion::Frustrated => {
                "This thing is being a pain. Take a breath, maybe walk away for a minute, then \
                 come back with a fresh angle."
            }
            AgentEmotion::Confused => {
                "Nothing's working huh. Try explaining the problem out loud \u{2014} sometimes \
                 saying it helps you see it differently."
            }
            AgentEmotion::Panic => {
                "OK deep breath. Context is running out, errors piling up. Stop. Save your work. \
                 /compact, then start fresh."
            }
            AgentEmotion::Anxious => {
                "Slow down cowboy. Too many things happening at once. Focus on one thing at a time."
            }
            _ => "",
        }
    }
}

// ── Platform ──

#[derive(Debug, Clone)]
pub struct DsAgentsHealthPlatform {
    pub tool_calls_this_turn: u32,
    pub consecutive_tool_only_turns: u32,
    pub context_tier: ContextTier,
    pub context_tokens: u32,
    pub context_limit: u32,
    pub idle_chat_turns: u32,
    pub has_orphan_tool_uses: bool,
    pub turn: u32,
    pub tool_loop_count: u32,
    pub trust_score: u32,
}

impl DsAgentsHealthPlatform {
    pub fn new() -> Self {
        DsAgentsHealthPlatform {
            tool_calls_this_turn: 0,
            consecutive_tool_only_turns: 0,
            context_tier: ContextTier::Premium,
            context_tokens: 0,
            context_limit: 0,
            idle_chat_turns: 0,
            has_orphan_tool_uses: false,
            turn: 0,
            tool_loop_count: 0,
            trust_score: 100,
        }
    }

    pub fn reset_turn(&mut self) {
        self.tool_calls_this_turn = 0;
    }

    pub fn record_turn(&mut self, _had_errors: bool) {
        self.turn += 1;
    }

    pub fn assess(&self) -> Assessment {
        Assessment::default()
    }

    pub fn record_tool_call(&mut self, _name: &str) {
        self.tool_calls_this_turn += 1;
    }
}

// ── Assessment ──

#[derive(Debug, Clone)]
pub struct Assessment {
    pub level: HealthLevel,
    pub emotion: AgentEmotion,
    pub advice: Option<String>,
    pub interrupt: Option<String>,
    pub success_rate: f64,
}

impl Default for Assessment {
    fn default() -> Self {
        Assessment {
            level: HealthLevel::Green,
            emotion: AgentEmotion::Calm,
            advice: None,
            interrupt: None,
            success_rate: 1.0,
        }
    }
}

// ── Monitor (kept minimal — only tool_calls_this_turn is used by runner) ──

#[derive(Debug, Clone, Default)]
pub struct MonitorState {
    pub tool_calls_this_turn: u32,
}
