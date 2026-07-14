//! Permission dialog and ask_user response commands.

use deepx_proto::Ui2Agent;
use super::super::registry::{ensure_agent, send_to_agent};

#[tauri::command]
pub fn cmd_permission_response(
    seed: String,
    tool_call_id: String,
    approved: bool,
    trust_folder: Option<bool>,
) -> Result<(), String> {
    log::info!("[REGISTRY] permission_response id={tool_call_id} approved={approved}");
    send_to_agent(
        &seed,
        Ui2Agent::PermissionResponse {
            tool_call_id,
            approved,
            trust_folder: trust_folder.unwrap_or(false),
        },
    )
}

/// Send user's answers to an ask_user prompt. Resumes a suspended turn.

#[tauri::command]
pub fn cmd_ask_response(
    seed: String,
    ask_id: String,
    answers: Vec<deepx_proto::AskAnswer>,
) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_ask_response seed={} ask_id={} num_answers={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        ask_id,
        answers.len(),
    );
    ensure_agent(&seed)?;
    send_to_agent(&seed, Ui2Agent::AskResponse { ask_id, answers })
}

/// User dismissed the ask_user dialog without answering. Notifies the agent.

#[tauri::command]
pub fn cmd_ask_dismiss(seed: String, ask_id: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_ask_dismiss seed={} ask_id={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        ask_id,
    );
    ensure_agent(&seed)?;
    send_to_agent(&seed, Ui2Agent::AskDismiss { ask_id })
}
