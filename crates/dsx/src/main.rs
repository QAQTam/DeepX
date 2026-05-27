//! DSX — umbrella binary. All roles in one binary.

fn main() {
    let _ = std::env::set_var("DSX_SINGLE_BINARY", "1");

    let role = std::env::args().nth(1).unwrap_or_default();
    match role.as_str() {
        "hp" => dsx_hp::runner::run(),
        "agent" => dsx_agent::runner::run(),
        "tools" => dsx_tools::run(),
        "config" | "init" => run_config(),
        _ => {
            // Default: headless agent (stdin/stdout JSON-LP)
            dsx_agent::runner::run();
        }
    }
}

/// Interactive setup wizard: writes ~/.dsx/config.json
fn run_config() {
    use std::io::Write;

    let dir = dsx_types::platform::data_dir();
    let cfg_path = dsx_types::platform::config_path();
    let _ = std::fs::create_dir_all(&dir);

    // Read existing values
    let mut api_key = String::new();
    let mut base_url = "https://api.deepseek.com/anthropic".to_string();
    let mut model = "deepseek-v4-flash".to_string();
    let mut context_limit = 1_000_000u32;
    if let Ok(data) = std::fs::read_to_string(&cfg_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            if let Some(k) = v.get("api_key").and_then(|k| k.as_str()) { api_key = k.to_string(); }
            if let Some(u) = v.get("base_url").and_then(|b| b.as_str()) { base_url = u.to_string(); }
            if let Some(m) = v.get("model").and_then(|m| m.as_str()) { model = m.to_string(); }
            if let Some(c) = v.get("context_limit").and_then(|c| c.as_u64()) { context_limit = c as u32; }
        }
    }

    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║   DSX — AI coding assistant setup    ║");
    println!("╚══════════════════════════════════════╝");
    println!("(leave blank to keep current value)");
    println!();

    // Step 1: API key
    println!("[1/3] DeepSeek API key");
    print!("  Key [{}]: ", if api_key.is_empty() { "(none)" } else { "****" });
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if !trimmed.is_empty() { api_key = trimmed; }

    // Step 2: Model name
    println!();
    println!("[2/3] Model name");
    println!("  deepseek-v4-flash    — Fast, general purpose");
    println!("  deepseek-v4-pro      — High capability");
    print!("  Model [{}]: ", model);
    std::io::stdout().flush().ok();
    input.clear();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if !trimmed.is_empty() { model = trimmed; }

    // Step 3: Context limit
    println!();
    println!("[3/3] Max context tokens (DeepSeek: 1,000,000)");
    print!("  Context limit [{}]: ", context_limit);
    std::io::stdout().flush().ok();
    input.clear();
    std::io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if !trimmed.is_empty() {
        if let Ok(v) = trimmed.parse::<u32>() {
            context_limit = v;
        }
    }

    // Build and save config
    let config = serde_json::json!({
        "api_key": api_key,
        "base_url": base_url,
        "model": model,
        "context_limit": context_limit,
        "auto_mode": true,
        "prompt_lang": "zh",
    });
    match std::fs::write(&cfg_path, serde_json::to_string_pretty(&config).unwrap()) {
        Ok(_) => {
            println!();
            println!("Config saved to {}", cfg_path.display());
            println!("Run `dsx` to start.");
        }
        Err(e) => eprintln!("Error saving config: {e}"),
    }
}
