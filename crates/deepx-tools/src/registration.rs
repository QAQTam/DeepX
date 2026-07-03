//! ToolManager 初始化构造器。
//!
//! 各模块的 `register()` 在此组装。外部注册器通过 `extra_registrars` 注入。

use super::ToolManager;
use super::exec;
use super::explore;
use super::web;
use super::file_mutate;
use super::file_query;

use super::task;
use super::ask_user;
use super::process_inspect;
use super::memory;
use super::git_tool;

/// 工具注册器函数签名。
pub type ToolRegistrar = fn(&mut ToolManager);

/// 构造并注册全部工具 handler，返回初始化后的 ToolManager。
/// `extra_registrars` 允许外部 crate（如 deepx-subagent）注入工具。
pub fn build_tool_manager(extra_registrars: &[ToolRegistrar]) -> ToolManager {
    let mut mgr = ToolManager::new();

    // ── 系统工具 ──
    exec::register(&mut mgr);
    explore::register(&mut mgr);
    web::register(&mut mgr);

    // ── 文件操作 ──
    file_mutate::register(&mut mgr);
    file_query::register(&mut mgr);

    // ── 任务 ──
    task::register(&mut mgr);

    // ── 交互 ──
    ask_user::register(&mut mgr);

    // ── 进程巡查 ──
    process_inspect::register(&mut mgr);

    // ── 跨会话记忆 ──
    memory::register(&mut mgr);

    // ── Git ──
    git_tool::register(&mut mgr);

    // ── 外部注册器 ──
    for reg in extra_registrars {
        reg(&mut mgr);
    }

    mgr
}
