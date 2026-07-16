//! SessionManager — unified singleton for session persistence and lifecycle.
//!
//! Stores each session as:
//!   {sessions_dir}/{seed}/
//!     meta.json       — SessionMeta (atomic replace-write)
//!     messages.jsonl  — one JSON line per Message (append-only)
//!
//! A central `index.json` enables fast listing.

use std::path::PathBuf;
use std::sync::OnceLock;

use deepx_types::{Message, SessionMeta};

use crate::store;

#[cfg(feature = "turso-backend")]
use crate::store::turso_backend::TursoBackend;
#[cfg(feature = "turso-backend")]
use std::collections::HashMap;
#[cfg(feature = "turso-backend")]
use std::sync::Mutex;
#[cfg(feature = "turso-backend")]
use std::sync::atomic::{AtomicBool, Ordering};

static INSTANCE: OnceLock<SessionManager> = OnceLock::new();

#[derive(Debug)]
pub struct SessionManager {
    sessions_dir: PathBuf,
    active_path: PathBuf,
    #[cfg(feature = "turso-backend")]
    turso_enabled: AtomicBool,
    #[cfg(feature = "turso-backend")]
    dbs: Mutex<HashMap<String, TursoBackend>>,
}

impl SessionManager {
    /// Initialize the global singleton. Must be called once at startup.
    /// Also triggers automatic migration from legacy TOML format if needed.
    /// When `turso_enabled` is true, per-session SQLite databases are created
    /// at `{sessions_dir}/{seed}/sessions.db`.
    pub fn init(data_dir: PathBuf, turso_enabled: bool) {
        let sessions_dir = data_dir.join("sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        #[cfg(feature = "turso-backend")]
        log::info!(
            "SessionManager: Turso mirroring {} (per-session at {})",
            if turso_enabled { "ENABLED" } else { "DISABLED" },
            sessions_dir.join("<seed>").join("sessions.db").display()
        );

        let mgr = Self {
            active_path: data_dir.join(".active_session"),
            sessions_dir,
            #[cfg(feature = "turso-backend")]
            turso_enabled: AtomicBool::new(turso_enabled),
            #[cfg(feature = "turso-backend")]
            dbs: Mutex::new(HashMap::new()),
        };
        // Migrate old TOML sessions on first startup of v0.4.0
        crate::migrate::run(&mgr.sessions_dir);
        INSTANCE
            .set(mgr)
            .expect("SessionManager already initialized");
    }

    /// Access the global instance.
    pub fn global() -> &'static Self {
        INSTANCE
            .get()
            .expect("SessionManager not initialized — call init() first")
    }

    /// Toggle Turso mirroring at runtime (no restart needed).
    #[cfg(feature = "turso-backend")]
    pub fn set_turso_enabled(&self, enabled: bool) {
        let old = self.turso_enabled.load(Ordering::Relaxed);
        self.turso_enabled.store(enabled, Ordering::Relaxed);
        log::info!(
            "SessionManager: Turso mirroring {} -> {}",
            if old { "ENABLED" } else { "DISABLED" },
            if enabled { "ENABLED" } else { "DISABLED" },
        );
    }

    // ── Session listing ──

