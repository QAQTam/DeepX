//! Session I/O: memory and plan persistence.
//!
//! dsx-tools runs as a subprocess. For memory/plan persistence, it accesses
//! the session filesystem directly (same paths as dsx-agent), avoiding IPC round-trips
//! for simple read/write operations.

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
    let (y, m, d) = civil_from_days(days as i64 + 719468);
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

fn plans_dir() -> Option<PathBuf> {
    Some(dsx_types::platform::plans_dir())
}

fn plan_path(seed: &str, name: &str) -> Option<PathBuf> {
    let slug = slugify(name);
    let date = chrono_date();
    plans_dir().map(|d| d.join(format!("{}-{}-{}.md", seed, slug, date)))
}

fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' { Some(c.to_ascii_lowercase()) }
            else if c.is_alphanumeric() { Some(c) }
            else { Some('-') }
        })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { "plan".to_string() } else { trimmed.to_string() }
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
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, content);
}

pub fn append_memory(seed: &str, tier: &str, line: &str) {
    let Some(path) = memory_path(seed, tier) else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap());

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

// ── Plan I/O ──

pub fn write_plan(seed: &str, name: &str, goal: &str) -> Option<PathBuf> {
    let path = plan_path(seed, name)?;
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let now = now_epoch();
    let content = format!(
        "---\nstatus: draft\ncreated_at: {}\nupdated_at: {}\nsession: {}\n---\n\n# Plan: {}\n\n## Goal\n{}\n\n## Steps\n\n",
        now, now, seed, name, goal
    );
    let _ = std::fs::write(&path, &content);
    Some(path)
}

pub fn update_plan_status(seed: &str, name: &str, new_status: &str) -> Option<String> {
    let path = plan_path(seed, name)?;
    if !path.exists() { return None; }
    let content = std::fs::read_to_string(&path).ok()?;
    let now = now_epoch();
    let mut updated = String::with_capacity(content.len());
    for line in content.lines() {
        if line.starts_with("status: ") {
            updated.push_str(&format!("status: {}", new_status));
        } else if line.starts_with("updated_at: ") {
            updated.push_str(&format!("updated_at: {}", now));
        } else {
            updated.push_str(line);
        }
        updated.push('\n');
    }
    let _ = std::fs::write(&path, &updated);
    Some(updated)
}

pub fn read_plan_content(seed: &str, name: &str) -> Option<String> {
    let path = plan_path(seed, name)?;
    if !path.exists() { return None; }
    std::fs::read_to_string(&path).ok()
}

fn extract_frontmatter_field(content: &str, key: &str) -> String {
    if !content.starts_with("---\n") { return "unknown".to_string(); }
    for line in content.lines().skip(1) {
        if line == "---" { break; }
        if let Some(rest) = line.strip_prefix(&format!("{}: ", key)) {
            return rest.to_string();
        }
    }
    "unknown".to_string()
}

fn extract_plan_name(fname: &str, prefix: &str) -> String {
    let core = fname.strip_prefix(prefix).unwrap_or(fname);
    let core = core.strip_suffix(".md").unwrap_or(core);
    if core.len() > 11 {
        let offset = core.len().saturating_sub(11);
        if core.is_char_boundary(offset) {
            let date_part = &core[offset..];
            if date_part.starts_with('-') && date_part[1..].chars().all(|c| c.is_ascii_digit() || c == '-') {
                return core[..offset].to_string();
            }
        }
    }
    core.to_string()
}

pub fn list_plans(seed: &str) -> Vec<(String, String, PathBuf)> {
    let Some(dir) = plans_dir() else { return vec![] };
    if !dir.exists() { return vec![]; }
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let prefix = format!("{}-", seed);
    let mut plans = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if !fname.starts_with(&prefix) || !fname.ends_with(".md") { continue; }
        let path = entry.path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            let status = extract_frontmatter_field(&content, "status");
            let name = extract_plan_name(&fname, &prefix);
            plans.push((name, status, path));
        }
    }
    plans
}
