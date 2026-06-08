//! Headless mode: stdin/stdout JSON-LP pipe adapter.

use std::io::BufReader;
use std::sync::mpsc;

use dsx_proto::{self, Agent2Ui, Ui2Agent};

use crate::agent::AgentState;

pub fn run() {
    eprintln!("dsx-agent starting (headless mode)");

    crate::dsx_log::init();

    let config = crate::config::Config::load().unwrap_or_default();
    eprintln!(
        "dsx-agent: model={} effort={:?} context_limit={}",
        config.model, config.reasoning_effort, config.context_limit
    );

    let args: Vec<String> = std::env::args().collect();
    let resume_seed = args
        .windows(2)
        .find(|w| w[0] == "--session")
        .and_then(|w| Some(w[1].clone()));

    let mut agent = AgentState::init("pipe");
    let active = dsx_session::SessionManager::global().active_seed();
    agent.session.resume_seed = active.or(resume_seed);

    if let Some(ref seed) = agent.session.resume_seed {
        eprintln!("dsx-agent: resume seed {seed}");
    }
    eprintln!("dsx-agent: tools -> {}", agent.tool_defs.len());

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

    super::run_agent_loop(agent, tui_rx, agent_tx);

    crate::tools::shutdown_tools();
    drop(stdin_handle);
    drop(stdout_handle);
}
