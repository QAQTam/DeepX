//! dsx-agent runner — main event loop and headless adapter.
//!
//! Submodules:
//! - `lifecycle` — session init, health status, phase config
//! - `hp_bridge` — HP TCP stream reading, result emission
//! - `turn` — user input handling, tool execution, context building

mod lifecycle;
mod hp_bridge;
pub mod turn;

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, AgentToHp, AgentToTools, AgentToTui, HpToAgent, ToolsToAgent, TuiToAgent};

use crate::agent::AgentState;
use crate::orchestrator::maybe_save_session;
use crate::router;

// ── Channel-based main loop (primary entry point, called by tui.rs) ──

pub fn run_agent_loop(
    mut agent: AgentState,
    mut hp_conn: Option<BufReader<TcpStream>>,
    tui_rx: mpsc::Receiver<TuiToAgent>,
    agent_tx: mpsc::Sender<AgentToTui>,
) {
    // Drain HP register response
    if let Some(ref mut hp) = hp_conn {
        let _: Option<HpToAgent> = dsx_proto::read_frame(hp).ok().flatten();
    }

    loop {
        let frame: TuiToAgent = match tui_rx.recv() {
            Ok(f) => f,
            Err(_) => break,
        };

        eprintln!(
            "dsx-agent: tui ← {:?}",
            std::mem::discriminant(&frame)
        );

        match frame {
            TuiToAgent::UserInput { text } => {
                // Respawn tools if IPC was lost
                if crate::tools::all_tools().is_empty() {
                    eprintln!("dsx-agent: tools IPC dead, respawning...");
                    let mut tools_opt: Option<std::process::Child> = None;
                    if crate::tools_spawn::respawn(&mut tools_opt) {
                        agent.tool_defs = crate::tools::all_tools();
                        eprintln!(
                            "dsx-agent: tools IPC restored ({} tools)",
                            agent.tool_defs.len()
                        );
                    } else {
                        eprintln!("dsx-agent: tools respawn FAILED");
                    }
                }
                // Process input — if HP not connected or fails, try reconnect once
                let hp_failed = if let Some(ref mut hp) = hp_conn {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || turn::handle_user_input(&mut agent, &text, hp, &agent_tx),
                    ));
                    result.is_err()
                } else {
                    true
                };

                if hp_failed {
                    eprintln!("dsx-agent: HP failed, reconnecting...");
                    if let Some(stream) = crate::hp::try_reconnect() {
                        let reader = BufReader::new(stream);
                        hp_conn = Some(reader);
                        eprintln!("dsx-agent: HP reconnected, retry input");
                        if let Some(ref mut hp) = hp_conn {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || turn::handle_user_input(&mut agent, &text, hp, &agent_tx),
                            ));
                        }
                    } else {
                        eprintln!("dsx-agent: HP reconnect failed");
                        let _ = agent_tx.send(AgentToTui::Error {
                            message: "HP disconnected. Please try again.".into(),
                        });
                    }
                }
                let _ = agent_tx.send(AgentToTui::Done);
            }

            TuiToAgent::ToolCall {
                id: _,
                name,
                action,
                args,
            } => {
                let args_str = args.to_string();
                let content = crate::tools::execute_tool(&name, &action, &args_str);
                let _ = agent_tx.send(AgentToTui::ApiResponse {
                    content,
                    reasoning_content: None,
                    tool_calls: None,
                    stop_reason: None,
                    usage: None,
                });
                let _ = agent_tx.send(AgentToTui::Done);
            }

            TuiToAgent::SetPhase { phase } => {
                let task_phase = match phase.as_str() {
                    "plan" => dsx_types::TaskPhase::Plan,
                    "coding" | "code" => dsx_types::TaskPhase::Coding,
                    "debug" => dsx_types::TaskPhase::Debug,
                    _ => dsx_types::TaskPhase::Coding,
                };
                agent.current_task_phase = task_phase;
                router::set_phase(task_phase, router::read_debug_level());
                let _ = agent_tx.send(AgentToTui::PhaseChanged { phase });
            }

            TuiToAgent::ToolConfirm { .. } => {}

            TuiToAgent::Cancel => {
                crate::tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                agent.stream_cancelled = true;
                crate::tools::cancel_current_tool();
            }

            TuiToAgent::Shutdown => {
                maybe_save_session(&mut agent);
                let _ = agent_tx.send(AgentToTui::ShutdownAck);
                break;
            }

            _ => {}
        }
    }

    // ── Cleanup ──
    crate::tools::shutdown_tools();

    agent.maybe_save_session();

    if let Some(ref mut hp) = hp_conn {
        let unreg = AgentToHp::Unregister {
            pid: std::process::id(),
        };
        let _ = dsx_proto::write_frame(hp.get_mut(), &unreg);
    }

    eprintln!(
        "dsx-agent: shutdown complete (session {}, {} turns, {} tokens)",
        agent.session_seed,
        agent.ctx.turn_count(),
        agent.session_tokens
    );
}

