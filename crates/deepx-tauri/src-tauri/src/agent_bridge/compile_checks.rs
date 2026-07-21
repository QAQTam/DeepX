//! Compile-time verification that all 38 Tauri commands are reachable
//! through the agent_bridge re-export chain.
//!
//! These tests use function-pointer coercion (`as fn(...) -> _`) which
//! resolves the symbol path at compile time without calling any code.
//! They are verified via `cargo check --tests`, not `cargo test`,
//! because the Tauri runtime DLLs aren't available in unit test binaries.

// Note: #[cfg(test)] means this only compiles with --tests

use crate::agent_bridge;

#[test]
fn all_session_commands_visible() {
    let _ = agent_bridge::cmd_send_message as fn(_, _, _) -> _;
    let _ = agent_bridge::cmd_set_mode as fn(_, _) -> _;
    let _ = agent_bridge::cmd_cancel as fn(_) -> _;
    let _ = agent_bridge::cmd_resume_session as fn(_) -> _;
    let _ = agent_bridge::cmd_replay_session_events as fn(_) -> _;
    let _ = agent_bridge::cmd_new_session as fn() -> _;
    let _ = agent_bridge::cmd_close_session as fn(_) -> _;
    let _ = agent_bridge::cmd_undo_turn as fn(_, _) -> _;
    let _ = agent_bridge::cmd_compact as fn(_) -> _;
    let _ = agent_bridge::cmd_load_more_turns as fn(_, _) -> _;
    let _ = agent_bridge::cmd_get_dashboard_data as fn(_) -> _;
    let _ = agent_bridge::cmd_get_activity as fn(_) -> _;
}

#[test]
fn all_permission_commands_visible() {
    let _ = agent_bridge::cmd_permission_response as fn(_, _, _, _) -> _;
    let _ = agent_bridge::cmd_ask_response as fn(_, _, _) -> _;
    let _ = agent_bridge::cmd_ask_dismiss as fn(_, _) -> _;
    let _ = agent_bridge::cmd_plan_review as fn(_, _, _, _, _) -> _;
}

#[test]
fn all_git_commands_visible() {
    let _ = agent_bridge::cmd_get_git_diff as fn(_) -> _;
    let _ = agent_bridge::cmd_get_git_branch as fn(_) -> _;
    let _ = agent_bridge::cmd_list_branches as fn(_) -> _;
    let _ = agent_bridge::cmd_switch_branch as fn(_, _, _) -> _;
    let _ = agent_bridge::cmd_git_commit as fn(_, _) -> _;
    let _ = agent_bridge::cmd_get_git_file_diff as fn(_, _) -> _;
}

#[test]
fn all_config_commands_visible() {
    let _ = agent_bridge::cmd_save_config; // 20-param fn, can't fn-ptr cast
    let _ = agent_bridge::cmd_set_database_enabled as fn(_) -> _;
    let _ = agent_bridge::cmd_load_config as fn() -> _;
    let _ = agent_bridge::cmd_list_sessions as fn() -> _;
    let _ = agent_bridge::cmd_list_session_activity as fn() -> _;
    let _ = agent_bridge::cmd_audit_turso_mirrors as fn() -> _;
    let _ = agent_bridge::cmd_delete_session as fn(_) -> _;
    let _ = agent_bridge::cmd_get_workspace as fn(_) -> _;
    let _ = agent_bridge::cmd_set_workspace as fn(_, _) -> _;
    let _ = agent_bridge::cmd_unload_skill as fn(_, _) -> _;
    let _ = agent_bridge::cmd_activate_skill as fn(_, _) -> _;
    let _ = agent_bridge::cmd_skill_operation as fn(_, _, _, _, _) -> _;
    let _ = agent_bridge::cmd_reload_skills as fn(_) -> _;
}

#[test]
fn all_plan_commands_visible() {
    let _ = agent_bridge::cmd_read_plan as fn(_) -> _;
    let _ = agent_bridge::cmd_plan_action as fn(_, _, _, _, _) -> _;
    let _ = agent_bridge::cmd_task_action as fn(_, _, _) -> _;
    let _ = agent_bridge::cmd_get_context_stats as fn(_) -> _;
    let _ = agent_bridge::cmd_migration_count as fn() -> _;
    let _ = agent_bridge::cmd_migrate_to_turso as fn() -> _;
}

#[test]
fn platform_and_util_visible() {
    let _ = agent_bridge::cache_system_path as fn();
    let _ = agent_bridge::detect_os_info as fn();
    let _: Option<agent_bridge::AgentRegistry> = None;
    let _: Option<agent_bridge::AgentInstance> = None;
}
