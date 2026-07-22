#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[FATAL] {info}");
        log::logger().flush();
    }));
    deepx_tauri_lib::run();
}
