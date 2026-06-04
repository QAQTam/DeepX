//! dsx-agent runner — main event loop and headless adapter.

pub mod lifecycle;
pub mod gate_bridge;
pub mod ui_emit;
pub mod api_turn;
pub mod turn;
pub mod headless;
pub use headless::run;

use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc;

use dsx_proto::{self, AgentToHp, Agent2Ui, DocInfo, TaskInfo, HpToAgent, Ui2Agent};

use crate::agent::AgentState;
use crate::orchestrator::{maybe_save_session, learning};

pub(super) fn build_documents(agent: &AgentState) -> Vec<DocInfo> {
    let mut docs: Vec<DocInfo> = agent.file_read_at.iter()
        .filter(|(path, _)| agent.is_file_stale(path))
        .map(|(path, &read_at)| {
            DocInfo {
                tag: learning::doc_tag(path),
                path: path.clone(),
                turns_since_read: agent.current_turn.saturating_sub(read_at),
                is_stale: true,
            }
        })
        .collect();
    docs.sort_by(|a, b| b.turns_since_read.cmp(&a.turns_since_read));
    docs.truncate(20);
    docs
}

pub(super) fn build_recent_edits(agent: &AgentState) -> Vec<String> {
    agent.tool_results.iter().rev()
        .filter(|(name, _)| matches!(name.as_str(), "write_file" | "edit_file" | "delete_file"))
        .take(10)
        .map(|(name, result)| {
            let path = result.lines().nth(1).unwrap_or("?").trim();
            format!("{}: {}", name, path)
        })
        .collect()
}

pub(super) fn build_tasks(agent: &AgentState) -> Vec<TaskInfo> {
    if agent.session_seed.is_empty() { return Vec::new(); }
    let sessions_dir = std::path::PathBuf::from(dsx_types::platform::sessions_dir());
    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(&sessions_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !dir_name.starts_with(&agent.session_seed) { continue; }
        let task_file = path.join("tasks-mem.md");
        if !task_file.is_file() { continue; }
        let Ok(content) = std::fs::read_to_string(&task_file) else { continue };
        for line in content.lines() {
            let trimmed = line.trim_start_matches("- ");
            let status = if trimmed.contains("[pending]") { "pending" }
                else if trimmed.contains("[in_progress]") { "in_progress" }
                else if trimmed.contains("[completed]") { "completed" }
                else if trimmed.contains("[cancelled]") { "cancelled" }
                else { continue };
            let task_text = trimmed
                .replacen("[pending] ", "", 1)
                .replacen("[in_progress] ", "", 1)
                .replacen("[completed] ", "", 1)
                .replacen("[cancelled] ", "", 1);
            let (subject, description) = task_text.split_once(" — ").unwrap_or((&task_text, ""));
            tasks.push(TaskInfo {
                subject: subject.trim().to_string(),
                description: description.trim().to_string(),
                status: status.to_string(),
            });
        }
        break; // only first matching session dir
    }
    tasks
}

pub(super) fn cache_tokens(agent: &AgentState) -> (u32, u32) {
    agent.api_usage.as_ref()
        .map(|u| (u.prompt_cache_hit_tokens, u.prompt_cache_miss_tokens))
        .unwrap_or((0, 0))
}

