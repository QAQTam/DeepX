//! dsx-tauri library — exposed for the unified dsx binary.

mod agent_bridge;

use tauri::Manager;

/// Entry point called by the `dsx` binary when run without flags.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
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
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(bridge) = window.try_state::<agent_bridge::AgentBridge>() {
                    bridge.shutdown();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running DeepX Tauri application");
}
