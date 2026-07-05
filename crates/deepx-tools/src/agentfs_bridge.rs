//! AgentFS bridge — progressive enhancement for `memory` and tool recording.
//!
//! Provides:
//! - `kv` store → replaces `memory` tool persistence
//! - `tools.record` → supplements audit.jsonl with structured tool call tracking
//!
//! The `agentfs` crate (v0.1) is declared as a dependency for future integration.
//! Until the crate exposes a stable public API, the bridge implements storage
//! locally via JSON files in the data directory.
//!
//! All operations are **best-effort** — failures are logged but never propagated.

use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

// ── Storage paths ──

fn kv_dir() -> PathBuf {
    deepx_types::platform::data_dir().join("agentfs").join("kv")
}

fn tools_dir() -> PathBuf {
    deepx_types::platform::data_dir().join("agentfs").join("tools")
}

fn ensure_dir(p: &PathBuf) {
    if let Err(e) = fs::create_dir_all(p) {
        log::error!("agentfs: cannot create dir {}: {e}", p.display());
    }
}

// ── Bridge struct ──

/// Global bridge instance, initialised once at startup via [`init_bridge`].
static BRIDGE: LazyLock<Mutex<Option<AgentFsBridge>>> = LazyLock::new(|| Mutex::new(None));

/// Wraps an AgentFS-compatible kv + tools store.
///
/// Once `agentfs::AgentFS` becomes available, the inner implementation
/// can be swapped to delegate to the real crate.
pub struct AgentFsBridge {
    session_id: String,
}

impl AgentFsBridge {
    /// Create a new bridge for the given session.
    pub fn new(session_id: &str) -> Self {
        let kv_path = kv_dir();
        let tools_path = tools_dir();
        ensure_dir(&kv_path);
        ensure_dir(&tools_path);
        Self {
            session_id: session_id.to_string(),
        }
    }

    /// Set a key-value pair in the kv store.
    ///
    /// Persisted as `<data_dir>/agentfs/kv/{session_id}/{key}.json`.
    pub fn kv_set(&self, key: &str, value: &str) {
        let dir = kv_dir().join(&self.session_id);
        ensure_dir(&dir);
        let path = dir.join(format!("{}.json", sanitise_key(key)));
        if let Err(e) = fs::write(&path, value) {
            log::error!("agentfs: kv_set {} failed: {e}", path.display());
        }
    }

    /// Get a value from the kv store.
    pub fn kv_get(&self, key: &str) -> Option<String> {
        let path = kv_dir()
            .join(&self.session_id)
            .join(format!("{}.json", sanitise_key(key)));
        fs::read_to_string(&path).ok()
    }

    /// Delete a key from the kv store.
    pub fn kv_delete(&self, key: &str) {
        let path = kv_dir()
            .join(&self.session_id)
            .join(format!("{}.json", sanitise_key(key)));
        let _ = fs::remove_file(&path);
    }

    /// Record a tool call via `agent.tools.record`.
    ///
    /// Appended as one JSON line per call to
    /// `<data_dir>/agentfs/tools/{session_id}.jsonl`.
    pub fn record_tool(
        &self,
        name: &str,
        action: &str,
        params_json: &str,
        result: &str,
        elapsed_ms: u64,
    ) {
        let dir = tools_dir();
        ensure_dir(&dir);
        let path = dir.join(format!("{}.jsonl", &self.session_id));
        let record = serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "name": name,
            "action": action,
            "params": params_json,
            "result": result,
            "elapsed_ms": elapsed_ms,
        });
        let line = serde_json::to_string(&record).unwrap_or_default();
        if let Err(e) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()))
        {
            log::error!("agentfs: record_tool failed: {e}");
        }
    }
}

/// Replace characters unsafe for filenames with `_`.
fn sanitise_key(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect()
}

// ── Public API ──

/// Initialise the global AgentFS bridge.
///
/// Must be called once at startup, typically from [`crate::bridge::init_tools`].
pub fn init_bridge(session_id: &str) {
    let bridge = AgentFsBridge::new(session_id);
    *BRIDGE.lock().expect("agentfs: BRIDGE lock") = Some(bridge);
    log::info!("agentfs: bridge initialised (session={session_id})");
}

/// Run a closure with a reference to the global bridge, if initialised.
pub fn with_bridge<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&AgentFsBridge) -> R,
{
    let guard = BRIDGE.lock().ok()?;
    guard.as_ref().map(f)
}

/// Best-effort kv set via AgentFS.
pub fn try_kv_set(key: &str, value: &str) {
    if let Some(b) = BRIDGE.lock().ok().as_ref().and_then(|g| g.as_ref()) {
        b.kv_set(key, value);
    }
}

/// Best-effort kv get via AgentFS.
pub fn try_kv_get(key: &str) -> Option<String> {
    BRIDGE
        .lock()
        .ok()
        .as_ref()
        .and_then(|g| g.as_ref())
        .and_then(|b| b.kv_get(key))
}

/// Best-effort kv delete via AgentFS.
pub fn try_kv_delete(key: &str) {
    if let Some(b) = BRIDGE.lock().ok().as_ref().and_then(|g| g.as_ref()) {
        b.kv_delete(key);
    }
}

/// Best-effort tool recording via AgentFS.
pub fn try_record_tool(
    name: &str,
    action: &str,
    params_json: &str,
    result: &str,
    elapsed_ms: u64,
) {
    if let Some(b) = BRIDGE.lock().ok().as_ref().and_then(|g| g.as_ref()) {
        b.record_tool(name, action, params_json, result, elapsed_ms);
    }
}
