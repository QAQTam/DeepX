//! deepx-tauri library — exposed for the unified deepx binary.

mod agent_bridge;

use tauri::Manager;

/// Entry point called by the `deepx` binary when run without flags.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Spawn the agent child process and expose its stdin writer via the bridge singleton.
            let bridge = agent_bridge::AgentBridge::init(app.handle());
            app.manage(bridge);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_bridge::cmd_send_message,
            agent_bridge::cmd_create_session,
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
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Gracefully terminate the agent child process before the window closes.
                agent_bridge::shutdown_agent();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running DeepX Tauri application");
}