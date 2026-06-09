//! DSX — single binary for all roles.
//!
//! Usage:
//!   dsx              → Tauri GUI (default, double-click)
//!   dsx --tui         → Terminal UI
//!   dsx --agent       → Headless agent (stdin/stdout JSON-LP)
//!   dsx config|init   → Setup wizard

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        "--tui" => {
            if let Err(e) = dsx_tui::run_tui() {
                eprintln!("dsx-tui: {e}");
                std::process::exit(1);
            }
        }
        "--agent" => {
            eprintln!("headless mode removed — use dsx-msglp");
        }
        "config" | "init" => {
            run_config();
        }
        _ => {
            // Default: Tauri GUI (handles its own arg parsing)
            dsx_tauri_lib::run();
        }
    }
}

/// Interactive setup wizard: writes ~/.config/dsx/config.toml
fn run_config() {
    use std::io::Write;

    let store = dsx_types::ConfigStore::default_location();
    let _ = std::fs::create_dir_all(
        dsx_types::platform::data_dir(),
    );

    let mut api_key = String::new();
    let (pid, ep) = deepx_config::registry::first_provider_endpoint();
    let mut base_url = deepx_config::registry::base_url_for(&pid, &ep);
    let mut model = deepx_config::registry::default_model_for(&pid, &ep);
    let mut context_limit = 1_000_000u32;

    if let Some(existing) = store.load_value() {
        if let Some(k) = existing.get("api_key").and_then(|k| k.as_str()) { api_key = k.to_string(); }
        if let Some(u) = existing.get("base_url").and_then(|b| b.as_str()) { base_url = u.to_string(); }
        if let Some(m) = existing.get("model").and_then(|m| m.as_str()) { model = m.to_string(); }
        if let Some(c) = existing.get("context_limit").and_then(|c| c.as_u64()) { context_limit = c as u32; }
    }

    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║   DSX — AI coding assistant setup    ║");
    println!("╚══════════════════════════════════════╝");
    println!("(leave blank to keep current value)");
    println!();

    println!("[1/3] API key");
    print!("  Key [{}]: ", if api_key.is_empty() { "(none)" } else { "****" });
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if !trimmed.is_empty() { api_key = trimmed; }

    println!();
    println!("[2/3] Model name");
    println!("  {} (default)", model);
    print!("  Model [{}]: ", model);
    std::io::stdout().flush().ok();
    input.clear();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if !trimmed.is_empty() { model = trimmed; }

    println!();
    println!("[3/3] Max context tokens (default: 1,000,000)");
    print!("  Context limit [{}]: ", context_limit);
    std::io::stdout().flush().ok();
    input.clear();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if !trimmed.is_empty() {
        match trimmed.parse::<u32>() {
            Ok(v) => context_limit = v,
            Err(_) => println!("  [ERROR] Invalid number '{}', using default {}", trimmed, context_limit),
        }
    }

    let pc = dsx_types::PersistentConfig {
        api_key: Some(api_key),
        base_url: Some(base_url),
        model: Some(model),
        context_limit: Some(context_limit),
        ..Default::default()
    };

    if store.save(&pc) {
        println!();
        println!("Config saved to {}", dsx_types::platform::config_path().display());
        println!("Run `dsx` to start.");
    } else {
        eprintln!("Error saving config");
    }
}
