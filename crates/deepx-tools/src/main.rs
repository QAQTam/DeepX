//! Standalone deepx-tools binary — run individual tools from CLI.
//!
//! Usage:
//!   deepx-tools <tool_name> [json_args]
//!   deepx-tools explore
//!   deepx-tools read_file '{"path":"src/main.rs","start_line":1,"end_line":50}'
//!   deepx-tools list

use std::env;

fn main() {
    let mut mgr = deepx_tools::registration::build_tool_manager();
    let cwd = env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".into());
    deepx_tools::set_workspace(&cwd);

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
    eprintln!("Usage: deepx-tools <tool> [json_args]");
    eprintln!("       deepx-tools list");
        std::process::exit(1);
    }

    let tool = &args[1];
    if tool == "list" {
        let defs = mgr.all_defs();
        println!("Available tools:");
        for def in &defs {
            println!("  {} — {}", def.function.name, def.function.description);
        }
        println!("\n{} tools registered", defs.len());
        return;
    }

    let json_args = args.get(2).map(|s| s.as_str()).unwrap_or("{}");
    let parsed_args: serde_json::Value = serde_json::from_str(json_args).unwrap_or_else(|_| {
        eprintln!("Error: invalid JSON args '{}'", json_args);
        std::process::exit(1);
    });

    let r = mgr.handle_req(
        "cli_0".into(),
        tool,
        "",
        parsed_args,
        None,
        None,
    );

    println!("{}", r.content);
    if !r.success { std::process::exit(1); }
}