pub fn run_agent_loop(
    mut agent: AgentState,
    mut hp_conn: Option<BufReader<TcpStream>>,
    tui_rx: mpsc::Receiver<Ui2Agent>,
    agent_tx: mpsc::Sender<Agent2Ui>,
) {
    crate::skills::init();

    if let Some(ref mut hp) = hp_conn {
        let _: Option<HpToAgent> = dsx_proto::read_frame(hp).ok().flatten();
    }

                let _ = agent_tx.send(Agent2Ui::DebugSnapshot {
                    hp_connected: hp_conn.is_some(),
                    session_seed: agent.session_seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.tool_calls_this_turn,
                    tool_failures: agent.tool_failures as u32,
                    current_phase: "single".to_string(),
                    streaming: false,
                    dsml_compat_count: agent.dsml_compat_count,
                    documents: build_documents(&agent),
        recent_edits: build_recent_edits(&agent),
        tasks: build_tasks(&agent),
          session_title: agent.session_title.clone(),
          prompt_cache_hit_tokens: cache_tokens(&agent).0,
          prompt_cache_miss_tokens: cache_tokens(&agent).1,
                  });

    if agent.session_seed.is_empty() {
        let seed = agent.resume_seed.clone();
        lifecycle::init_session(&mut agent, seed.as_deref());
        if agent.resume_seed.is_some() {
            let msg_count = agent.ctx.message_count();
            let summary = agent.ctx.turns().last()
                .and_then(|t| t.steps.last())
                .and_then(|s| {
                    s.assistant.content.iter().find_map(|b| {
                        if let dsx_types::ContentBlock::Text { text } = b {
                            Some(text.chars().take(100).collect::<String>())
                        } else { None }
                    })
                })
                .unwrap_or_default();
            let _ = agent_tx.send(Agent2Ui::SessionRestored {
                seed: agent.session_seed.clone(),
                message_count: msg_count as u64,
                summary,
                tokens_used: agent.token_estimate,
                cache_hit_pct: 0.0,
            });
        }
    }

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
                let hp_failed = if let Some(ref mut hp) = hp_conn {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || turn::handle_user_input(&mut agent, &text, hp, &agent_tx),
                    ));
                    result.is_err()
                } else {
                    true
                };

                if hp_failed {
                    log::warn!("dsx-agent: gate failed, reconnecting...");
                    let _ = agent_tx.send(Agent2Ui::Error {
                        message: "gate disconnected. Attempting reconnect...".into(),
                    });
                    if let Some(stream) = crate::gate::try_reconnect() {
                        let reader = BufReader::new(stream);
                        hp_conn = Some(reader);
                        log::info!("dsx-agent: gate reconnected, retry input");
                        if let Some(ref mut hp) = hp_conn {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || turn::handle_user_input(&mut agent, &text, hp, &agent_tx),
                            ));
                        }
                    } else {
                        log::error!("dsx-agent: gate reconnect failed");
                        let _ = agent_tx.send(Agent2Ui::Error {
                            message: "gate disconnected. Please try again.".into(),
                        });
                    }
                }
                let _ = agent_tx.send(Agent2Ui::DebugSnapshot {
                    hp_connected: hp_conn.is_some(),
                    session_seed: agent.session_seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.tool_calls_this_turn,
                    tool_failures: agent.tool_failures as u32,
                    current_phase: "single".to_string(),
                    streaming: false,
                    dsml_compat_count: agent.dsml_compat_count,
                    documents: build_documents(&agent),
        recent_edits: build_recent_edits(&agent),
        tasks: build_tasks(&agent),
          session_title: agent.session_title.clone(),
          prompt_cache_hit_tokens: cache_tokens(&agent).0,
          prompt_cache_miss_tokens: cache_tokens(&agent).1,
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
                    tool_id: id.clone(),
                    output: content,
                    success,
                    file: None,
                });
                let _ = agent_tx.send(Agent2Ui::Done);
            }

            Ui2Agent::Cancel => {
                agent.pending_ask_user = None;
                dsx_tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                agent.stream_cancelled = true;
                crate::tools::cancel_current_tool();
                let _ = agent_tx.send(Agent2Ui::Cancelled);
            }

            Ui2Agent::ReloadConfig => {
                if let Ok(cfg) = crate::config::Config::load() {
                    agent.config.api_key = cfg.api_key;
                    agent.config.model = cfg.model;
                    agent.config.effort = cfg.effort;
                    agent.config.max_tokens = cfg.max_tokens;
                    agent.config.context_limit = cfg.context_limit;
                    agent.config.lang = cfg.lang;
                    agent.health.context_limit = cfg.context_limit;
                    if let Some(ref key) = cfg.context7_api_key {
                        if !key.is_empty() {
                            crate::tools::set_context7_key(key);
                        }
                    }
                    crate::tools::load_workspace(&agent.session_seed);
                    log::info!("dsx-agent: config reloaded");
                }
            }

            Ui2Agent::Shutdown => {
                maybe_save_session(&mut agent);
                let _ = agent_tx.send(Agent2Ui::ShutdownAck);
                break;
            }

            Ui2Agent::DebugCommand { cmd } => {
                let _ = agent_tx.send(Agent2Ui::DebugSnapshot {
                    hp_connected: hp_conn.is_some(),
                    session_seed: agent.session_seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.tool_calls_this_turn,
                    tool_failures: agent.tool_failures as u32,
                    current_phase: "single".to_string(),
                    streaming: false,
                    dsml_compat_count: agent.dsml_compat_count,
                    documents: build_documents(&agent),
        recent_edits: build_recent_edits(&agent),
        tasks: build_tasks(&agent),
          session_title: agent.session_title.clone(),
          prompt_cache_hit_tokens: cache_tokens(&agent).0,
          prompt_cache_miss_tokens: cache_tokens(&agent).1,
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

    crate::tools::shutdown_tools();
    crate::gate::kill_hp_daemon();

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