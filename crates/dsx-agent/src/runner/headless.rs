//! Headless mode: stdin/stdout JSON-LP pipe adapter.

use std::io::BufReader;
use std::sync::mpsc;

use dsx_proto::{self, Agent2Ui, Ui2Agent};

use crate::agent::AgentState;

pub fn run() {
    eprintln!("dsx-agent starting (headless mode)");

    crate::dsx_log::init();

    crate::skills::init();

    let config = crate::config::Config::load().unwrap_or_default();
    eprintln!(
        "dsx-agent: model={} effort={:?} context_limit={}",
        config.model, config.effort, config.context_limit
    );

    let args: Vec<String> = std::env::args().collect();
    let resume_seed = args
        .windows(2)
        .find(|w| w[0] == "--session")
        .and_then(|w| Some(w[1].clone()));
    if let Some(ref seed) = resume_seed {
        eprintln!("dsx-agent: resume request for session {seed}");
    }

    let mcp_configs = config.mcp_servers.clone();
    let mut agent = AgentState::new(config);
    agent.resume_seed = resume_seed;
    agent.health.context_limit = agent.config.context_limit;

    let hp_conn = crate::gate::try_reconnect().map(BufReader::new);

    crate::tools::init_tools("pipe", &mcp_configs);
    if let Some(ref key) = agent.config.context7_api_key {
        if !key.is_empty() {
            crate::tools::set_context7_key(key);
        }
    }
    agent.tool_defs = crate::tools::all_tools();
    eprintln!("dsx-agent: tools → {}", agent.tool_defs.len());

    let lives = crate::session::find_live_sessions();
    if !lives.is_empty() {
        eprintln!(
            "dsx-agent: {} live session(s) available for resume",
            lives.len()
        );
    }

    let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    let stdin_handle = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match dsx_proto::read_frame::<Ui2Agent>(&mut reader) {
                Ok(Some(frame)) => {
                    if tui_tx.send(frame).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("dsx-agent: stdin parse error: {e}");
                    continue;
                }
            }
        }
    });

    let stdout_handle = std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        while let Ok(frame) = agent_rx.recv() {
            if dsx_proto::write_frame(&mut stdout, &frame).is_err() {
                break;
            }
        }
    });

    super::run_agent_loop(agent, hp_conn, tui_rx, agent_tx);

    crate::tools::shutdown_tools();
    crate::gate::kill_hp_daemon();
    drop(stdin_handle);
    drop(stdout_handle);
}
