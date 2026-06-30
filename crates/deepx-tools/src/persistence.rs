//! Session I/O: memory and plan persistence.
//!
//! Memory and plan data is persisted directly to the session filesystem
//! (same paths as SessionManager), avoiding IPC round-trips for simple
//! read/write operations.

use deepx_types;
use std::path::PathBuf;

// ── Path resolution (consistent with deepx-session::SessionManager) ──

fn sessions_dir() -> Option<PathBuf> {
    Some(deepx_types::platform::sessions_dir())
}

fn session_dir(seed: &str) -> Option<PathBuf> {
    sessions_dir().map(|d| d.join(seed))
}

fn memory_path(seed: &str, tier: &str) -> Option<PathBuf> {
    session_dir(seed).map(|d| d.join(format!("{}-mem.md", tier)))
}

// ── Memory I/O ──

static PERSIST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read session-scoped memory (tier = "tasks", "workspace", etc.).
pub fn read_memory(seed: &str, tier: &str) -> String {
    let _lock = PERSIST_LOCK.lock().expect("PERSIST_LOCK");
    let Some(path) = memory_path(seed, tier) else { return String::new(); };
    if !path.exists() { return String::new(); }
    let Ok(content) = std::fs::read_to_string(&path) else { return String::new(); };
    if content.len() > 16000 {
        let start = content.len() - 16000;
        let s = content.ceil_char_boundary(start);
        if let Some(nl) = content[s..].find('\n') {
            content[s + nl + 1..].to_string()
        } else {
            content[s..].to_string()
        }
    } else {
        content
    }
}

pub fn write_memory(seed: &str, tier: &str, content: &str) {
    let _lock = PERSIST_LOCK.lock().expect("PERSIST_LOCK");
    let Some(path) = memory_path(seed, tier) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, content);
}

// ── Global (cross-session) memory ──

fn global_memory_dir() -> PathBuf {
    deepx_types::platform::data_dir().join("memory")
}

fn global_memory_path(scope: &str) -> PathBuf {
    global_memory_dir().join(format!("{}.md", scope))
}

/// Read global memory (cross-session). Returns empty string if not found.
pub fn read_global_memory(scope: &str) -> String {
    let _lock = PERSIST_LOCK.lock().expect("PERSIST_LOCK");
    let path = global_memory_path(scope);
    if !path.exists() { return String::new(); }
    let Ok(content) = std::fs::read_to_string(&path) else { return String::new(); };
    content
}

/// Write global memory (cross-session). Creates parent dirs as needed.
pub fn write_global_memory(scope: &str, content: &str) {
    let _lock = PERSIST_LOCK.lock().expect("PERSIST_LOCK");
    let path = global_memory_path(scope);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, content);
}

/// Append a line to global memory.
pub fn append_global_memory(scope: &str, line: &str) {
    let existing = read_global_memory(scope);
    let mut content = existing.trim().to_string();
    if !content.is_empty() { content.push('\n'); }
    content.push_str(line);
    write_global_memory(scope, &content);
}
