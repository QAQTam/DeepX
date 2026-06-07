//! dsx-agent runner — main event loop and headless adapter.

pub mod lifecycle;
pub mod ui_emit;
pub mod api_turn;
pub mod turn;
pub mod headless;
pub use headless::run;

use std::sync::mpsc;

use dsx_proto::{self, Agent2Ui, DocInfo, TaskInfo, Ui2Agent};

use crate::agent::AgentState;
use crate::orchestrator::learning;

/// Emit an Agent2Ui event, logging errors instead of silently dropping.
pub(super) fn emit(tx: &mpsc::Sender<Agent2Ui>, event: Agent2Ui) {
    if let Err(e) = tx.send(event) {
        log::warn!("dsx-agent: failed to emit UI event: {e}");
    }
}

pub(super) fn build_documents(agent: &AgentState) -> Vec<DocInfo> {
    let mut docs: Vec<DocInfo> = agent.files.file_read_at.iter()
        .filter(|(path, _)| agent.is_file_stale(path))
        .map(|(path, &read_at)| {
            DocInfo {
                tag: learning::doc_tag(path),
                path: path.clone(),
                turns_since_read: agent.files.staleness_epoch.saturating_sub(read_at),
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
    if agent.session.seed.is_empty() { return Vec::new(); }
    let sessions_dir = std::path::PathBuf::from(dsx_types::platform::sessions_dir());
    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(&sessions_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !dir_name.starts_with(&agent.session.seed) { continue; }
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
            let (id, rest) = if task_text.trim_start().starts_with("T") {
                let after_marker = task_text.trim_start().strip_prefix("T").unwrap_or("");
                if let Some((num_str, after)) = after_marker.split_once(": ") {
                    (format!("T{}", num_str.trim()), after.trim().to_string())
                } else {
                    ("?".into(), task_text.clone())
                }
            } else {
                ("?".into(), task_text.clone())
            };
            let (subject, description) = rest.split_once(" — ").unwrap_or((&rest, ""));
            tasks.push(TaskInfo {
                id,
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
    tui_rx: mpsc::Receiver<Ui2Agent>,
    agent_tx: mpsc::Sender<Agent2Ui>,
) {
    emit(&agent_tx, Agent2Ui::DebugSnapshot {
        hp_connected: true,
        session_seed: agent.session.seed.clone(),
        context_tokens: agent.token_estimate,
        tool_calls_total: agent.turn.tool_calls_this_turn,
        tool_failures: agent.turn.tool_failures as u32,
        current_phase: "single".to_string(),
        streaming: false,
        dsml_compat_count: agent.dsml_compat_count,
        documents: build_documents(&agent),
        recent_edits: build_recent_edits(&agent),
        tasks: build_tasks(&agent),
        session_title: agent.session.title.clone(),
        prompt_cache_hit_tokens: cache_tokens(&agent).0,
        prompt_cache_miss_tokens: cache_tokens(&agent).1,
    });

    if agent.session.seed.is_empty() && agent.session.resume_seed.is_some() {
        let seed = agent.session.resume_seed.clone();
        let restored = lifecycle::init_session(&mut agent, seed.as_deref());
        if restored && agent.session.from_resume {
            let turns = crate::runner::turn::build_turns_from_context(&agent);
            emit(&agent_tx, Agent2Ui::SessionRestored {
                seed: agent.session.seed.clone(),
                turns,
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
                if agent.session.seed.is_empty() {
                    emit(&agent_tx, Agent2Ui::Error {
                        message: "No session — create one first".into(),
                    });
                    continue;
                }
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || turn::handle_user_input(&mut agent, &text, &agent_tx),
                ));

                emit(&agent_tx, Agent2Ui::DebugSnapshot {
                    hp_connected: true,
                    session_seed: agent.session.seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.turn.tool_calls_this_turn,
                    tool_failures: agent.turn.tool_failures as u32,
                    current_phase: "single".to_string(),
                    streaming: false,
                    dsml_compat_count: agent.dsml_compat_count,
                    documents: build_documents(&agent),
                    recent_edits: build_recent_edits(&agent),
                    tasks: build_tasks(&agent),
                    session_title: agent.session.title.clone(),
                    prompt_cache_hit_tokens: cache_tokens(&agent).0,
                    prompt_cache_miss_tokens: cache_tokens(&agent).1,
                });
                emit(&agent_tx, Agent2Ui::Done);
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
                emit(&agent_tx, Agent2Ui::ToolResults {
                    turn_id: "headless".into(),
                    round_num: 0,
                    results: vec![dsx_proto::ToolResultDef {
                        tool_call_id: id.clone(),
                        output: content,
                        success,
                        file: None,
                    }],
                });
                emit(&agent_tx, Agent2Ui::Done);
            }

            Ui2Agent::CreateSession => {
                if agent.session.seed.is_empty() {
                    lifecycle::create_session(&mut agent);
                    emit(&agent_tx, Agent2Ui::SessionCreated {
                        seed: agent.session.seed.clone(),
                    });
                } else {
                    log::warn!("dsx-agent: CreateSession ignored — session {} already active", agent.session.seed);
                }
            }

            Ui2Agent::Cancel => {
                agent.pending_ask_user = None;
                dsx_tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                agent.turn.stream_cancelled = true;
                crate::tools::cancel_current_tool();
                emit(&agent_tx, Agent2Ui::Cancelled);
            }

            Ui2Agent::ReloadConfig => {
                if let Ok(cfg) = crate::config::Config::load() {
                    agent.config.api_key = cfg.api_key;
                    agent.config.model = cfg.model;
                    agent.config.base_url = cfg.base_url;
                    agent.config.endpoint = cfg.endpoint;
                    agent.config.provider_id = cfg.provider_id;
                    agent.config.reasoning_effort = cfg.reasoning_effort;
                    agent.config.max_tokens = cfg.max_tokens;
                    agent.config.context_limit = cfg.context_limit;
                    agent.config.lang = cfg.lang;
                    if let Some(ref key) = cfg.context7_api_key {
                        if !key.is_empty() {
                            crate::tools::set_context7_key(key);
                        }
                    }
                    crate::tools::load_workspace(&agent.session.seed);
                    log::info!("dsx-agent: config reloaded");
                }
            }

            Ui2Agent::Shutdown => {
                agent.maybe_save_session();
                emit(&agent_tx, Agent2Ui::ShutdownAck);
                break;
            }

            Ui2Agent::DebugCommand { cmd } => {
                emit(&agent_tx, Agent2Ui::DebugSnapshot {
                    hp_connected: true,
                    session_seed: agent.session.seed.clone(),
                    context_tokens: agent.token_estimate,
                    tool_calls_total: agent.turn.tool_calls_this_turn,
                    tool_failures: agent.turn.tool_failures as u32,
                    current_phase: "single".to_string(),
                    streaming: false,
                    dsml_compat_count: agent.dsml_compat_count,
                    documents: build_documents(&agent),
        recent_edits: build_recent_edits(&agent),
        tasks: build_tasks(&agent),
          session_title: agent.session.title.clone(),
          prompt_cache_hit_tokens: cache_tokens(&agent).0,
          prompt_cache_miss_tokens: cache_tokens(&agent).1,
                  });
                if cmd == "dump_context" {
                    let json = serde_json::to_string_pretty(&agent.ctx.to_vec())
                        .unwrap_or_default();
                    emit(&agent_tx, Agent2Ui::Error {
                        message: format!("[CONTEXT_DUMP]\n{}", json),
                    });
                }
            }

            _ => {}
        }
    }

    crate::tools::shutdown_tools();

    agent.maybe_save_session();

    log::info!(
        "dsx-agent: shutdown complete (session {}, {} turns, {} tokens)",
        agent.session.seed,
        agent.ctx.turn_count(),
        agent.session.tokens
    );
}