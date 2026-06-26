//! deepx-tauri library — exposed for the unified deepx binary.

mod agent_bridge;

/// Entry point called by the `deepx` binary when run without flags.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize the multi-agent registry (replaces old singleton AgentBridge).
            agent_bridge::AgentRegistry::init(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_bridge::cmd_send_message,
            agent_bridge::cmd_cancel,
            agent_bridge::cmd_get_debug_snapshot,
            agent_bridge::cmd_save_config,
            agent_bridge::cmd_load_config,
            agent_bridge::cmd_list_sessions,
            agent_bridge::cmd_load_session,
            agent_bridge::cmd_set_active_session,
            agent_bridge::cmd_delete_session,
            agent_bridge::cmd_undo_turn,
            agent_bridge::cmd_compact,
            agent_bridge::cmd_resume_session,
            agent_bridge::cmd_new_session,
            agent_bridge::cmd_load_more_turns,
            agent_bridge::cmd_get_workspace,
            agent_bridge::cmd_set_workspace,
            agent_bridge::cmd_close_session,
            agent_bridge::cmd_get_token_stats,
            agent_bridge::cmd_get_code_stats,
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Gracefully terminate all agent child processes.
                agent_bridge::shutdown_all_agents();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running DeepX Tauri application");
}
