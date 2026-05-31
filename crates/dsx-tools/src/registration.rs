//! ToolManager 初始化构造器。
//!
//! 各模块的 `register()` 在此组装。

use super::ToolManager;
use super::exec;
use super::explore;
use super::web;
use super::file;
use super::task;
use super::plan;
use super::ask;
use super::{ToolCallCtx, ToolHandler, ToolKey, ToolPhase, ToolResult, SafetyVerdict, set_phase};
use std::time::Duration;


/// 构造并注册全部工具 handler，返回初始化后的 ToolManager。
pub fn build_tool_manager() -> ToolManager {
    let mut mgr = ToolManager::new();

    // ── Phase commit (must be first to unlock other tools) ──
    mgr.register(commit_handler());

    // ── 系统工具 ──
    exec::register(&mut mgr);
    explore::register(&mut mgr);
    web::register(&mut mgr);

    // ── 文件操作 ──
    file::register(&mut mgr);

    // ── 任务/计划 ──
    task::register(&mut mgr);
    plan::register(&mut mgr);

    // ── 用户交互 ──
    ask::register(&mut mgr);


    mgr
}

fn commit_handler() -> ToolHandler {
    ToolHandler {
        key: ToolKey::new("commit", ""),
        description: "Commit to an execution phase to unlock tools. Required before any file/exec/explore operations.\n\nIn Plan mode, ALL tools are blocked. You MUST call commit(state=\"coding\") or commit(state=\"debug\") first.\n\n- coding: full tool access, use deepseek-v4-flash model\n- debug: full tool access, use deepseek-v4-pro model with max effort",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "state": {
                    "type": "string",
                    "enum": ["coding", "debug"],
                    "description": "The phase to commit to. Plan/explore cannot be committed — they are the starting state."
                }
            },
            "required": ["state"],
            "additionalProperties": false
        }),
        handler: |ctx: ToolCallCtx| -> ToolResult {
            let state = ctx.get_str("state").unwrap_or("coding");
            match state {
                "coding" => {
                    set_phase(ToolPhase::Coding);
                    ToolResult::ok("[OK] Committed to coding mode. All tools unlocked.\n[HINT] Use deepseek-v4-flash for fast, economical execution.")
                }
                "debug" => {
                    set_phase(ToolPhase::Debug);
                    ToolResult::ok("[OK] Committed to debug mode. All tools unlocked.\n[HINT] Use deepseek-v4-pro with max effort for deep analysis.")
                }
                other => {
                    ToolResult {
                        success: false,
                        content: format!("[ERROR] Unknown state '{}'. Use 'coding' or 'debug'.", other),
                    }
                }
            }
        },
        safety: |_: &ToolCallCtx| -> SafetyVerdict {
            SafetyVerdict::Allow
        },
        default_timeout: Duration::from_secs(5),
    }
}
