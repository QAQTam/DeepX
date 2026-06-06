//! FileTracker: explore-before-read state machine and file change detection.
//!
//! Tracks which files were read/written on which turn, enforces
//! stale-edit blocking, and caches file hashes for change detection.

use std::collections::HashMap;
use super::result::FileSnapshot;

#[derive(Debug, Clone)]
pub struct FileTracker {
    /// Whether explore() has been called at least once.
    pub has_explored: bool,

    /// Per-file turn NUMBER when last read (compared against current_turn).
    pub file_read_at: HashMap<String, u32>,

    /// Per-file turn NUMBER when last written by a tool.
    pub file_written_at: HashMap<String, u32>,

    /// Monotonic staleness counter — only increments when files are written.
    /// Chat-only turns don't advance this. Used to detect stale reads.
    pub staleness_epoch: u32,

    /// After a file write/edit, forces a re-read before other tools.
    pub re_read_required: Option<String>,

    /// Files written/edited this turn (for sandbox enforcement).
    pub files_written_this_turn: Vec<String>,

    /// File path → (hash, last_modified). Used to skip re-reads
    /// of unchanged files and serve cached diffs across turns.
    pub file_cache: HashMap<String, FileSnapshot>,
}

impl FileTracker {
    pub fn new() -> Self {
        Self {
            has_explored: false,
            file_read_at: HashMap::new(),
            file_written_at: HashMap::new(),
            staleness_epoch: 0,
            re_read_required: None,
            files_written_this_turn: Vec::new(),
            file_cache: HashMap::new(),
        }
    }

    /// Mark a file as just read (records current turn number).
    pub fn touch_file(&mut self, path: &str) {
        self.file_read_at.insert(path.to_string(), self.staleness_epoch);
    }

    /// Mark a file as just written (records current turn, triggers re-read).
    pub fn mark_file_written(&mut self, path: &str) {
        self.file_written_at.insert(path.to_string(), self.staleness_epoch);
        self.re_read_required = Some(path.to_string());
    }

    /// Check if a file is stale.
    ///
    /// Stale when:
    /// 1. Written after last read (e.g. external edit), OR
    /// 2. Read more than MAX_TURNS_SINCE_READ turns ago (LLM context window expires).
    ///
    /// A file with read_at == 0 (never read) is also stale — forces initial read.
    pub fn is_file_stale(&self, path: &str) -> bool {
        const MAX_TURNS_SINCE_READ: u32 = 10;
        let read_at = self.file_read_at.get(path).copied().unwrap_or(0);
        let self_written = self.file_written_at.get(path).copied().unwrap_or(0);
        self_written > read_at || self.staleness_epoch.saturating_sub(read_at) > MAX_TURNS_SINCE_READ
    }

    /// Cache file snapshot after read. Returns true if file has changed since last cache.
    pub fn cache_file(&mut self, path: &str) -> bool {
        let new = FileSnapshot {
            lines: 0,
            hash: FileSnapshot::hash_of(path),
            last_read_turn: 0,
        };
        if let Some(old) = self.file_cache.get(path) {
            if old.hash == new.hash {
                return false;
            }
        }
        self.file_cache.insert(path.to_string(), new);
        true
    }

    /// Increment turn counter and return the new turn number.
    pub fn increment_turn(&mut self) -> u32 {
        self.staleness_epoch += 1;
        self.staleness_epoch
    }

    /// Clear per-turn scratch state (called at start of each turn).
    pub fn reset_turn(&mut self) {
        self.files_written_this_turn.clear();
    }
}

impl Default for FileTracker {
    fn default() -> Self {
        Self::new()
    }
}
