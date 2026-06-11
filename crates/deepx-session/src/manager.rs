//! SessionManager — unified singleton for session persistence and lifecycle.
//!
//! Pattern mirrors dsx-tools::ToolManager.

use std::path::PathBuf;
use std::sync::OnceLock;

use deepx_types::{SessionFile, SessionMeta};
use sha2::{Sha256, Digest};

static INSTANCE: OnceLock<SessionManager> = OnceLock::new();

#[derive(Debug)]
pub struct SessionManager {
    sessions_dir: PathBuf,
    active_path: PathBuf,
}

impl SessionManager {
    /// Initialize the global singleton. Must be called once at startup.
    pub fn init(data_dir: PathBuf) {
        let mgr = Self {
            active_path: data_dir.join(".active_session"),
            sessions_dir: data_dir.join("sessions"),
        };
        INSTANCE.set(mgr).expect("SessionManager already initialized");
    }

    /// Access the global instance.
    pub fn global() -> &'static Self {
        INSTANCE.get().expect("SessionManager not initialized — call init() first")
    }

    // ── Session listing ──

    /// List all sessions sorted by updated_at descending.
    pub fn list(&self) -> Vec<SessionMeta> {
        let mut metas = self.load_index();

        // Fallback: if index.toml is empty/broken, scan directories
        if metas.is_empty() {
            if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() { continue; }
                    let session_file = path.join("session.toml");
                    if let Ok(data) = std::fs::read_to_string(&session_file) {
                        if let Ok(sf) = toml::from_str::<SessionFile>(&data) {
                            metas.push(SessionMeta {
                                seed: sf.seed,
                                created_at: sf.created_at,
                                updated_at: sf.updated_at,
                                model: sf.model,
                                effort: sf.effort,
                                message_count: sf.messages.len(),
                                last_summary: sf.last_summary,
                                checksum: sf.checksum.clone(),
                            });
                        }
                    }
                }
            }
        }

        metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        metas
    }

    /// Delete a session: removes the session directory and its index entry.
    pub fn delete(&self, seed: &str) -> Result<(), String> {
        let dir = self.session_dir(seed)
            .ok_or_else(|| format!("Session not found: {seed}"))?;

        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("Failed to delete session: {e}"))?;

        let mut index = self.load_index();
        index.retain(|m| m.seed != seed);
        self.save_index(&index);

        log::info!("SessionManager: deleted session {seed}");
        Ok(())
    }

    // ── Session CRUD (continued) ──

    /// Load full session data from disk. Verifies integrity if checksum present.
    pub fn load(&self, seed: &str) -> Option<SessionFile> {
        let file = self.load_from_disk(seed)?;
        if let Some(ref stored) = file.checksum {
            let computed = Self::compute_checksum(&file.seed, &file.messages);
            if stored != &computed {
                log::error!("SessionManager: checksum mismatch for session {} — data may be tampered", file.seed);
                return None;
            }
        }
        Some(file)
    }

    /// Save session data and update index.
    pub fn save(&self, seed: &str, messages: &[deepx_types::Message], model: &str, effort: Option<&str>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let session_path = self.session_path(seed);
        if let Some(parent) = session_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let checksum = Self::compute_checksum(seed, messages);
        let file = SessionFile {
            seed: seed.to_string(),
            created_at: self.existing_created_at(seed).unwrap_or(now),
            updated_at: now,
            model: model.to_string(),
            effort: effort.map(String::from),
            messages: messages.to_vec(),
            last_summary: Self::extract_summary(messages),
            checksum: Some(checksum),
        };

        // Write session file
        if let Ok(content) = toml::to_string_pretty(&file) {
            if std::fs::write(&session_path, &content).is_err() {
                log::error!("SessionManager: failed to write session file {}", session_path.display());
            }
        }

        // Update index
        let mut index = self.load_index();
        let meta = SessionMeta {
            seed: file.seed.clone(),
            created_at: file.created_at,
            updated_at: file.updated_at,
            model: file.model.clone(),
            effort: file.effort.clone(),
            message_count: file.messages.len(),
            last_summary: file.last_summary.clone(),
            checksum: file.checksum.clone(),
        };
        if let Some(existing) = index.iter_mut().find(|m| m.seed == meta.seed) {
            *existing = meta;
        } else {
            index.push(meta);
        }
        self.save_index(&index);
    }

    // ── Active session ──

    /// Read the currently active session seed.
    pub fn active_seed(&self) -> Option<String> {
        std::fs::read_to_string(&self.active_path).ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Set the active session seed (persisted to disk).
    pub fn set_active_seed(&self, seed: &str) {
        if let Some(parent) = self.active_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&self.active_path, seed).is_err() {
            log::error!("SessionManager: failed to write active session file");
        }
    }

    /// Clear the active session marker.
    pub fn clear_active(&self) {
        let _ = std::fs::remove_file(&self.active_path);
    }


    /// Generate a new session seed.
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

    /// Current UNIX epoch.
    pub fn now_epoch() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }


    /// Compute an integrity checksum for a set of messages.
    fn compute_checksum(seed: &str, messages: &[deepx_types::Message]) -> String {
        let msg_toml = toml::to_string(messages).unwrap_or_default();
        let input = format!("{}:{}", seed, msg_toml);
        let hash = Sha256::digest(input.as_bytes());
        format!("{:x}", hash)
    }


    /// Explicitly verify the integrity of a session file on disk.
    pub fn verify_integrity(&self, seed: &str) -> Result<(), String> {
        let file = self.load_from_disk(seed)
            .ok_or_else(|| format!("Session not found: {seed}"))?;
        match &file.checksum {
            None => Err("No checksum stored — session predates integrity checks".into()),
            Some(stored) => {
                let computed = Self::compute_checksum(&file.seed, &file.messages);
                if stored == &computed {
                    Ok(())
                } else {
                    Err(format!("Checksum mismatch for session {seed}"))
                }
            }
        }
    }

    // ── Private helpers ──

    fn load_index(&self) -> Vec<SessionMeta> {
        let path = self.sessions_dir.join("index.toml");
        if !path.exists() { return vec![]; }
        let Ok(data) = std::fs::read_to_string(&path) else { return vec![] };
        toml::from_str::<Vec<SessionMeta>>(&data).unwrap_or_else(|_| {
            serde_json::from_str::<Vec<SessionMeta>>(&data).unwrap_or_default()
        })
    }

    fn save_index(&self, metas: &[SessionMeta]) {
        let path = self.sessions_dir.join("index.toml");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = toml::to_string_pretty(metas) {
            if std::fs::write(&path, &content).is_err() {
                log::error!("SessionManager: failed to write index.toml");
            }
        }
    }

    fn load_from_disk(&self, seed: &str) -> Option<SessionFile> {
        let path = self.session_path(seed);
        if !path.exists() { return None; }
        let data = std::fs::read_to_string(&path).ok()?;
        toml::from_str::<SessionFile>(&data).ok()
            .or_else(|| serde_json::from_str::<SessionFile>(&data).ok())
    }

    fn session_path(&self, seed: &str) -> PathBuf {
        if let Some(dir) = self.session_dir(seed) {
            return dir.join("session.toml");
        }
        let date = chrono_date();
        self.sessions_dir.join(format!("{}-{}", seed, date)).join("session.toml")
    }

    fn session_dir(&self, seed: &str) -> Option<PathBuf> {
        if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let name = path.file_name()?.to_string_lossy();
                if name.starts_with(&format!("{}-", seed)) {
                    return Some(path);
                }
            }
        }
        None
    }

    fn existing_created_at(&self, seed: &str) -> Option<u64> {
        let path = self.session_path(seed);
        if path.exists() {
            let data = std::fs::read_to_string(&path).ok()?;
            toml::from_str::<SessionFile>(&data).ok().map(|f| f.created_at)
        } else {
            None
        }
    }

    fn extract_summary(messages: &[deepx_types::Message]) -> String {
        messages.iter().rev()
            .find(|m| m.role == "assistant" && !m.content.is_empty())
            .and_then(|m| m.content.iter().find_map(|b| {
                if let deepx_types::ContentBlock::Text { text } = b {
                    Some(text.lines().next().unwrap_or(text))
                } else { None }
            }))
            .map(|s| {
                if s.len() <= 80 { return s.to_string(); }
                let mut end = 80;
                while !s.is_char_boundary(end) { end -= 1; }
                format!("{}..", &s[..end])
            })
            .unwrap_or_default()
    }
}

fn chrono_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

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
