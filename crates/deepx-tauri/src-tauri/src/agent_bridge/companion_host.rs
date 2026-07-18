use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{AppHandle, Emitter, Manager};

use deepx_companion::{
    BeginClaim, CompanionHub, CompanionHubHandle, InteractionCoordinator, InteractionSource,
    PetSupervisor, RestartPolicy, generate_secret_hex, interaction_from_agent_event,
    next_visual_state_for_agent_event, notification_for_agent_event, response_to_agent_frame,
};
use deepx_proto::{
    Agent2Ui, CompanionClientMessage, CompanionCommandStatus, CompanionEvent,
    CompanionInteractionKey, CompanionInteractionKind, CompanionInteractionResponse,
    CompanionSession, CompanionVisualState, SessionActivity, SessionActivityState,
};

use super::registry::send_to_agent_generation;

static COMPANION_HOST: OnceLock<CompanionHost> = OnceLock::new();

pub struct CompanionHost {
    app_handle: AppHandle,
    hub: CompanionHubHandle,
    coordinator: Arc<InteractionCoordinator>,
    sessions: Mutex<HashMap<String, CompanionSession>>,
    supervisor: Option<PetSupervisor>,
    token_file: PathBuf,
}

impl CompanionHost {
    pub fn init(app_handle: &AppHandle) -> Result<(), String> {
        let token = generate_secret_hex();
        let epoch = generate_secret_hex();
        let (hub, mut commands) =
            tauri::async_runtime::block_on(CompanionHub::bind_loopback(token, epoch))
                .map_err(|error| format!("bind companion hub: {error}"))?;
        let endpoint = hub.endpoint().to_string();
        let token_file = write_auth_document(&endpoint, hub.token())?;
        let supervisor = start_companion_supervisor(&token_file)
            .map_err(|error| {
                log::warn!("[COMPANION] desktop pet was not started: {error}");
                error
            })
            .ok();
        let host = CompanionHost {
            app_handle: app_handle.clone(),
            hub,
            coordinator: Arc::new(InteractionCoordinator::default()),
            sessions: Mutex::new(HashMap::new()),
            supervisor,
            token_file,
        };
        COMPANION_HOST
            .set(host)
            .map_err(|_| "CompanionHost already initialized".to_string())?;
        tauri::async_runtime::spawn(async move {
            while let Some(command) = commands.recv().await {
                handle_client_command(command);
            }
        });
        log::info!("[COMPANION] listening on {endpoint}");
        Ok(())
    }

