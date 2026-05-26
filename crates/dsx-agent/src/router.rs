use dsx_types::{DebugLevel, TaskPhase};
use std::sync::atomic::{AtomicU8, Ordering};

/// Global phase + debug level set by the status tool, read by start_agent_loop().
/// Encoded as: bits [2:0] = TaskPhase (0=Plan,1=Coding,2=Debug),
///             bits [4:3] = DebugLevel (0=Low,1=Medium,2=High)
pub static CURRENT_PHASE: AtomicU8 = AtomicU8::new(0);

fn encode(phase: TaskPhase, level: DebugLevel) -> u8 {
    let p = match phase {
        TaskPhase::Plan => 0u8,
        TaskPhase::Coding => 1,
        TaskPhase::Debug => 2,
    };
    let l = match level {
        DebugLevel::Low => 0u8,
        DebugLevel::Medium => 1,
        DebugLevel::High => 2,
    };
    p | (l << 3)
}

pub fn read_phase() -> TaskPhase {
    let v = CURRENT_PHASE.load(Ordering::Relaxed) & 0x07;
    match v {
        0 => TaskPhase::Plan,
        1 => TaskPhase::Coding,
        2 => TaskPhase::Debug,
        _ => TaskPhase::Coding,
    }
}

pub fn read_debug_level() -> DebugLevel {
    let v = (CURRENT_PHASE.load(Ordering::Relaxed) >> 3) & 0x03;
    match v {
        0 => DebugLevel::Low,
        2 => DebugLevel::High,
        _ => DebugLevel::Medium,
    }
}

pub fn set_phase(phase: TaskPhase, level: DebugLevel) {
    CURRENT_PHASE.store(encode(phase, level), Ordering::Relaxed);
}

// ── Routing table ──

pub struct PhaseConfig {
    pub model: &'static str,
    pub effort: Option<&'static str>,
    pub max_tokens: u32,
}

pub fn phase_config(phase: TaskPhase, level: DebugLevel) -> PhaseConfig {
    match phase {
        TaskPhase::Plan => PhaseConfig {
            model: "deepseek-v4-pro",
            effort: Some("max"),
            max_tokens: 300_000,
        },
        TaskPhase::Coding => PhaseConfig {
            model: "deepseek-v4-flash",
            effort: Some("high"),
            max_tokens: 96_000,
        },
        TaskPhase::Debug => match level {
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

pub fn phase_prompt_suffix(phase: TaskPhase, lang: &str) -> Option<&'static str> {
    if lang == "zh" {
        match phase {
            TaskPhase::Plan => Some("\n\
                模式: PLAN · 模型: Pro(超级大脑)\n\
                分析和设计阶段，输出结构化方案后等待批准。"),
            TaskPhase::Coding => Some("\n\
                模式: CODING · 模型: Flash(快速经济)\n\
                高效实现变更。"),
            TaskPhase::Debug => Some("\n\
                模式: DEBUG · 模型: Pro(超级大脑)\n\
                专注排查和修复错误。"),
        }
    } else {
        match phase {
            TaskPhase::Plan => Some("\n\
                Mode: PLAN · Model: Pro (super brain)\n\
                Analyze and design, output a structured plan, wait for approval."),
            TaskPhase::Coding => Some("\n\
                Mode: CODING · Model: Flash (fast & economical)\n\
                Implement changes efficiently."),
            TaskPhase::Debug => Some("\n\
                Mode: DEBUG · Model: Pro (super brain)\n\
                Focus on diagnosing and fixing errors."),
        }
    }
}

// ── Unified keyword lists for phase detection (merged from phase_detector + router) ──

const PLAN_KWS: &[&str] = &["plan", "design", "architect", "approach", "analyze", "outline",
    "review", "refactor", "方案", "设计", "架构", "分析", "规划", "审查", "重构"];
const CODE_KWS: &[&str] = &["implement", "write", "create", "build", "add", "modify", "edit",
    "code", "make", "实现", "编写", "创建", "添加", "修改", "写", "编码"];
const DEBUG_KWS: &[&str] = &["error", "bug", "crash", "wrong", "failed", "debug", "issue",
    "fix", "fail", "broken", "错误", "崩溃", "调试", "修复", "故障"];

/// Score-based phase detection from a lowercase scan string.
fn score_phase(scan: &str) -> TaskPhase {
    let p_score = PLAN_KWS.iter().filter(|kw| scan.contains(*kw)).count();
    let c_score = CODE_KWS.iter().filter(|kw| scan.contains(*kw)).count();
    let d_score = DEBUG_KWS.iter().filter(|kw| scan.contains(*kw)).count();

    if p_score >= c_score && p_score >= d_score && p_score >= 2 { TaskPhase::Plan }
    else if d_score >= p_score && d_score >= c_score && d_score >= 2 { TaskPhase::Debug }
    else if c_score >= p_score && c_score >= d_score && c_score >= 2 { TaskPhase::Coding }
    else { TaskPhase::Coding }
}

/// Detect task phase from AI reasoning stream (first ~300 chars).
/// Used for auto-mode phase tracking.
pub fn detect_task_phase_from_reasoning(reasoning: &str) -> TaskPhase {
    let scan: String = reasoning.chars().take(300).collect::<String>().to_lowercase();
    score_phase(&scan)
}

/// Detect initial phase from user input (used when auto_mode is ON and
/// no status() has been called yet). Same keyword lists and scoring as reasoning detection.
pub fn detect_initial_phase(input: &str) -> TaskPhase {
    score_phase(&input.to_lowercase())
}


