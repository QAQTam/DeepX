//! Session restore: scan live snapshots + finalized sessions for resume.

use dsx_types::{SessionFile, SessionMeta};

pub fn find_live_sessions() -> Vec<SessionMeta> {
    let mut metas = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // ── 1. Live snapshots (interrupted / in-progress sessions) ──
    if let Some(dir) = super::sessions_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let live_path = if path.is_dir() {
                    path.join("session.live.json")
                } else if path.extension().map(|e| e == "json").unwrap_or(false)
                    && path.file_name().and_then(|n| n.to_str()).map_or(false, |n| n.ends_with(".live.json"))
                {
                    path
                } else {
                    continue;
                };
                if !live_path.exists() { continue; }
                if let Ok(data) = std::fs::read_to_string(&live_path) {
                    if let Ok(file) = serde_json::from_str::<SessionFile>(&data) {
                        seen.insert(file.seed.clone());
                        metas.push(SessionMeta {
                            seed: file.seed,
                            created_at: file.created_at,
                            updated_at: file.updated_at,
                            model: file.model,
                            effort: file.effort,
                            message_count: file.messages.len(),
                            last_summary: format!("[INTERRUPTED] {}", file.last_summary),
                        });
                    }
                }
            }
        }
    }

    // ── 2. Finalized sessions from index (deduplicate by seed) ──
    for meta in super::load_index() {
        if !seen.contains(&meta.seed) {
            metas.push(meta);
        }
    }

    // Sort: most recently updated first
    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    metas
}
