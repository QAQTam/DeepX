use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use deepx_client::DeepxClient;
use deepx_proto::ControlServerMessage;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};

static BRIDGE: OnceLock<BackendBridge> = OnceLock::new();

pub struct BackendBridge {
    app: AppHandle,
    client: Arc<RwLock<Option<DeepxClient>>>,
    last_error: Arc<RwLock<Option<String>>>,
    connect_gate: Arc<Mutex<()>>,
    client_instance_id: String,
    attached: Arc<RwLock<HashSet<String>>>,
    cursor: Arc<RwLock<Option<(String, u64)>>>,
}

impl BackendBridge {
    pub fn init(app: AppHandle) {
        let bridge = Self {
            app: app.clone(),
            client: Arc::new(RwLock::new(None)),
            last_error: Arc::new(RwLock::new(None)),
            connect_gate: Arc::new(Mutex::new(())),
            client_instance_id: format!("desktop-{}", std::process::id()),
            attached: Arc::new(RwLock::new(HashSet::new())),
            cursor: Arc::new(RwLock::new(None)),
        };
        let _ = BRIDGE.set(bridge);
        tauri::async_runtime::spawn(async move {
            let _ = connect_shared().await;
        });
    }

    pub fn release_all() {
        let Some(bridge) = BRIDGE.get() else { return };
        tauri::async_runtime::block_on(async {
            let seeds: Vec<_> = bridge.attached.read().await.iter().cloned().collect();
            if let Some(client) = bridge.client.read().await.clone() {
                for seed in seeds {
                    let _ = client.detach(seed).await;
                }
            }
            bridge.attached.write().await.clear();
        });
    }
}

async fn connect_shared() -> Result<DeepxClient, String> {
    let bridge = BRIDGE
        .get()
        .ok_or_else(|| "backend bridge is not initialized".to_string())?;
    let existing = { bridge.client.read().await.clone() };
    if let Some(client) = existing {
        if client.is_connected() {
            return Ok(client);
        }
        *bridge.client.write().await = None;
    }
    let _gate = bridge.connect_gate.lock().await;
    let existing = { bridge.client.read().await.clone() };
    if let Some(client) = existing {
        if client.is_connected() {
            return Ok(client);
        }
        *bridge.client.write().await = None;
    }
    let daemon_path = daemon_path();
    let cursor = bridge.cursor.read().await.clone();
    match DeepxClient::connect_or_launch_with_state(
        "desktop",
        Some(&daemon_path),
        bridge.client_instance_id.clone(),
        cursor.as_ref().map(|v| v.0.clone()),
        cursor.map(|v| v.1),
    )
    .await
    {
        Ok(client) => {
            *bridge.client.write().await = Some(client.clone());
            *bridge.last_error.write().await = None;
            forward_events(bridge.app.clone(), client.clone());
            // A dropped Desktop connection retains its daemon lease for 15s.
            // Reattach with the stable client id so running tasks continue to
            // stream without waiting for another user command.
            let seeds: Vec<_> = bridge.attached.read().await.iter().cloned().collect();
            for seed in seeds {
                if let Err(error) = client.attach(seed.clone()).await {
                    log::warn!("failed to restore daemon lease for {seed}: {error}");
                }
            }
            let _ = bridge
                .app
                .emit("backend-status", serde_json::json!({"connected":true}));
            Ok(client)
        }
        Err(error) => {
            let message = error.to_string();
            *bridge.last_error.write().await = Some(message.clone());
            let _ = bridge.app.emit(
                "backend-status",
                serde_json::json!({"connected":false,"error":message}),
            );
            Err(message)
        }
    }
}

fn forward_events(app: AppHandle, client: DeepxClient) {
    let mut events = client.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let message = match events.recv().await {
                Ok(message) => message,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    // The websocket is still healthy: only this local Tauri
                    // consumer fell behind the client's broadcast queue. Do
                    // a silent reconnect so the daemon supplies a canonical
                    // Snapshot, but never surface this as a backend outage.
                    log::warn!(
                        "desktop event bridge lagged by {skipped} messages; resyncing from snapshot"
                    );
                    if clear_matching_client(&client, true).await {
                        spawn_reconnect();
                    }
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    ControlServerMessage::Error {
                        request_id: None,
                        code: "disconnected".into(),
                        message: "daemon event channel closed".into(),
                    }
                }
            };
            if let Some((epoch, seq)) = message_cursor(&message) {
                if let Some(bridge) = BRIDGE.get() {
                    *bridge.cursor.write().await = Some((epoch, seq));
                }
            }
            let disconnected =
                matches!(&message,ControlServerMessage::Error{code,..}if code=="disconnected");
            let _ = app.emit("backend-message", &message);
            if disconnected {
                if !clear_matching_client(&client, false).await {
                    break;
                }
                let _ = app.emit(
                    "backend-status",
                    serde_json::json!({"connected":false,"error":"daemon disconnected"}),
                );
                spawn_reconnect();
                break;
            }
        }
    });
}

async fn clear_matching_client(client: &DeepxClient, reset_cursor: bool) -> bool {
    let Some(bridge) = BRIDGE.get() else {
        return false;
    };
    let mut current = bridge.client.write().await;
    if current
        .as_ref()
        .is_some_and(|value| value.same_connection(client))
    {
        *current = None;
        if reset_cursor {
            *bridge.cursor.write().await = None;
        }
        true
    } else {
        false
    }
}

fn spawn_reconnect() {
    tauri::async_runtime::spawn(async {
        loop {
            if connect_shared().await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

fn message_cursor(message: &ControlServerMessage) -> Option<(String, u64)> {
    match message {
        ControlServerMessage::Event {
            server_epoch, seq, ..
        }
        | ControlServerMessage::Snapshot {
            server_epoch, seq, ..
        }
        | ControlServerMessage::Heartbeat {
            server_epoch, seq, ..
        }
        | ControlServerMessage::Shutdown {
            server_epoch, seq, ..
        } => Some((server_epoch.clone(), *seq)),
        _ => None,
    }
}

#[tauri::command]
pub async fn backend_connect() -> Result<(), String> {
    connect_shared().await.map(|_| ())
}

#[tauri::command]
pub async fn backend_request(method: String, params: Value) -> Result<Value, String> {
    connect_shared()
        .await?
        .request(method, params)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn backend_attach(seed: String) -> Result<Value, String> {
    let result = connect_shared()
        .await?
        .attach(seed.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(bridge) = BRIDGE.get() {
        bridge.attached.write().await.insert(seed);
    }
    Ok(result)
}

#[tauri::command]
pub async fn backend_detach(seed: String) -> Result<Value, String> {
    let result = connect_shared()
        .await?
        .detach(seed.clone())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(bridge) = BRIDGE.get() {
        bridge.attached.write().await.remove(&seed);
    }
    Ok(result)
}

#[tauri::command]
pub async fn backend_status() -> Value {
    let Some(bridge) = BRIDGE.get() else {
        return serde_json::json!({"connected":false});
    };
    let connected = bridge
        .client
        .read()
        .await
        .as_ref()
        .is_some_and(DeepxClient::is_connected);
    serde_json::json!({"connected":connected,"error":bridge.last_error.read().await.clone()})
}

fn daemon_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("deepx-daemon"));
    path.set_file_name(if cfg!(windows) {
        "deepx-daemon.exe"
    } else {
        "deepx-daemon"
    });
    path
}