// ── Backward-compat headless mode (stdin/stdout pipes) ──

pub fn run() {
    eprintln!("dsx-agent starting (headless mode)");

    // ── 1. Initialize logging ──
    crate::dsx_log::init();

    // ── 2. Load configuration ──
    let config = crate::config::Config::load().unwrap_or_default();
    eprintln!(
        "dsx-agent: model={} effort={:?} context_limit={}",
        config.model, config.effort, config.context_limit
    );

    // ── 3. Parse CLI args ──
    let args: Vec<String> = std::env::args().collect();
    let resume_seed = args
        .windows(2)
        .find(|w| w[0] == "--session")
        .and_then(|w| Some(w[1].clone()));
    if let Some(ref seed) = resume_seed {
        eprintln!("dsx-agent: resume request for session {seed}");
    }

    // ── 4. Initialize AgentState ──
    let mut agent = AgentState::new(config);
    agent.resume_seed = resume_seed;
    agent.health.context_limit = agent.config.context_limit;

    // ── 5. Connect to HP ──
    let hp_conn = crate::hp::connect().map(BufReader::new);

    // ── 6. Spawn dsx-tools ──
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("dsx"));
    let (tools_child, mut tools_reader, mut tools_writer) =
        crate::tools_spawn::spawn_process(&exe);
    let mut tools_option = Some(tools_child);

    // Send init frame and read Ready response
    let init = AgentToTools::Init {
        allowed_tools: vec![],
        session_seed: "pipe".into(),
        auto_mode: agent.auto_mode,
    };
    let _ = dsx_proto::write_frame(&mut tools_writer, &init);
    let ready: Option<ToolsToAgent> = dsx_proto::read_frame(&mut tools_reader).ok().flatten();
    if let Some(ToolsToAgent::Ready { tools }) = &ready {
        agent.tool_defs = tools.clone();
        eprintln!(
            "dsx-agent: tools → {}",
            agent.tool_defs.len(),
        );
    }

    crate::tools::init_tools_ipc(tools_reader, tools_writer, agent.tool_defs.clone());

    // ── 7. Session check ──
    let lives = crate::session::find_live_sessions();
    if !lives.is_empty() {
        eprintln!(
            "dsx-agent: {} live session(s) available for resume",
            lives.len()
        );
    }

    // ── 8. Create channels + adapter threads ──
    let (tui_tx, tui_rx) = mpsc::channel::<TuiToAgent>();
    let (agent_tx, agent_rx) = mpsc::channel::<AgentToTui>();

    // Thread: read TuiToAgent frames from stdin, forward to channel
    let stdin_handle = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match dsx_proto::read_frame::<TuiToAgent>(&mut reader) {
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

    // Thread: receive AgentToTui frames from channel, write JSON-LP to stdout
    let stdout_handle = std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        while let Ok(frame) = agent_rx.recv() {
            if dsx_proto::write_frame(&mut stdout, &frame).is_err() {
                break;
            }
        }
    });

    // ── 9. Run agent loop ──
    run_agent_loop(agent, hp_conn, tui_rx, agent_tx);

    // ── 10. Cleanup ──
    if let Some(mut c) = tools_option.take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    stdin_handle.join().ok();
    stdout_handle.join().ok();
}
