//! Turso local database backend for session dual-write.
//!
//! Mirrors JSONL session data to a local Turso (.db) file for fast
//! queries without cloud sync.
//!
//! All code is gated by `#[cfg(feature = "turso-backend")]` —
//! the module declaration in `store/mod.rs` already carries the gate.

use std::path::Path;

use deepx_types::{Message, SessionMeta};

/// Turso-backed session store mirroring JSONL data.
#[derive(Debug)]
pub struct TursoBackend {
    db: turso::Database,
}

impl TursoBackend {
    /// Open or create a local Turso database at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        let db = rt
            .block_on(turso::Builder::new_local(path.as_ref()).build())
            .map_err(|e| format!("open turso db: {e}"))?;
        Ok(Self { db })
    }

    /// Create tables if they don't exist.
    pub fn init_tables(&self) -> Result<(), String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            self.db
                .execute(
                    "CREATE TABLE IF NOT EXISTS sessions (
                        seed TEXT PRIMARY KEY,
                        created_at INTEGER NOT NULL,
                        updated_at INTEGER NOT NULL,
                        model TEXT NOT NULL,
                        effort TEXT,
                        message_count INTEGER NOT NULL DEFAULT 0,
                        turn_count INTEGER NOT NULL DEFAULT 0,
                        last_summary TEXT NOT NULL DEFAULT '',
                        compact_skip INTEGER NOT NULL DEFAULT 0
                    )",
                    [],
                )
                .await
                .map_err(|e| format!("create sessions table: {e}"))?;

            self.db
                .execute(
                    "CREATE TABLE IF NOT EXISTS messages (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        seed TEXT NOT NULL,
                        msg_id INTEGER,
                        role TEXT NOT NULL,
                        name TEXT,
                        content TEXT NOT NULL,
                        created_at INTEGER NOT NULL DEFAULT (unixepoch())
                    )",
                    [],
                )
                .await
                .map_err(|e| format!("create messages table: {e}"))?;

            Ok(())
        })
    }

    /// Upsert session metadata.
    pub fn upsert_meta(&self, meta: &SessionMeta) -> Result<(), String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            self.db
                .execute(
                    "INSERT OR REPLACE INTO sessions
                        (seed, created_at, updated_at, model, effort, message_count, turn_count, last_summary, compact_skip)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    turso::params![
                        meta.seed,
                        meta.created_at as i64,
                        meta.updated_at as i64,
                        meta.model,
                        meta.effort,
                        meta.message_count as i64,
                        meta.turn_count as i64,
                        meta.last_summary,
                        meta.compact_skip as i64
                    ],
                )
                .await
                .map_err(|e| format!("upsert meta: {e}"))?;
            Ok(())
        })
    }

    /// Insert a message into the messages table.
    pub fn insert_message(&self, seed: &str, msg: &Message) -> Result<(), String> {
        let content = serde_json::to_string(msg)
            .map_err(|e| format!("serialize message: {e}"))?;
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            self.db
                .execute(
                    "INSERT INTO messages (seed, msg_id, role, name, content)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    turso::params![seed, msg.msg_id, msg.role, msg.name, content],
                )
                .await
                .map_err(|e| format!("insert message: {e}"))?;
            Ok(())
        })
    }

    /// Load all messages for a session, ordered by insertion.
    pub fn load_messages(&self, seed: &str) -> Result<Vec<Message>, String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            let mut rows = self
                .db
                .query(
                    "SELECT content FROM messages WHERE seed = ?1 ORDER BY id",
                    turso::params![seed],
                )
                .await
                .map_err(|e| format!("query messages: {e}"))?;

            let mut msgs = Vec::new();
            while let Some(row) = rows.next().await {
                let row = row.map_err(|e| format!("read row: {e}"))?;
                let content: String = row
                    .get("content")
                    .map_err(|e| format!("get content: {e}"))?;
                let msg: Message = serde_json::from_str(&content)
                    .map_err(|e| format!("deserialize message: {e}"))?;
                msgs.push(msg);
            }
            Ok(msgs)
        })
    }

    /// Load session metadata, if it exists.
    pub fn load_meta(&self, seed: &str) -> Result<Option<SessionMeta>, String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            let mut rows = self
                .db
                .query(
                    "SELECT seed, created_at, updated_at, model, effort, message_count, turn_count, last_summary, compact_skip
                     FROM sessions WHERE seed = ?1",
                    turso::params![seed],
                )
                .await
                .map_err(|e| format!("query meta: {e}"))?;

            let Some(row) = rows.next().await else {
                return Ok(None);
            };
            let row = row.map_err(|e| format!("read row: {e}"))?;
            Ok(Some(SessionMeta {
                seed: row.get("seed").map_err(|e| format!("get seed: {e}"))?,
                created_at: row.get::<i64>("created_at").map_err(|e| format!("get created_at: {e}"))? as u64,
                updated_at: row.get::<i64>("updated_at").map_err(|e| format!("get updated_at: {e}"))? as u64,
                model: row.get("model").map_err(|e| format!("get model: {e}"))?,
                effort: row.get("effort").ok(),
                message_count: row.get::<i64>("message_count").map_err(|e| format!("get message_count: {e}"))? as usize,
                turn_count: row.get::<i64>("turn_count").map_err(|e| format!("get turn_count: {e}"))? as usize,
                last_summary: row.get("last_summary").map_err(|e| format!("get last_summary: {e}"))?,
                compact_skip: row.get::<i64>("compact_skip").map_err(|e| format!("get compact_skip: {e}"))? as usize,
                ..Default::default()
            }))
        })
    }

    /// List all sessions ordered by updated_at descending.
    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>, String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            let mut rows = self
                .db
                .query(
                    "SELECT seed, created_at, updated_at, model, effort, message_count, turn_count, last_summary, compact_skip
                     FROM sessions ORDER BY updated_at DESC",
                    [],
                )
                .await
                .map_err(|e| format!("query all sessions: {e}"))?;

            let mut metas = Vec::new();
            while let Some(row) = rows.next().await {
                let row = row.map_err(|e| format!("read row: {e}"))?;
                metas.push(SessionMeta {
                    seed: row.get("seed").map_err(|e| format!("get seed: {e}"))?,
                    created_at: row.get::<i64>("created_at").map_err(|e| format!("get created_at: {e}"))? as u64,
                    updated_at: row.get::<i64>("updated_at").map_err(|e| format!("get updated_at: {e}"))? as u64,
                    model: row.get("model").map_err(|e| format!("get model: {e}"))?,
                    effort: row.get("effort").ok(),
                    message_count: row.get::<i64>("message_count").map_err(|e| format!("get message_count: {e}"))? as usize,
                    turn_count: row.get::<i64>("turn_count").map_err(|e| format!("get turn_count: {e}"))? as usize,
                    last_summary: row.get("last_summary").map_err(|e| format!("get last_summary: {e}"))?,
                    compact_skip: row.get::<i64>("compact_skip").map_err(|e| format!("get compact_skip: {e}"))? as usize,
                    ..Default::default()
                });
            }
            Ok(metas)
        })
    }

    /// Delete a session and its messages.
    pub fn delete_session(&self, seed: &str) -> Result<(), String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("create tokio runtime: {e}"))?;
        rt.block_on(async {
            self.db
                .execute("DELETE FROM messages WHERE seed = ?1", turso::params![seed])
                .await
                .map_err(|e| format!("delete messages: {e}"))?;
            self.db
                .execute("DELETE FROM sessions WHERE seed = ?1", turso::params![seed])
                .await
                .map_err(|e| format!("delete session: {e}"))?;
            Ok(())
        })
    }
}
