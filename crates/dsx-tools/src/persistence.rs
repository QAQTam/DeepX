//! Session I/O: memory and plan persistence.
//!
//! Memory and plan data is persisted directly to the session filesystem
//! (same paths as dsx-agent), avoiding IPC round-trips for simple
//! read/write operations.

use dsx_types;
use std::path::PathBuf;

// ── Path resolution (same as dsx-agent::session) ──

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn chrono_date() -> String {
    let secs = now_epoch();
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant algorithm: convert days since civil epoch to (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
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

fn sessions_dir() -> Option<PathBuf> {
    Some(dsx_types::platform::sessions_dir())
}

fn session_dir(seed: &str) -> Option<PathBuf> {
    sessions_dir().map(|d| {
        let date = chrono_date();
        d.join(format!("{}-{}", seed, date))
    })
}

fn memory_path(seed: &str, tier: &str) -> Option<PathBuf> {
    session_dir(seed).map(|d| d.join(format!("{}-mem.md", tier)))
}

// ── Memory I/O ──

pub fn read_memory(seed: &str, tier: &str) -> String {
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
    let Some(path) = memory_path(seed, tier) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, content);
}

pub fn append_memory(seed: &str, tier: &str, line: &str) {
    let Some(path) = memory_path(seed, tier) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut existing = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    const MAX_CHARS: usize = 32000;
    if existing.len() > MAX_CHARS {
        let cut = existing.ceil_char_boundary(existing.len().saturating_sub(MAX_CHARS / 2));
        existing = existing[cut..].to_string();
        if let Some(nl) = existing.find('\n') {
            existing = existing[nl + 1..].to_string();
        }
    }

    existing.push_str(line);
    existing.push('\n');
    let _ = std::fs::write(&path, &existing);
}