    pub fn get() -> Option<&'static CompanionHost> {
        COMPANION_HOST.get()
    }

    pub fn endpoint(&self) -> &str {
        self.hub.endpoint()
    }

    pub fn token(&self) -> &str {
        self.hub.token()
    }

    pub fn shutdown() {
        if let Some(host) = Self::get() {
            tauri::async_runtime::block_on(host.hub.shutdown());
            if let Some(supervisor) = &host.supervisor {
                supervisor.shutdown();
            }
            let _ = std::fs::remove_file(&host.token_file);
        }
    }

    fn publish_agent_event(
        &self,
        seed: &str,
        generation: u64,
        payload: &serde_json::Value,
        activity: &SessionActivity,
    ) {
        let mut expired = self.coordinator.advance_generation(seed, generation);
        if activity.state == SessionActivityState::Disconnected {
            expired.extend(self.coordinator.invalidate_generation(seed, generation));
        }
        for key in expired {
            let _ = self.app_handle.emit("companion-interaction-resolved", &key);
            self.hub.publish(CompanionEvent::InteractionResolved {
                key,
                resolution: "expired".into(),
            });
        }
        let event_type = payload
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let typed = serde_json::from_value::<Agent2Ui>(payload.clone()).ok();
        if let Some(interaction) = typed
            .as_ref()
            .and_then(|event| interaction_from_agent_event(seed, generation, event))
        {
            self.coordinator.register(interaction.clone());
            self.hub
                .publish(CompanionEvent::InteractionRequested { interaction });
        }
        if let Some(event) = typed.as_ref() {
            let resolved = match event {
                Agent2Ui::AskResolved { ask_id, resolution } => Some((
                    CompanionInteractionKind::AskUser,
                    ask_id.clone(),
                    format!("{resolution:?}").to_lowercase(),
                )),
                Agent2Ui::PlanResolved { call_id, approved } => Some((
                    CompanionInteractionKind::PlanReview,
                    call_id.clone(),
                    if *approved { "approved" } else { "rejected" }.into(),
                )),
                _ => None,
            };
            if let Some((kind, request_id, resolution)) = resolved {
                let key = CompanionInteractionKey {
                    seed: seed.to_string(),
                    generation,
                    kind,
                    request_id,
                };
                self.coordinator.resolve(&key);
                self.hub
                    .publish(CompanionEvent::InteractionResolved { key, resolution });
            }
        }

        let previous_visual_state = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(seed)
            .map(|session| session.visual_state);
        let visual_state = next_visual_state_for_agent_event(
            event_type,
            payload,
            previous_visual_state,
            default_visual_state(activity.state),
        );
        if let Some((level, message)) = notification_for_agent_event(event_type, payload) {
            self.hub.publish(CompanionEvent::Notification {
                seed: Some(seed.to_string()),
                level,
                message,
            });
        }
        let workspace = std::fs::read_to_string(
            deepx_types::platform::sessions_dir()
                .join(seed)
                .join("workspace.txt"),
        )
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
        let session = CompanionSession {
            seed: seed.to_string(),
            title: None,
            workspace,
            state: activity.state,
            visual_state,
            turn_id: activity.turn_id.clone(),
            session_seq: activity.seq,
            updated_at: activity.updated_at,
        };
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if sessions.get(seed) != Some(&session) {
            sessions.insert(seed.to_string(), session.clone());
            self.hub
                .publish(CompanionEvent::SessionActivity { session });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchSpec {
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

fn write_auth_document(endpoint: &str, token: &str) -> Result<PathBuf, String> {
    let directory = deepx_types::platform::data_dir().join("companion");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("create companion data directory: {error}"))?;
    let path = directory.join(format!("auth-{}.json", std::process::id()));
    let body = serde_json::json!({
        "endpoint": endpoint,
        "token": token,
        "parent_pid": std::process::id(),
    });
    let bytes = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
    write_private_file(&path, &bytes)
        .map_err(|error| format!("write companion auth document: {error}"))?;
    Ok(path)
}

#[cfg(unix)]
fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

fn resolve_launch_spec() -> Option<LaunchSpec> {
    let executable = std::env::var_os("DEEPX_COMPANION_EXE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let configured = std::env::var_os("DEEPX_COMPANION_DIR").map(PathBuf::from);
    let development = if cfg!(windows) {
        Some(PathBuf::from(r"E:\clawd-on-desk"))
    } else {
        None
    };
    resolve_launch_spec_from(executable, configured.or(development))
}

fn resolve_launch_spec_from(
    executable: Option<PathBuf>,
    directory: Option<PathBuf>,
) -> Option<LaunchSpec> {
    if let Some(program) = executable {
        let cwd = program
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        return Some(LaunchSpec {
            program,
            args: vec!["--deepx-companion".into()],
            cwd,
        });
    }
    let directory = directory?;
    let launcher = directory.join("launch.js");
    if !launcher.is_file() {
        return None;
    }
    Some(LaunchSpec {
        program: PathBuf::from("node"),
        args: vec![
            launcher.to_string_lossy().into_owned(),
            "--deepx-companion".into(),
        ],
        cwd: directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_override_is_launched_directly_in_companion_mode() {
        let spec =
            resolve_launch_spec_from(Some(PathBuf::from(r"C:\Apps\DeepX Companion.exe")), None)
                .expect("launch spec");
        assert_eq!(spec.program, PathBuf::from(r"C:\Apps\DeepX Companion.exe"));
        assert_eq!(spec.args, vec!["--deepx-companion"]);
        assert_eq!(spec.cwd, PathBuf::from(r"C:\Apps"));
    }

    #[test]
    fn missing_source_launcher_is_not_spawned() {
        let missing = std::env::temp_dir().join(format!("deepx-missing-{}", std::process::id()));
        assert!(resolve_launch_spec_from(None, Some(missing)).is_none());
    }
}

fn start_companion_supervisor(token_file: &Path) -> Result<PetSupervisor, String> {
    if std::env::var_os("DEEPX_COMPANION_DISABLED").is_some() {
        return Err("disabled by DEEPX_COMPANION_DISABLED".into());
    }
    let spec = resolve_launch_spec().ok_or_else(|| {
        "no companion app found; set DEEPX_COMPANION_EXE or DEEPX_COMPANION_DIR".to_string()
    })?;
    let token_file = token_file.to_path_buf();
    Ok(PetSupervisor::start(
        move || {
            Command::new(&spec.program)
                .args(&spec.args)
                .current_dir(&spec.cwd)
                .env("DEEPX_COMPANION_TOKEN_FILE", &token_file)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        },
        RestartPolicy::default(),
    ))
}

pub fn publish_agent_event(
    seed: &str,
    generation: u64,
    payload: &serde_json::Value,
    activity: &SessionActivity,
) {
    if let Some(host) = CompanionHost::get() {
        host.publish_agent_event(seed, generation, payload, activity);
    }
}

pub fn submit_tauri_response(
    seed: &str,
    kind: CompanionInteractionKind,
    request_id: &str,
    response: CompanionInteractionResponse,
) -> Result<(), String> {
    let host = CompanionHost::get().ok_or_else(|| "CompanionHost not initialized".to_string())?;
    let key = host
        .coordinator
        .pending()
        .into_iter()
        .map(|interaction| interaction.key)
        .find(|key| key.seed == seed && key.kind == kind && key.request_id == request_id);
    let Some(key) = key else {
        // The pet or agent already resolved it. Treat the fallback UI's late
        // click as an idempotent no-op instead of surfacing a spurious error.
        return Ok(());
    };
    let command_id = format!("tauri:{}:{}", request_id, generate_secret_hex());
    match submit_response(host, key, response, &command_id, InteractionSource::Tauri) {
        CompanionCommandStatus::Accepted | CompanionCommandStatus::AlreadyResolved => Ok(()),
        status => Err(format!("interaction response rejected: {status:?}")),
    }
}

fn handle_client_command(command: CompanionClientMessage) {
    let Some(host) = CompanionHost::get() else {
        return;
    };
    match command {
        CompanionClientMessage::InteractionResponse {
            command_id,
            key,
            response,
        } => {
            let status = submit_response(host, key, response, &command_id, InteractionSource::Pet);
            host.hub.command_result(command_id, status, None);
        }
        CompanionClientMessage::FocusSession { command_id, seed } => {
            let exists = host
                .sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains_key(&seed);
            if !exists {
                host.hub.command_result(
                    command_id,
                    CompanionCommandStatus::Rejected,
                    Some("unknown session".into()),
                );
                return;
            }
            let _ = host.app_handle.emit("companion-focus-session", &seed);
            if let Some(window) = host.app_handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            host.hub
                .command_result(command_id, CompanionCommandStatus::Accepted, None);
        }
        CompanionClientMessage::ClientHello { .. } | CompanionClientMessage::Heartbeat { .. } => {}
    }
}

fn submit_response(
    host: &CompanionHost,
    key: CompanionInteractionKey,
    response: CompanionInteractionResponse,
    command_id: &str,
    source: InteractionSource,
) -> CompanionCommandStatus {
    let claim = match host.coordinator.begin(&key, command_id, source) {
        BeginClaim::Claimed(claim) => claim,
        BeginClaim::Duplicate(status) | BeginClaim::Rejected(status) => return status,
    };
    let frame = match response_to_agent_frame(&key, response) {
        Ok(frame) => frame,
        Err(_) => {
            host.coordinator.rollback(&claim);
            return CompanionCommandStatus::Rejected;
        }
    };
    if send_to_agent_generation(&key.seed, key.generation, frame).is_err() {
        host.coordinator.rollback(&claim);
        return CompanionCommandStatus::Rejected;
    }
    if !host.coordinator.commit(&claim) {
        return CompanionCommandStatus::Rejected;
    }
    let _ = host.app_handle.emit("companion-interaction-resolved", &key);
    host.hub.publish(CompanionEvent::InteractionResolved {
        key,
        resolution: "answered".into(),
    });
    CompanionCommandStatus::Accepted
}

fn default_visual_state(state: SessionActivityState) -> CompanionVisualState {
    match state {
        SessionActivityState::Starting => CompanionVisualState::Starting,
        SessionActivityState::Idle => CompanionVisualState::Idle,
        SessionActivityState::Working => CompanionVisualState::Working,
        SessionActivityState::WaitingUser => CompanionVisualState::WaitingUser,
        SessionActivityState::Disconnected => CompanionVisualState::Disconnected,
    }
}
