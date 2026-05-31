use dsx_types::DebugLevel;

// ── Routing table ──

pub struct PhaseConfig {
    pub model: &'static str,
    pub effort: Option<&'static str>,
    pub max_tokens: u32,
}

pub fn phase_config(phase: dsx_types::TaskPhase, level: DebugLevel) -> PhaseConfig {
    match phase {
        dsx_types::TaskPhase::Plan => PhaseConfig {
            model: "deepseek-v4-pro",
            effort: Some("max"),
            max_tokens: 300_000,
        },
        dsx_types::TaskPhase::Coding => PhaseConfig {
            model: "deepseek-v4-flash",
            effort: Some("high"),
            max_tokens: 96_000,
        },
        dsx_types::TaskPhase::Debug => match level {
            DebugLevel::Low => PhaseConfig {
                model: "deepseek-v4-pro",
                effort: Some("high"),
                max_tokens: 64_000,
            },
            DebugLevel::Medium => PhaseConfig {
                model: "deepseek-v4-pro",
                effort: Some("max"),
                max_tokens: 96_000,
            },
            DebugLevel::High => PhaseConfig {
                model: "deepseek-v4-pro",
                effort: Some("max"),
                max_tokens: 128_000,
            },
        },
    }
}

// ── Phase prompt suffixes ──

pub fn phase_prompt_suffix(phase: dsx_types::TaskPhase) -> Option<&'static str> {
    match phase {
        dsx_types::TaskPhase::Plan => Some("\n\
            Mode: PLAN · Model: Pro (super brain)\n\
            Analyze and design, output a structured plan, wait for approval."),
        dsx_types::TaskPhase::Coding => Some("\n\
            Mode: CODING · Model: Flash (fast & economical)\n\
            Implement changes efficiently."),
        dsx_types::TaskPhase::Debug => Some("\n\
            Mode: DEBUG · Model: Pro (super brain)\n\
            Focus on diagnosing and fixing errors."),
    }
}

/// Read current phase from dsx_tools as TaskPhase.
pub fn read_phase() -> dsx_types::TaskPhase {
    match dsx_tools::current_phase() {
        dsx_tools::ToolPhase::Plan => dsx_types::TaskPhase::Plan,
        dsx_tools::ToolPhase::Coding => dsx_types::TaskPhase::Coding,
        dsx_tools::ToolPhase::Debug => dsx_types::TaskPhase::Debug,
    }
}


