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

use dsx_proto::{self, AgentToHp, Agent2Ui, HpToAgent, Ui2Agent};

use crate::agent::AgentState;
use crate::orchestrator::maybe_save_session;
use crate::router;

// ── Channel-based main loop (primary entry point, called by tui.rs) ──

pub fn run_agent_loop(
    mut agent: AgentState,
    mut hp_conn: Option<BufReader<TcpStream>>,
    tui_rx: mpsc::Receiver<Ui2Agent>,
    agent_tx: mpsc::Sender<Agent2Ui>,
) {
    // Drain HP register response
    if let Some(ref mut hp) = hp_conn {
        let _: Option<HpToAgent> = dsx_proto::read_frame(hp).ok().flatten();
    }

    let _ = agent_tx.send(Agent2Ui::DebugSnapshot {
        hp_connected: hp_conn.is_some(),
        session_seed: agent.session_seed.clone(),
        context_tokens: agent.token_estimate,
        tool_calls_total: agent.tool_calls_this_turn,
        tool_failures: agent.tool_failures as u32,
        current_phase: format!("{:?}", agent.current_task_phase).to_lowercase(),
        streaming: false,
    });

    loop {
        let frame: Ui2Agent = match tui_rx.recv() {
            Ok(f) => f,
            Err(_) => break,
        };

        log::debug!(
            "dsx-agent: tui ← {:?}",
            std::mem::discriminant(&frame)
        );

        match frame {
            Ui2Agent::UserInput { text } => {
                // Tools are now in-process — always available, no respawn needed
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
                    log::warn!("dsx-agent: HP failed, reconnecting...");
                    let _ = agent_tx.send(Agent2Ui::Error {
                        message: "HP disconnected. Attempting reconnect...".into(),
                    });
                    if let Some(stream) = crate::hp::try_reconnect() {
                        let reader = BufReader::new(stream);
                        hp_conn = Some(reader);
                        log::info!("dsx-agent: HP reconnected, retry input");
                        if let Some(ref mut hp) = hp_conn {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || turn::handle_user_input(&mut agent, &text, hp, &agent_tx),
                            ));
                        }
                    } else {
                        log::error!("dsx-agent: HP reconnect failed");
                        let _ = agent_tx.send(Agent2Ui::Error {
                            message: "HP disconnected. Please try again.".into(),
                        });
                    }
                }
                let _ = agent_tx.send(Agent2Ui::DebugSnapshot {
                    hp_connected: hp_conn.is_some(),
                    session_seed: agent.session_seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.tool_calls_this_turn,
                    tool_failures: agent.tool_failures as u32,
                    current_phase: format!("{:?}", agent.current_task_phase).to_lowercase(),
                    streaming: false,
                });
                let _ = agent_tx.send(Agent2Ui::Done);
            }

            Ui2Agent::ToolCall {
                id,
                name,
                action,
                args,
            } => {
                let args_str = args.to_string();
                let content = crate::tools::execute_tool_with_id(&name, &action, &args_str, &id);
                let success = !content.starts_with("[ERROR]") && !content.starts_with("[FAIL]");
                let _ = agent_tx.send(Agent2Ui::ToolResult {
                    id,
                    name,
                    content,
                    success,
                    args: None,
                });
                let _ = agent_tx.send(Agent2Ui::Done);
            }

            Ui2Agent::SetPhase { phase } => {
                let task_phase = match phase.as_str() {
                    "plan" => dsx_types::TaskPhase::Plan,
                    "coding" | "code" => dsx_types::TaskPhase::Coding,
                    "debug" => dsx_types::TaskPhase::Debug,
                    _ => dsx_types::TaskPhase::Coding,
                };
                agent.current_task_phase = task_phase;
                router::set_phase(task_phase, router::read_debug_level());
                let _ = agent_tx.send(Agent2Ui::PhaseChanged { phase });
            }

            Ui2Agent::Cancel => {
                crate::tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                agent.stream_cancelled = true;
                crate::tools::cancel_current_tool();
                let _ = agent_tx.send(Agent2Ui::Cancelled);
            }

            Ui2Agent::Shutdown => {
                maybe_save_session(&mut agent);
                let _ = agent_tx.send(Agent2Ui::ShutdownAck);
                break;
            }

            Ui2Agent::SetAutoMode { auto_mode } => {
                agent.auto_mode = auto_mode;
                crate::tools::AUTO_MODE.store(auto_mode, std::sync::atomic::Ordering::Relaxed);
                dsx_tools::AUTO_MODE.store(auto_mode, std::sync::atomic::Ordering::Relaxed);
                log::info!("dsx-agent: auto_mode set to {}", auto_mode);
            }

            Ui2Agent::DebugCommand { cmd } => {
                let _ = agent_tx.send(Agent2Ui::DebugSnapshot {
                    hp_connected: hp_conn.is_some(),
                    session_seed: agent.session_seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.tool_calls_this_turn,
                    tool_failures: agent.tool_failures as u32,
                    current_phase: format!("{:?}", agent.current_task_phase).to_lowercase(),
                    streaming: false,
                });
                if cmd == "dump_context" {
                    let json = serde_json::to_string_pretty(&agent.ctx.to_vec())
                        .unwrap_or_default();
                    let _ = agent_tx.send(Agent2Ui::Error {
                        message: format!("[CONTEXT_DUMP]\n{}", json),
                    });
                }
            }

            _ => {}
        }
    }

    // ── Cleanup ──
    crate::tools::shutdown_tools();
    crate::hp::kill_hp_daemon();

    agent.maybe_save_session();

    if let Some(ref mut hp) = hp_conn {
        let unreg = AgentToHp::Unregister {
            pid: std::process::id(),
        };
        let _ = dsx_proto::write_frame(hp.get_mut(), &unreg);
    }

    log::info!(
        "dsx-agent: shutdown complete (session {}, {} turns, {} tokens)",
        agent.session_seed,
        agent.ctx.turn_count(),
        agent.session_tokens
    );
}

// ── Headless mode (stdin/stdout pipes) ──

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
    let hp_conn = crate::hp::try_reconnect().map(BufReader::new);

    // ── 6. Init in-process tools ──
    crate::tools::init_tools("pipe", agent.auto_mode);
    agent.tool_defs = crate::tools::all_tools();
    eprintln!("dsx-agent: tools → {}", agent.tool_defs.len());

    // ── 7. Session check ──
    let lives = crate::session::find_live_sessions();
    if !lives.is_empty() {
        eprintln!(
            "dsx-agent: {} live session(s) available for resume",
            lives.len()
        );
    }

    // ── 8. Create channels + adapter threads ──
    let (tui_tx, tui_rx) = mpsc::channel::<Ui2Agent>();
    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();

    // Thread: read Ui2Agent frames from stdin, forward to channel
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

    // Thread: receive Agent2Ui frames from channel, write JSON-LP to stdout
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
    crate::tools::shutdown_tools();
    crate::hp::kill_hp_daemon();
    stdin_handle.join().ok();
    stdout_handle.join().ok();
}
