//! Turso local database backend for session dual-write.
//!
//! Mirrors JSONL session data to a local Turso (.db) file.
//! All code gated by `#[cfg(feature = "turso-backend")]`.

use std::path::Path;
use std::sync::LazyLock;

use deepx_types::{Message, SessionMeta};

static RT: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Builder::new_current_thread().enable_all().build().expect("turso tokio runtime"));

pub struct TursoBackend {
    _db: turso::Database,
    conn: turso::Connection,
}

impl std::fmt::Debug for TursoBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TursoBackend").finish_non_exhaustive()
    }
}

impl TursoBackend {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_str = path.as_ref().to_str().ok_or("invalid path")?;
        let db = RT
            .block_on(turso::Builder::new_local(path_str).build())
            .map_err(|e| format!("open turso: {e}"))?;
        let conn = db.connect().map_err(|e| format!("connect turso: {e}"))?;
        Ok(Self { _db: db, conn })
    }

    pub fn init_tables(&self) -> Result<(), String> {
        RT.block_on(async {
            self.conn
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS sessions (
                        seed TEXT PRIMARY KEY,
                        meta_json TEXT NOT NULL DEFAULT '{}',
                        created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                        updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                    );
                    CREATE TABLE IF NOT EXISTS messages (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        session_seed TEXT NOT NULL,
                        msg_id INTEGER,
                        role TEXT NOT NULL DEFAULT '',
                        content_json TEXT NOT NULL DEFAULT '{}',
                        FOREIGN KEY (session_seed) REFERENCES sessions(seed) ON DELETE CASCADE
                    );
                    CREATE INDEX IF NOT EXISTS idx_msgs ON messages(session_seed, msg_id);",
                )
                .await
                .map_err(|e| format!("init tables: {e}"))
        })
    }

    // ── meta ──────────────────────────────────────────────────────────

    pub fn upsert_meta(&self, seed: &str, meta: &SessionMeta) -> Result<(), String> {
        let json = serde_json::to_string(meta).unwrap_or_default();
        let seed = seed.to_string();
        RT.block_on(async {
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO sessions (seed, meta_json, updated_at)
                     VALUES (?1, ?2, unixepoch())",
                    turso::params![seed, json],
                )
                .await
                .map_err(|e| format!("upsert meta: {e}"))?;
            Ok(())
        })
    }

    pub fn load_meta(&self, seed: &str) -> Result<Option<SessionMeta>, String> {
        let seed = seed.to_string();
        RT.block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT meta_json FROM sessions WHERE seed = ?1",
                    turso::params![seed.clone()],
                )
                .await
                .map_err(|e| format!("load meta: {e}"))?;
            if let Some(row) = rows.next().await.map_err(|e| format!("rows: {e}"))? {
                let s: String = row
                    .get_value(0)
                    .map_err(|e| format!("get: {e}"))?
                    .as_text()
                    .unwrap_or(&String::new())
                    .clone();
                Ok(serde_json::from_str(&s).ok())
            } else {
                Ok(None)
            }
        })
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>, String> {
        RT.block_on(async {
            let mut rows = self
                .conn
                .query("SELECT meta_json FROM sessions ORDER BY updated_at DESC", [0i32; 0])
                .await
                .map_err(|e| format!("list: {e}"))?;
            let mut v = Vec::new();
            while let Some(row) = rows.next().await.map_err(|e| format!("rows: {e}"))? {
                let s: String = row
                    .get_value(0)
                    .map_err(|e| format!("get: {e}"))?
                    .as_text()
                    .unwrap_or(&String::new())
                    .clone();
                if let Ok(m) = serde_json::from_str(&s) {
                    v.push(m);
                }
            }
            Ok(v)
        })
    }

    // ── messages ──────────────────────────────────────────────────────

    pub fn insert_message(&self, seed: &str, msg: &Message) -> Result<(), String> {
        let json = serde_json::to_string(msg).unwrap_or_default();
        let seed = seed.to_string();
        let mid = msg.msg_id.map(|v| v as i64);
        let role = msg.role.clone();
        RT.block_on(async {
            self.conn
                .execute(
                    "INSERT INTO messages (session_seed, msg_id, role, content_json)
                     VALUES (?1, ?2, ?3, ?4)",
                    turso::params![seed, mid, role, json],
                )
                .await
                .map_err(|e| format!("insert msg: {e}"))?;
            Ok(())
        })
    }

    pub fn load_messages(&self, seed: &str) -> Result<Vec<Message>, String> {
        let seed = seed.to_string();
        RT.block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT content_json FROM messages WHERE session_seed = ?1 ORDER BY msg_id",
                    turso::params![seed],
                )
                .await
                .map_err(|e| format!("load msgs: {e}"))?;
            let mut v = Vec::new();
            while let Some(row) = rows.next().await.map_err(|e| format!("rows: {e}"))? {
                let s: String = row
                    .get_value(0)
                    .map_err(|e| format!("get: {e}"))?
                    .as_text()
                    .unwrap_or(&String::new())
                    .clone();
                if let Ok(m) = serde_json::from_str(&s) {
                    v.push(m);
                }
            }
            Ok(v)
        })
    }

    // ── delete ────────────────────────────────────────────────────────

    pub fn delete_session(&self, seed: &str) -> Result<(), String> {
        let seed = seed.to_string();
        RT.block_on(async {
            self.conn
                .execute(
                    "DELETE FROM messages WHERE session_seed = ?1",
                    turso::params![seed.clone()],
                )
                .await
                .map_err(|e| format!("del msgs: {e}"))?;
            self.conn
                .execute(
                    "DELETE FROM sessions WHERE seed = ?1",
                    turso::params![seed],
                )
                .await
                .map_err(|e| format!("del session: {e}"))?;
            Ok(())
        })
    }
}