    /// List all sessions sorted by updated_at descending.
    /// Index-first; fallback scans directories with Turso-priority meta read.
    pub fn list(&self) -> Vec<SessionMeta> {
        let mut metas = store::read_index(&self.sessions_dir);

        // Fallback: scan directories if index is empty
        if metas.is_empty() {
            if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let seed = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    // Turso-first meta read, JSON fallback
                    let meta = {
                        #[cfg(feature = "turso-backend")]
                        if self.turso_enabled.load(Ordering::Relaxed) {
                            if let Some(dbs) = self.get_or_open_db(seed) {
                                if let Some(db) = dbs.get(seed) {
                                    if let Ok(Some(m)) = db.load_meta(seed) {
                                        Some(m)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                        #[cfg(not(feature = "turso-backend"))]
                        None
                    };
                    let meta = meta.or_else(|| store::read_meta(&path));
                    if let Some(meta) = meta {
                        metas.push(meta);
                    }
                }
            }
        }

        metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        metas
    }

    /// Delete a session: removes the session directory and its index entry.
    pub fn delete(&self, seed: &str) -> Result<(), String> {
        let dir = self
            .session_dir(seed)
            .ok_or_else(|| format!("Session not found: {seed}"))?;

        std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to delete session: {e}"))?;

        store::remove_from_index(&self.sessions_dir, seed);

        #[cfg(feature = "turso-backend")]
        {
            self.dbs.lock().unwrap().remove(seed);
        }

        log::info!("SessionManager: deleted session {seed}");
        Ok(())
    }

    // ── Load / Save ──

    /// Load session messages from disk. Reads from Turso when enabled,
    /// falling back to JSONL.
    pub fn load(&self, seed: &str) -> Option<(SessionMeta, Vec<Message>)> {
        let dir = self.session_dir(seed)?;
        let meta = store::read_meta(&dir)?;
        let messages = self.load_messages_inner(seed, &dir)?;
        Some((meta, messages))
    }

    /// Try Turso first (lazy-open if needed), fall back to JSONL.
    fn load_messages_inner(&self, seed: &str, dir: &std::path::Path) -> Option<Vec<Message>> {
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Some(dbs) = self.get_or_open_db(seed) {
                if let Some(db) = dbs.get(seed) {
                    if let Ok(msgs) = db.load_messages(seed) {
                        if !msgs.is_empty() {
                            log::info!(
                                "SessionManager: loaded {} msgs from Turso for {seed}",
                                msgs.len()
                            );
                            return Some(msgs);
                        }
                    }
                }
            }
        }
        let _ = seed;
        store::read_messages(dir).ok()
    }

    /// Check whether a session exists (directory on disk or Turso DB).
    pub fn exists(&self, seed: &str) -> bool {
        if self.session_dir(seed).is_some() {
            return true;
        }
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            return self.is_turso_backed(seed);
        }
        false
    }

