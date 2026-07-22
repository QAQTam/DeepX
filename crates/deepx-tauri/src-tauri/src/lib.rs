mod backend_bridge;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            backend_bridge::BackendBridge::init(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            backend_bridge::backend_connect,
            backend_bridge::backend_request,
            backend_bridge::backend_attach,
            backend_bridge::backend_detach,
            backend_bridge::backend_status,
        ])
        .on_window_event(|_, event| {
            if let tauri::WindowEvent::Destroyed = event {
                backend_bridge::BackendBridge::release_all();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running DeepX desktop application");
}
