#!/usr/bin/env python3
"""Extract cmd_* functions from agent_bridge_legacy.rs into separate module files."""
import re, os

SRC = r"F:\DeepX-Fork\crates\deepx-tauri\src-tauri\src"
LEGACY = os.path.join(SRC, "agent_bridge_legacy.rs")

# Mapping: function name prefix → target module
ROUTING = {
    # session.rs
    "cmd_send_message": "session",
    "cmd_set_mode": "session",
    "cmd_resume_session": "session",
    "cmd_new_session": "session",
    "cmd_cancel": "session",
    "cmd_get_activity": "session",
    "cmd_undo_turn": "session",
    "cmd_compact": "session",
    "cmd_load_more_turns": "session",
    "cmd_close_session": "session",
    "cmd_get_dashboard_data": "session",
    # permission.rs
    "cmd_permission_response": "permission",
    "cmd_ask_response": "permission",
    "cmd_ask_dismiss": "permission",
    # git.rs
    "cmd_get_git_diff": "git",
    "cmd_get_git_branch": "git",
    "cmd_list_branches": "git",
    "cmd_switch_branch": "git",
    "cmd_git_commit": "git",
    "cmd_get_git_file_diff": "git",
    # config.rs
    "cmd_unload_skill": "config",
    "cmd_activate_skill": "config",
    "cmd_reload_skills": "config",
    "cmd_get_version": "config",
    "cmd_list_available_tools": "config",
    "cmd_save_config": "config",
    "cmd_load_config": "config",
    "cmd_list_sessions": "config",
    "cmd_delete_session": "config",
    "cmd_get_workspace": "config",
    "cmd_set_workspace": "config",
    "cmd_migration_count": "plan",
    "cmd_migrate_to_turso": "plan",
    # plan.rs
    "cmd_task_action": "plan",
    "cmd_get_context_stats": "plan",
    "cmd_get_token_stats": "plan",
    "cmd_read_plan": "plan",
    "cmd_plan_action": "plan",
}

IMPORTS = {
    "session": "use tauri::AppHandle;\nuse deepx_proto::Ui2Agent;\nuse super::super::registry::{ensure_agent, send_to_agent, AgentRegistry};\nuse super::super::util::read_file_preview;\nuse super::super::config::resolve_deepx_dir;\n",
    "permission": "use tauri::AppHandle;\nuse deepx_proto::Ui2Agent;\nuse super::super::registry::{ensure_agent, send_to_agent};\n",
    "git": "use tauri::AppHandle;\nuse deepx_proto::Ui2Agent;\nuse super::super::registry::send_to_agent;\n",
    "config": "use tauri::AppHandle;\nuse deepx_proto::Ui2Agent;\nuse super::super::registry::{ensure_agent, send_to_agent, AgentRegistry};\n",
    "plan": "use tauri::AppHandle;\nuse deepx_proto::Ui2Agent;\nuse super::super::registry::send_to_agent;\nuse super::super::util::{parse_plan_items, PlanItem, chrono_local_date_from_epoch, generate_date_range, days_before_today};\nuse super::super::config::resolve_deepx_dir;\n",
}

DOCS = {
    "session": "//! Session lifecycle commands: send message, create/resume/close session,\n//! cancel, set mode, dashboard, activity, undo, compact, load more turns.\n",
    "permission": "//! Permission dialog and ask_user response commands.\n",
    "git": "//! Git operation commands: diff, branch listing, switch, commit.\n",
    "config": "//! Configuration, tool listing, skill management, workspace, session list commands.\n",
    "plan": "//! Plan/task management, context stats, token stats, migration commands.\n",
}

with open(LEGACY, "r", encoding="utf-8") as f:
    text = f.read()

# Split by #[tauri::command] boundaries
# Find all functions: look for #[tauri::command]\npub fn cmd_XXX
pattern = r'(?:(?:^|\n)\s*///[^\n]*\n)*\s*#\[tauri::command\][\s\S]*?(?=\n(?:#\[tauri::command\]|\n// ──|\n\Z))'
funcs = re.findall(pattern, text)

print(f"Found {len(funcs)} tauri commands")

# Group by target module
modules = {k: [] for k in set(ROUTING.values())}

for func_body in funcs:
    # Determine which module this belongs to
    target = None
    for name, mod_name in ROUTING.items():
        if name in func_body:
            target = mod_name
            break
    if target:
        modules[target].append(func_body.strip())
    else:
        # Check if it's a cmd_ we don't know about
        if "pub fn cmd_" in func_body:
            print(f"  WARNING: unmatched command in:\n{func_body[:100]}...")

for mod_name, bodies in sorted(modules.items()):
    if not bodies:
        continue
    out_path = os.path.join(SRC, "agent_bridge", "commands", f"{mod_name}.rs")
    content = DOCS.get(mod_name, "") + "\n" + IMPORTS.get(mod_name, "") + "\n" + "\n\n".join(bodies) + "\n"
    with open(out_path, "w", encoding="utf-8") as f:
        f.write(content)
    line_count = content.count("\n") + 1
    print(f"  {mod_name}.rs: {len(bodies)} functions, {line_count} lines → {out_path}")

print("Done!")
