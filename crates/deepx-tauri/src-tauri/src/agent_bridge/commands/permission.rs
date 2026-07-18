//! Permission dialog and ask_user response commands.

use super::super::companion_host::submit_tauri_response;
use deepx_proto::{CompanionInteractionKind, CompanionInteractionResponse};

#[tauri::command]
pub fn cmd_permission_response(
    seed: String,
    tool_call_id: String,
    approved: bool,
    trust_folder: Option<bool>,
) -> Result<(), String> {
    log::info!("[REGISTRY] permission_response id={tool_call_id} approved={approved}");
    submit_tauri_response(
        &seed,
        CompanionInteractionKind::Permission,
        &tool_call_id,
        CompanionInteractionResponse::Permission {
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
    submit_tauri_response(
        &seed,
        CompanionInteractionKind::AskUser,
        &ask_id,
        CompanionInteractionResponse::AskUser {
            answers,
            dismissed: false,
        },
    )
}

/// User dismissed the ask_user dialog without answering. Notifies the agent.

#[tauri::command]
pub fn cmd_ask_dismiss(seed: String, ask_id: String) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_ask_dismiss seed={} ask_id={}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        ask_id,
    );
    submit_tauri_response(
        &seed,
        CompanionInteractionKind::AskUser,
        &ask_id,
        CompanionInteractionResponse::AskUser {
            answers: Vec::new(),
            dismissed: true,
        },
    )
}

/// Send user's plan review decision. Resumes a turn suspended by plan_submit.

#[tauri::command]
pub fn cmd_plan_review(
    seed: String,
    call_id: String,
    approved: bool,
    message: Option<String>,
) -> Result<(), String> {
    log::info!(
        "[REGISTRY] cmd_plan_review seed={} call_id={} approved={approved}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        call_id,
    );
    submit_tauri_response(
        &seed,
        CompanionInteractionKind::PlanReview,
        &call_id,
        CompanionInteractionResponse::PlanReview {
            approved,
            message: message.unwrap_or_default(),
        },
    )
}
