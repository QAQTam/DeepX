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
        .unwrap_or_default()
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
            if name.starts_with(&format!("{}-", seed)) && entry.path().is_dir() {
                return Some(entry.path());
            }
        }
    }
    // 2. New session: create with today's date
    let date = chrono_date();
    Some(base.join(format!("{}-{}", seed, date)))
}

pub fn session_path(seed: &str) -> Option<PathBuf> {
    let dir = session_dir(seed)?;
    let new_path = dir.join("session.json");

    let base = sessions_dir()?;
    let _ = std::fs::create_dir_all(&base);

    let old_path = base.join(format!("{}.json", seed));
    if old_path.exists() {
        if new_path.exists() {
            let _ = std::fs::remove_file(&old_path);
        } else {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::rename(&old_path, &new_path);
        }
    }

    Some(new_path)
}

pub fn live_path(seed: &str) -> Option<PathBuf> {
    session_path(seed)
}

/// Find an existing session file on disk without creating new directories.
/// Returns None if no session file exists for this seed.
pub fn find_existing_session_path(seed: &str) -> Option<PathBuf> {
    let base = sessions_dir()?;
    // 1. Look for {seed}-{date}/session.json
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{}-", seed)) && entry.path().is_dir() {
                let p = entry.path().join("session.json");
                if p.exists() { return Some(p); }
            }
        }
    }
    // 2. Legacy flat file {seed}.json
    let old = base.join(format!("{}.json", seed));
    if old.exists() { return Some(old); }
    None
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
