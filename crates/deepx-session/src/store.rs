//! Low-level JSONL I/O for session persistence.
//!
//! Each session directory contains:
//!   meta.json      — session metadata (small, atomic replace-write)
//!   messages.jsonl — one JSON line per Message, append-only
//!
//! A central `index.json` in the sessions root enables fast listing
//! without scanning every session directory.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use deepx_types::{Message, SessionMeta};

// ── Meta ──

/// Write session metadata to `meta.json` atomically (write to temp, rename).
pub fn write_meta(session_dir: &Path, meta: &SessionMeta) -> Result<(), String> {
    let tmp = session_dir.join(".meta.tmp");
    let dst = session_dir.join("meta.json");
    let json = serde_json::to_string_pretty(meta).map_err(|e| format!("serialize meta: {e}"))?;
    fs::write(&tmp, &json).map_err(|e| format!("write meta tmp: {e}"))?;
    fs::rename(&tmp, &dst).map_err(|e| format!("rename meta: {e}"))?;
    Ok(())
}

/// Read session metadata from `meta.json`.
pub fn read_meta(session_dir: &Path) -> Option<SessionMeta> {
    let path = session_dir.join("meta.json");
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

// ── Messages (JSONL) ──

/// Append a single message as a JSON line to `messages.jsonl`.
/// Used for immediate per-message persistence.
pub fn append_one(session_dir: &Path, msg: &Message) -> Result<(), String> {
    let path = session_dir.join("messages.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open messages.jsonl: {e}"))?;
    let line = serde_json::to_string(msg).map_err(|e| format!("serialize message: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("write message: {e}"))?;
    file.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

/// Append messages as JSON lines to `messages.jsonl`.
/// Creates the file if it doesn't exist.
pub fn append_messages(session_dir: &Path, messages: &[Message]) -> Result<(), String> {
    let path = session_dir.join("messages.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open messages.jsonl: {e}"))?;
    for msg in messages {
        let line = serde_json::to_string(msg).map_err(|e| format!("serialize message: {e}"))?;
        writeln!(file, "{line}").map_err(|e| format!("write message: {e}"))?;
    }
    file.flush().map_err(|e| format!("flush messages: {e}"))?;
    Ok(())
}

/// Read all messages from `messages.jsonl`.
/// Returns empty vec if the file doesn't exist.
/// Deduplicates by msg_id: if the same msg_id appears more than once
/// (from a prior bug where from_messages re-persisted, fixed in v0.4.1),
/// only the first occurrence is kept.
pub fn read_messages(session_dir: &Path) -> Result<Vec<Message>, String> {
    let path = session_dir.join("messages.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path).map_err(|e| format!("open messages.jsonl: {e}"))?;
    let reader = BufReader::new(file);
    let mut msgs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    let mut dup_count = 0u32;
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("read line {i}: {e}"))?;
        if line.trim().is_empty() { continue; }
        let msg: Message = serde_json::from_str(&line)
            .map_err(|e| format!("parse line {i}: {e}"))?;
        // Skip duplicate msg_ids (prior bug: from_messages re-persisted).
        if let Some(mid) = msg.msg_id {
            if !seen_ids.insert(mid) {
                dup_count += 1;
                continue;
            }
        }
        msgs.push(msg);
    }
    if dup_count > 0 {
        log::warn!("[read_messages] skipped {dup_count} duplicate messages (msg_id collision) — will rewrite cleanly on next save_full");
    }
    Ok(msgs)
}

/// Rewrite the entire messages.jsonl with the given messages.
/// Used after undo or compact.
pub fn rewrite_messages(session_dir: &Path, messages: &[Message]) -> Result<(), String> {
    let tmp = session_dir.join(".messages.tmp");
    let dst = session_dir.join("messages.jsonl");
    {
        let mut file = fs::File::create(&tmp).map_err(|e| format!("create tmp: {e}"))?;
        for msg in messages {
            let line = serde_json::to_string(msg).map_err(|e| format!("serialize: {e}"))?;
            writeln!(file, "{line}").map_err(|e| format!("write: {e}"))?;
        }
        file.flush().map_err(|e| format!("flush: {e}"))?;
    }
    fs::rename(&tmp, &dst).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Truncate messages.jsonl to the first `keep_lines` lines.
/// Returns the truncated messages.
pub fn truncate_messages(session_dir: &Path, keep_lines: usize) -> Result<Vec<Message>, String> {
    let path = session_dir.join("messages.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let all = read_messages(session_dir)?;
    let truncated: Vec<Message> = all.into_iter().take(keep_lines).collect();
    rewrite_messages(session_dir, &truncated)?;
    Ok(truncated)
}

/// Count lines in messages.jsonl (fast, reads line-by-line without parsing JSON).
pub fn count_message_lines(session_dir: &Path) -> Result<usize, String> {
    let path = session_dir.join("messages.jsonl");
    if !path.exists() { return Ok(0); }
    let file = fs::File::open(&path).map_err(|e| format!("open: {e}"))?;
    let reader = BufReader::new(file);
    Ok(reader.lines().count())
}

// ── Index ──

/// Read the central session index.
pub fn read_index(sessions_dir: &Path) -> Vec<SessionMeta> {
    let path = sessions_dir.join("index.json");
    let Ok(data) = fs::read_to_string(&path) else { return vec![] };
    serde_json::from_str(&data).unwrap_or_default()
}

/// Write the central session index atomically.
pub fn write_index(sessions_dir: &Path, metas: &[SessionMeta]) {
    let Ok(json) = serde_json::to_string_pretty(metas) else { return };
    let tmp = sessions_dir.join(".index.tmp");
    let dst = sessions_dir.join("index.json");
    let _ = fs::write(&tmp, &json);
    let _ = fs::rename(&tmp, &dst);
}

/// Upsert a single session meta into the index (avoids full rewrite).
pub fn upsert_index(sessions_dir: &Path, meta: &SessionMeta) {
    let mut index = read_index(sessions_dir);
    if let Some(existing) = index.iter_mut().find(|m| m.seed == meta.seed) {
        *existing = meta.clone();
    } else {
        index.push(meta.clone());
    }
    write_index(sessions_dir, &index);
}

/// Remove a session from the index.
pub fn remove_from_index(sessions_dir: &Path, seed: &str) {
    let mut index = read_index(sessions_dir);
    index.retain(|m| m.seed != seed);
    write_index(sessions_dir, &index);
}
