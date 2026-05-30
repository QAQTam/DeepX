//! Session persistence: save/load/resume and crash recovery snapshots.
//!
//! # Storage format
//!
//! Sessions are persisted as [`SessionFile`] on disk, containing
//! a flat `Vec<Message>` alongside metadata (seed, timestamps, model).
//! This internal `Vec<Message>` format is the canonical conversation
//! representation — conversion to API format happens at the gateway layer.

use std::path::PathBuf;
use dsx_types::Message;

mod persist;
mod restore;

// ── Re-exports ──
pub use persist::{
    finalize_session,
    load_session,
    load_session_or_live,
    save_live_snapshot,
    save_session,
};
pub use restore::{find_live_sessions};

// ── Seed generation ──

pub fn generate_seed() -> String {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut h = DefaultHasher::new();
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .hash(&mut h);
    std::process::id().hash(&mut h);
    let v = h.finish();
    let mixed = (v as u32) ^ ((v >> 32) as u32);
    format!("{:08x}", mixed)
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Date helper (crate-internal) ──

pub(crate) fn chrono_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ── Paths ──

pub fn sessions_dir() -> Option<PathBuf> {
    Some(dsx_types::platform::sessions_dir())
}

/// Directory for a single session's data.
/// For new sessions: creates path with today's date.
/// For existing sessions: finds the existing directory matching seed prefix.
pub fn session_dir(seed: &str) -> Option<PathBuf> {
    let base = sessions_dir()?;
    // 1. Look for existing directory matching {seed}-*
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(seed) && entry.path().is_dir() {
                return Some(entry.path());
            }
        }
    }
    // 2. New session: create with today's date
    let date = chrono_date();
    Some(base.join(format!("{}-{}", seed, date)))
}

pub fn session_path(seed: &str) -> Option<PathBuf> {
    if let Some(dir) = session_dir(seed) {
        let new_path = dir.join("session.json");
        if new_path.exists() || dir.parent().map_or(false, |p| p.exists()) {
            return Some(new_path);
        }
        // Auto-migrate: move old flat file into new directory
        let old_path = sessions_dir()?.join(format!("{}.json", seed));
        if old_path.exists() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::rename(&old_path, &new_path);
            return Some(new_path);
        }
    }
    // Fallback: old flat path
    sessions_dir().map(|d| d.join(format!("{}.json", seed)))
}

pub fn live_path(seed: &str) -> Option<PathBuf> {
    session_path(seed)
}

pub fn index_path() -> Option<PathBuf> {
    sessions_dir().map(|d| d.join("index.json"))
}

// ── Shared helpers (crate-internal) ──

pub(crate) fn extract_last_summary(messages: &[Message]) -> String {
    messages.iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.is_empty())
        .and_then(|m| m.content.iter().find_map(|b| {
            if let dsx_types::ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        }))
        .map(|c| {
            let first = c.lines().next().unwrap_or(c);
            safe_truncate(first, 80)
        })
        .unwrap_or_default()
}

pub(crate) fn safe_truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes { return s.to_string(); }
    let end = s.floor_char_boundary(max_bytes);
    format!("{}…", &s[..end])
}
