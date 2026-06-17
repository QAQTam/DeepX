#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        "agent" => {
            // IPC agent loop: reads JSON-LP commands from stdin, writes JSON-LP events to stdout.
            let mut resume_seed: Option<String> = None;
            let args: Vec<String> = std::env::args().collect();
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--resume-seed" => {
                        if i + 1 < args.len() {
                            resume_seed = Some(args[i + 1].clone());
                            i += 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }

            deepx_session::SessionManager::init(deepx_types::platform::data_dir());
            let _ = deepx_msglp::logger::init_agent_logger(&deepx_types::platform::data_dir());
            let mut agent = deepx_msglp::agent::AgentState::init("cli");
            if let Some(seed) = resume_seed {
                agent.session.resume_seed = Some(seed);
            }

            let stdin = std::io::BufReader::new(std::io::stdin());
            let stdout = std::io::stdout();
            let mut loop_ = deepx_msglp::Loop::new_ipc(agent, stdin, stdout);
            loop_.run();
        }
        _ => {
            // Launch the Tauri GUI application (dev mode or end-user launch).
            deepx_tauri_lib::run();
        }
    }
}