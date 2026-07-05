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

static INSTANCE: OnceLock<SessionManager> = OnceLock::new();

#[derive(Debug)]
pub struct SessionManager {
    sessions_dir: PathBuf,
    active_path: PathBuf,
    #[cfg(feature = "turso-backend")]
    db: Option<TursoBackend>,
}

impl SessionManager {
    /// Initialize the global singleton. Must be called once at startup.
    /// Also triggers automatic migration from legacy TOML format if needed.
    /// When `db_url` is `Some`, a Turso local database mirror is opened.
    pub fn init(data_dir: PathBuf, db_url: Option<String>) {
        #[cfg(feature = "turso-backend")]
        let db = {
            let path = db_url.unwrap_or_else(|| {
                data_dir.join("sessions.db").to_string_lossy().to_string()
            });
            TursoBackend::open(&path)
                .inspect(|_| log::info!("SessionManager: Turso backend at {}", path))
                .ok()
                .and_then(|db| {
                    if let Err(e) = db.init_tables() {
                        log::warn!("SessionManager: Turso init_tables failed: {e}");
                    }
                    Some(db)
                })
        };

        #[cfg(not(feature = "turso-backend"))]
        let _ = db_url;

        let mgr = Self {
            active_path: data_dir.join(".active_session"),
            sessions_dir: data_dir.join("sessions"),
            #[cfg(feature = "turso-backend")]
            db,
        };
        // Migrate old TOML sessions on first startup of v0.4.0
        crate::migrate::run(&mgr.sessions_dir);
        INSTANCE.set(mgr).expect("SessionManager already initialized");
    }

    /// Access the global instance.
    pub fn global() -> &'static Self {
        INSTANCE.get().expect("SessionManager not initialized — call init() first")
    }

    // ── Session listing ──

    /// List all sessions sorted by updated_at descending.
    pub fn list(&self) -> Vec<SessionMeta> {
        let mut metas = store::read_index(&self.sessions_dir);

        // Fallback: scan directories if index is empty
        if metas.is_empty() {
            if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() { continue; }
                    if let Some(meta) = store::read_meta(&path) {
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
        let dir = self.session_dir(seed)
            .ok_or_else(|| format!("Session not found: {seed}"))?;

        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("Failed to delete session: {e}"))?;

        store::remove_from_index(&self.sessions_dir, seed);

        #[cfg(feature = "turso-backend")]
        if let Some(ref db) = self.db {
            let _ = db.delete_session(seed);
        }

        log::info!("SessionManager: deleted session {seed}");
        Ok(())
    }

    // ── Load / Save ──

    /// Load session messages from disk. Returns (meta, messages).
    pub fn load(&self, seed: &str) -> Option<(SessionMeta, Vec<Message>)> {
        let dir = self.session_dir(seed)?;
        let meta = store::read_meta(&dir)?;
        let messages = store::read_messages(&dir).ok()?;
        Some((meta, messages))
    }

    /// Check whether a session directory exists on disk (fast path).
    pub fn exists(&self, seed: &str) -> bool {
        self.session_dir(seed).is_some()
    }

    /// Load only metadata (fast, no message parsing).
    pub fn load_meta(&self, seed: &str) -> Option<SessionMeta> {
        let dir = self.session_dir(seed)?;
        store::read_meta(&dir)
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
        if let Some(ref db) = self.db {
            let _ = db.insert_message(seed, msg);
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
        let created_at = self.load_meta(seed)
            .map(|m| m.created_at)
            .unwrap_or(now);
        let total = store::count_message_lines(&dir).unwrap_or(0);

        // Extract summary: read last few messages for title
        let last_summary = match store::read_messages(&dir) {
            Ok(msgs) => Self::extract_summary(&msgs),
            Err(_) => String::new(),
        };

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
            ..Default::default()
        };
        if let Err(e) = store::write_meta(&dir, &meta) {
            log::error!("SessionManager: write_meta failed: {e}");
            return;
        }
        store::upsert_index(&self.sessions_dir, &meta);

        #[cfg(feature = "turso-backend")]
        if let Some(ref db) = self.db {
            let _ = db.upsert_meta(seed, &meta);
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

        let created_at = self.load_meta(seed)
            .map(|m| m.created_at)
            .unwrap_or(now);

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
        if let Some(ref db) = self.db {
            let _ = db.upsert_meta(seed, &meta);
            let _ = db.insert_messages_batch(seed, messages);
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
        if new_messages.is_empty() { return; }

        let now = Self::now_epoch();
        let dir = self.session_path_dir(seed);
        let _ = std::fs::create_dir_all(&dir);

        let created_at = self.load_meta(seed)
            .map(|m| m.created_at)
            .unwrap_or(now);

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
            ..Default::default()
        };

        if let Err(e) = store::write_meta(&dir, &meta) {
            log::error!("SessionManager: write_meta failed: {e}");
            return;
        }
        store::upsert_index(&self.sessions_dir, &meta);

        #[cfg(feature = "turso-backend")]
        if let Some(ref db) = self.db {
            let _ = db.upsert_meta(seed, &meta);
            for msg in new_messages {
                let _ = db.insert_message(seed, msg);
            }
        }
    }

    /// Truncate messages.jsonl to `keep_lines` lines.
    /// Returns the truncated messages.
    pub fn truncate_messages(&self, seed: &str, keep_lines: usize) -> Result<Vec<Message>, String> {
        let dir = self.session_dir(seed)
            .ok_or_else(|| format!("Session not found: {seed}"))?;
        store::truncate_messages(&dir, keep_lines)
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

    // ── Helpers ──

    /// Generate a new session seed (8 hex chars from hashed time + PID).
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

    // ── Private ──

    fn session_path_dir(&self, seed: &str) -> PathBuf {
        self.sessions_dir.join(seed)
    }

    fn session_dir(&self, seed: &str) -> Option<PathBuf> {
        let dir = self.session_path_dir(seed);
        if dir.exists() && dir.is_dir() { Some(dir) } else { None }
    }

    fn extract_summary(messages: &[Message]) -> String {
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
