// No windows_subsystem attribute — always a console application.
// For GUI mode (Tauri), we hide the console window at startup.
// For TUI/agent modes, the console is used normally.

fn main() {
    // Capture full system PATH at process start, before Windows GUI subsystem
    // strips it. This is injected into child agent processes so pwsh/conpty
    // can find git, cargo, etc.
    deepx_tauri_lib::agent_bridge::cache_system_path();
    
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        "--tui" => {
            // Always a console app — no special allocation needed.
            // Set UTF-8 code page for box-drawing and CJK.
            #[cfg(target_os = "windows")]
            unsafe {
                unsafe extern "system" {
                    fn SetConsoleOutputCP(codePage: u32) -> i32;
                    fn SetConsoleCP(codePage: u32) -> i32;
                }
                SetConsoleOutputCP(65001);
                SetConsoleCP(65001);
            }
            if let Err(e) = deepx_terminalui::run_tui() {
                eprintln!("deepx-terminalui: {e}");
                std::process::exit(1);
            }
        }
        "--agent" | "agent" => {
            run_agent(false);
        }
        "subagent" => {
            run_agent(true);
        }
        "daemon" => {
            // deepxd daemon process — managed by deepx-daemon crate
            deepx_daemon::run();
        }
        "config" | "init" => {
            run_config();
        }
        _ => {
            // Default: Tauri GUI — hide the console window.
            #[cfg(target_os = "windows")]
            unsafe {
                unsafe extern "system" {
                    fn GetConsoleWindow() -> isize;
                    fn ShowWindow(hWnd: isize, nCmdShow: i32) -> i32;
                }
                let hwnd = GetConsoleWindow();
                if hwnd != 0 {
                    ShowWindow(hwnd, 0); // SW_HIDE
                }
            }
            deepx_tauri_lib::run();
        }
    }
}

/// Shared agent/subagent entry point.
/// `is_subagent`: uses subagent config defaults, always ephemeral.
fn run_agent(is_subagent: bool) {
    let mut resume_seed: Option<String> = None;
    let mut new_seed: Option<String> = None;
    let mut tools_allowlist: Vec<String> = Vec::new();
    let mut ephemeral = is_subagent; // subagents are always ephemeral
    let mut model_override: Option<String> = None;
    let mut base_url_override: Option<String> = None;
    let mut api_key_override: Option<String> = None;
    let mut max_tokens_override: Option<u32> = None;

    // Load subagent defaults if applicable
    let subagent_defaults = if is_subagent {
        deepx_config::Config::load().map(|c| c.subagent).unwrap_or_default()
    } else {
        Default::default()
    };

    let args: Vec<String> = std::env::args().collect();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--resume-seed" => {
                if i + 1 < args.len() { resume_seed = Some(args[i + 1].clone()); i += 1; }
            }
            "--seed" => {
                if i + 1 < args.len() { new_seed = Some(args[i + 1].clone()); i += 1; }
            }
            "--tools" => {
                if i + 1 < args.len() {
                    if let Ok(list) = serde_json::from_str::<Vec<String>>(&args[i + 1]) { tools_allowlist = list; }
                    i += 1;
                }
            }
            "--ephemeral" => { ephemeral = true; }
            "--model" => {
                if i + 1 < args.len() { model_override = Some(args[i + 1].clone()); i += 1; }
            }
            "--base-url" => {
                if i + 1 < args.len() { base_url_override = Some(args[i + 1].clone()); i += 1; }
            }
            "--api-key" => {
                if i + 1 < args.len() { api_key_override = Some(args[i + 1].clone()); i += 1; }
            }
            "--max-tokens" => {
                if i + 1 < args.len() {
                    if let Ok(v) = args[i + 1].parse::<u32>() { max_tokens_override = Some(v); }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Apply subagent defaults (CLI overrides take priority)
    if is_subagent {
        if model_override.is_none() && !subagent_defaults.model.is_empty() {
            model_override = Some(subagent_defaults.model.clone());
        }
        if base_url_override.is_none() && !subagent_defaults.base_url.is_empty() {
            base_url_override = Some(subagent_defaults.base_url.clone());
        }
        if api_key_override.is_none() && !subagent_defaults.api_key.is_empty() {
            api_key_override = Some(subagent_defaults.api_key.clone());
        }
        if max_tokens_override.is_none() {
            max_tokens_override = Some(subagent_defaults.max_tokens);
        }
        if tools_allowlist.is_empty() && !subagent_defaults.default_tools.is_empty() {
            tools_allowlist = subagent_defaults.default_tools.clone();
        }
    }

    deepx_session::SessionManager::init(deepx_types::platform::data_dir());
    let _ = deepx_msglp::logger::init_agent_logger(&deepx_types::platform::data_dir());

    let mut agent = if tools_allowlist.is_empty() && model_override.is_none() && base_url_override.is_none() && !ephemeral {
        deepx_msglp::agent::AgentState::init("cli")
    } else {
        deepx_msglp::agent::AgentState::init_subagent(&tools_allowlist, ephemeral)
    };

    if let Some(m) = model_override { agent.config.model = m; }
    if let Some(u) = base_url_override { agent.config.base_url = u; }
    if let Some(k) = api_key_override { agent.config.api_key = k; }
    if let Some(mt) = max_tokens_override { agent.config.max_tokens = mt; }

    if let Some(seed) = resume_seed { agent.session.resume_seed = Some(seed); }
    if let Some(seed) = new_seed {
        agent.session.seed = seed;
        agent.session.created_at = deepx_session::SessionManager::now_epoch();
    }

    let stdin = std::io::BufReader::new(std::io::stdin());
    let stdout = std::io::stdout();
    let mut loop_ = deepx_msglp::Loop::new_ipc(agent, stdin, stdout);
    loop_.run();
}

/// Interactive setup wizard: writes config.toml
fn run_config() {
    use std::io::Write;

    let store = deepx_types::ConfigStore::default_location();
    let _ = std::fs::create_dir_all(deepx_types::platform::data_dir());

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
    println!("║   deepx — AI coding assistant setup    ║");
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

    let pc = deepx_types::PersistentConfig {
        api_key: Some(api_key),
        base_url: Some(base_url),
        model: Some(model),
        context_limit: Some(context_limit),
        ..Default::default()
    };

    if store.save(&pc) {
        println!();
        println!("Config saved to {}", deepx_types::platform::config_path().display());
        println!("Run `deepx` to start.");
    } else {
        eprintln!("Error saving config");
    }
}
