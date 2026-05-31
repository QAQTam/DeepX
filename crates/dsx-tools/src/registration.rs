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



/// 构造并注册全部工具 handler，返回初始化后的 ToolManager。
pub fn build_tool_manager() -> ToolManager {
    let mut mgr = ToolManager::new();

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