    /// Check whether this session has messages in the Turso SQLite store.
    pub fn is_turso_backed(&self, seed: &str) -> bool {
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Some(dbs) = self.get_or_open_db(seed) {
                if let Some(db) = dbs.get(seed) {
                    return db.message_count(seed).unwrap_or(0) > 0;
                }
            }
        }
        let _ = seed;
        false
    }

    /// Count sessions that have JSONL data but not yet migrated to Turso.
    pub fn count_pending_migration(&self) -> usize {
        let mut count = 0;
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    if let Some(seed) = path.file_name().and_then(|n| n.to_str()) {
                        if path.join("messages.jsonl").exists() && !self.is_turso_backed(seed) {
                            count += 1;
                        }
                    }
                }
            }
        }
        count
    }

    /// Migrate all pending sessions from JSONL to Turso.
    /// Returns (migrated_count, total_messages).
    pub fn migrate_all_to_turso(&self) -> Result<(usize, usize), String> {
        #[cfg(not(feature = "turso-backend"))]
        return Err("Turso backend not compiled".into());

        #[cfg(feature = "turso-backend")]
        {
            if !self.turso_enabled.load(Ordering::Relaxed) {
                return Err("Turso is disabled in settings".into());
            }
            let mut migrated = 0usize;
            let mut total_msgs = 0usize;
            let entries: Vec<_> = std::fs::read_dir(&self.sessions_dir)
                .map_err(|e| format!("read sessions dir: {e}"))?
                .flatten()
                .filter(|e| e.path().is_dir())
                .collect();

            for entry in entries {
                let path = entry.path();
                let Some(seed) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let jsonl = path.join("messages.jsonl");
                if !jsonl.exists() {
                    continue;
                }
                if self.is_turso_backed(seed) {
                    continue;
                } // already done

                let msgs =
                    store::read_messages(&path).map_err(|e| format!("read {}: {e}", seed))?;
                if msgs.is_empty() {
                    continue;
                }

                let dbs = self
                    .get_or_open_db(seed)
                    .ok_or_else(|| "failed to open Turso db".to_string())?;
                let db = dbs.get(seed).unwrap();

                let count = msgs.len();
                db.insert_messages_batch(seed, &msgs)
                    .map_err(|e| format!("batch insert {}: {e}", seed))?;
                // Also upsert meta to sessions table
                if let Some(meta) = store::read_meta(&path) {
                    let _ = db.upsert_meta(seed, &meta);
                }

                log::info!(
                    "SessionManager: migrated {} messages to Turso for {seed}",
                    count
                );
                migrated += 1;
                total_msgs += count;
            }
            Ok((migrated, total_msgs))
        }
    }

    /// Load only metadata (fast, no message parsing).
    /// Turso-first when enabled, falling back to JSON.
    pub fn load_meta(&self, seed: &str) -> Option<SessionMeta> {
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Some(dbs) = self.get_or_open_db(seed) {
                if let Some(db) = dbs.get(seed) {
                    match db.load_meta(seed) {
                        Ok(Some(meta)) => return Some(meta),
                        Ok(None) => {} // fall through to JSON
                        Err(e) => log::warn!("Turso load_meta failed for {seed}: {e}"),
                    }
                }
            }
        }
        let dir = self.session_dir(seed)?;
        store::read_meta(&dir)
    }

    /// Persist agent mode to meta.json without rewriting messages.
    /// Called when the user switches PLAN/CODE mode so it survives agent restart.
    pub fn persist_mode(&self, seed: &str, mode: u8) {
        let dir = self.session_path_dir(seed);
        let mut meta = self.load_meta(seed).unwrap_or_default();
        meta.mode = mode;
        let _ = store::write_meta(&dir, &meta);
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Some(dbs) = self.get_or_open_db(seed) {
                let _ = dbs.get(seed).unwrap().upsert_meta(seed, &meta);
            }
        }
    }

    pub fn persist_skills(&self, seed: &str, skills: deepx_types::SkillSessionStateV2) {
        let dir = self.session_path_dir(seed);
        let _ = std::fs::create_dir_all(&dir);
        let mut meta = self.load_meta(seed).unwrap_or_default();
        let now = Self::now_epoch();
        meta.seed = seed.to_string();
        if meta.created_at == 0 {
            meta.created_at = now;
        }
        meta.updated_at = now;
        meta.skills = skills;
        let _ = store::write_meta(&dir, &meta);
        store::upsert_index(&self.sessions_dir, &meta);
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Some(dbs) = self.get_or_open_db(seed) {
                let _ = dbs.get(seed).unwrap().upsert_meta(seed, &meta);
            }
        }
    }

    /// Append a single message to JSONL immediately (per-message persistence).
    /// Does NOT update meta or index — caller should update meta periodically.
    pub fn save_one(&self, seed: &str, msg: &Message) {
        let dir = self.session_path_dir(seed);
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = store::append_one(&dir, msg) {
            log::error!("SessionManager: save_one failed: {e}");
        }
        #[cfg(feature = "turso-backend")]
        if let Some(dbs) = self.get_or_open_db(seed) {
            let _ = dbs.get(seed).unwrap().insert_message(seed, msg);
        }
    }

    /// Update session metadata and index after messages have been appended.
    pub fn update_meta(
        &self,
        seed: &str,
        model: &str,
        effort: Option<&str>,
        compact_skip: usize,
        turn_count: usize,
    ) {
        let now = Self::now_epoch();
        let dir = self.session_path_dir(seed);
        let created_at = self.load_meta(seed).map(|m| m.created_at).unwrap_or(now);
        let total = store::count_message_lines(&dir).unwrap_or(0);

        // Extract summary: read last few messages for title
        let last_summary = match store::read_messages(&dir) {
            Ok(msgs) => Self::extract_summary(&msgs),
            Err(_) => String::new(),
        };

        let existing = self.load_meta(seed).unwrap_or_default();

        let meta = SessionMeta {
            seed: seed.to_string(),
            created_at,
            updated_at: now,
            model: model.to_string(),
            effort: effort.map(String::from),
            message_count: total,
            turn_count,
            last_summary,
            compact_skip,
            mode: existing.mode,
            skills: existing.skills,
            ..Default::default()
        };
        if let Err(e) = store::write_meta(&dir, &meta) {
            log::error!("SessionManager: write_meta failed: {e}");
            return;
        }
        store::upsert_index(&self.sessions_dir, &meta);

        #[cfg(feature = "turso-backend")]
        if let Some(dbs) = self.get_or_open_db(seed) {
            if let Err(e) = dbs.get(seed).unwrap().upsert_meta(seed, &meta) {
                log::warn!("SessionManager: Turso upsert_meta failed for {seed}: {e}");
            }
        }
    }

    /// Save session: write meta + rewrite all messages.
    /// Used for initial save or after undo/compact.
    pub fn save_full(
        &self,
        seed: &str,
        messages: &[Message],
        model: &str,
        effort: Option<&str>,
        compact_skip: usize,
        turn_count: usize,
    ) {
        let now = Self::now_epoch();
        let dir = self.session_path_dir(seed);
        let _ = std::fs::create_dir_all(&dir);

        let created_at = self.load_meta(seed).map(|m| m.created_at).unwrap_or(now);

        let existing = self.load_meta(seed).unwrap_or_default();
        let last_summary = Self::extract_summary(messages);

        let meta = SessionMeta {
            seed: seed.to_string(),
            created_at,
            updated_at: now,
            model: model.to_string(),
            effort: effort.map(String::from),
            message_count: messages.len(),
            turn_count,
            last_summary,
            compact_skip,
            mode: existing.mode,
            skills: existing.skills,
            ..Default::default()
        };

        if let Err(e) = store::rewrite_messages(&dir, messages) {
            log::error!("SessionManager: rewrite_messages failed: {e}");
            return;
        }
        if let Err(e) = store::write_meta(&dir, &meta) {
            log::error!("SessionManager: write_meta failed: {e}");
            return;
        }
        store::upsert_index(&self.sessions_dir, &meta);

        #[cfg(feature = "turso-backend")]
        if let Some(dbs) = self.get_or_open_db(seed) {
            let db = dbs.get(seed).unwrap();
            if let Err(e) = db.upsert_meta(seed, &meta) {
                log::warn!("SessionManager: Turso upsert_meta failed for {seed}: {e}");
            }
            if let Err(e) = db.rewrite_messages(seed, messages) {
                log::warn!("SessionManager: Turso rewrite_messages failed for {seed}: {e}");
            }
        }
    }

    /// Append new messages (since last save) to the session JSONL.
    /// Updates meta and index.
    pub fn save_append(
        &self,
        seed: &str,
        new_messages: &[Message],
        model: &str,
        effort: Option<&str>,
        compact_skip: usize,
        turn_count: usize,
    ) {
        if new_messages.is_empty() {
            return;
        }

        let now = Self::now_epoch();
        let dir = self.session_path_dir(seed);
        let _ = std::fs::create_dir_all(&dir);

        let existing = self.load_meta(seed).unwrap_or_default();
        let created_at = if existing.created_at == 0 {
            now
        } else {
            existing.created_at
        };

        // Append messages
        if let Err(e) = store::append_messages(&dir, new_messages) {
            log::error!("SessionManager: append_messages failed: {e}");
            return;
        }

        // Update total count
        let total = store::count_message_lines(&dir).unwrap_or(0);

        // Extract summary from new messages
        let last_summary = Self::extract_summary(new_messages);

        let meta = SessionMeta {
            seed: seed.to_string(),
            created_at,
            updated_at: now,
            model: model.to_string(),
            effort: effort.map(String::from),
            message_count: total,
            turn_count,
            last_summary,
            compact_skip,
            mode: existing.mode,
            skills: existing.skills,
            ..Default::default()
        };

        if let Err(e) = store::write_meta(&dir, &meta) {
            log::error!("SessionManager: write_meta failed: {e}");
            return;
        }
        store::upsert_index(&self.sessions_dir, &meta);

        #[cfg(feature = "turso-backend")]
        if let Some(dbs) = self.get_or_open_db(seed) {
            let db = dbs.get(seed).unwrap();
            if let Err(e) = db.upsert_meta(seed, &meta) {
                log::warn!("SessionManager: Turso upsert_meta failed for {seed}: {e}");
            }
            if let Err(e) = db.insert_messages_batch(seed, new_messages) {
                log::warn!("SessionManager: Turso insert_messages_batch failed for {seed}: {e}");
            }
        }
    }

    /// Truncate messages.jsonl to `keep_lines` lines.
    /// Returns the truncated messages.
    pub fn truncate_messages(&self, seed: &str, keep_lines: usize) -> Result<Vec<Message>, String> {
        let dir = self
            .session_dir(seed)
            .ok_or_else(|| format!("Session not found: {seed}"))?;
        let truncated = store::truncate_messages(&dir, keep_lines)?;
        #[cfg(feature = "turso-backend")]
        if self.turso_enabled.load(Ordering::Relaxed) {
            if let Some(dbs) = self.get_or_open_db(seed) {
                if let Some(db) = dbs.get(seed) {
                    if let Err(e) = db.rewrite_messages(seed, &truncated) {
                        log::warn!("SessionManager: Turso truncate rewrite failed for {seed}: {e}");
                    }
                }
            }
        }
        Ok(truncated)
    }

    // ── Active session ──

    /// Read the currently active session seed.
    pub fn active_seed(&self) -> Option<String> {
        std::fs::read_to_string(&self.active_path)
            .ok()
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

    // ── Helpers ──

    /// Generate a new session seed (8 hex chars from hashed time + PID).
    pub fn generate_seed() -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
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

    // ── Private ──

    #[cfg(feature = "turso-backend")]
    fn get_or_open_db(
        &self,
        seed: &str,
    ) -> Option<std::sync::MutexGuard<'_, HashMap<String, TursoBackend>>> {
        if !self.turso_enabled.load(Ordering::Relaxed) {
            return None;
        }
        let mut dbs = self.dbs.lock().unwrap();
        if !dbs.contains_key(seed) {
            let dir = self.session_path_dir(seed);
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("sessions.db");
            match TursoBackend::open(&path) {
                Ok(db) => {
                    log::info!("SessionManager: Turso backend at {}", path.display());
                    if let Err(e) = db.init_tables() {
                        log::warn!("SessionManager: Turso init_tables failed: {e}");
                    }
                    dbs.insert(seed.to_string(), db);
                }
                Err(e) => {
                    log::warn!("SessionManager: Turso open failed for {seed}: {e}");
                    return None;
                }
            }
        }
        Some(dbs)
    }

    fn session_path_dir(&self, seed: &str) -> PathBuf {
        self.sessions_dir.join(seed)
    }

    fn session_dir(&self, seed: &str) -> Option<PathBuf> {
        let dir = self.session_path_dir(seed);
        if dir.exists() && dir.is_dir() {
            Some(dir)
        } else {
            None
        }
    }

    fn extract_summary(messages: &[Message]) -> String {
        messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant" && !m.content.is_empty())
            .and_then(|m| {
                m.content.iter().find_map(|b| {
                    if let deepx_types::ContentBlock::Text { text } = b {
                        Some(text.lines().next().unwrap_or(text))
                    } else {
                        None
                    }
                })
            })
            .map(|s| {
                if s.len() <= 80 {
                    return s.to_string();
                }
                let mut end = 80;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}..", &s[..end])
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod skill_persistence_tests {
    use super::*;
    use deepx_types::{SkillSessionEntry, SkillSessionEntryState, SkillSessionStateV2};

    fn manager() -> (PathBuf, SessionManager) {
        let root = std::env::temp_dir().join(format!(
            "deepx-session-skills-{}-{}",
            std::process::id(),
            SessionManager::now_epoch(),
        ));
        let sessions_dir = root.join("sessions");
        std::fs::create_dir_all(&sessions_dir).expect("create test sessions");
        let manager = SessionManager {
            sessions_dir,
            active_path: root.join(".active_session"),
            #[cfg(feature = "turso-backend")]
            turso_enabled: AtomicBool::new(false),
            #[cfg(feature = "turso-backend")]
            dbs: Mutex::new(HashMap::new()),
        };
        (root, manager)
    }

    fn state() -> SkillSessionStateV2 {
        SkillSessionStateV2 {
            version: 2,
            context_epoch: 7,
            operation_revision: 9,
            entries: vec![SkillSessionEntry {
                name: "alpha".into(),
                activation_order: 1,
                source: "model".into(),
                state: SkillSessionEntryState::Active,
                lease_remaining: 2,
            }],
        }
    }

    #[test]
    fn metadata_rewrites_preserve_skill_session_state_v2() {
        let (root, manager) = manager();
        manager.persist_skills("seed", state());
        manager.update_meta("seed", "model", None, 0, 1);
        manager.save_full("seed", &[Message::user("hello")], "model", None, 0, 1);
        let meta = manager.load_meta("seed").expect("metadata");
        assert_eq!(meta.seed, "seed");
        assert_eq!(meta.skills, state());
        std::fs::remove_dir_all(root).expect("remove test directory");
    }
}
