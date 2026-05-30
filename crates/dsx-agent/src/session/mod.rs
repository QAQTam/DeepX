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
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

// ── Paths ──

pub fn sessions_dir() -> Option<PathBuf> {
    Some(dsx_types::platform::sessions_dir())
}

/// Directory for a single session's data.
pub fn session_dir(seed: &str) -> Option<PathBuf> {
    sessions_dir().map(|d| {
        let date = chrono_date();
        d.join(format!("{}-{}", seed, date))
    })
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
