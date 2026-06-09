//! DSX message protocol — shared frame definitions for agent communication.
//!
//! Every frame is a single-line JSON object (`\n` delimited), tagged with `"type"`.
//!
//! ## Channels
//!
//! | Channel | Transport | Direction |
//! |---------|-----------|-----------|
//! | UI ↔ Agent | mpsc channels (primary) / stdin-stdout (headless) | Bidirectional |
//! | Agent ↔ Tools | direct call (in-process) | Bidirectional |
//! | Agent → HP | TCP localhost | Bidirectional |
//!
//! ## Submodules
//!
//! - `agent_protocol` — `Ui2Agent` / `Agent2Ui`
//! - `hp` — `AgentToHp` / `HpToAgent`

use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

// ── Submodule declarations ──────────────────────────────────────────────

mod agent_protocol;

// ── Re-exports ──────────────────────────────────────────────────────────

pub use agent_protocol::{Agent2Ui, DocInfo, FileSnapshotInfo, RoundBlock, RoundData, RoundDeltaKind, TaskInfo, ToolCallDef, ToolResultDef, TurnData, Ui2Agent};

// ── Redacted (prevents API key leaks in debug logs) ─────────────────────

/// Wrapper that serializes normally but redacts in Debug output.
/// Prevents API keys from leaking into debug logs.
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted(pub String);

impl Serialize for Redacted {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("\"\"")
        } else {
            f.write_str("\"***\"")
        }
    }
}

impl<'de> Deserialize<'de> for Redacted {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Redacted)
    }
}

impl From<&str> for Redacted {
    fn from(s: &str) -> Self {
        Redacted(s.to_string())
    }
}

impl From<String> for Redacted {
    fn from(s: String) -> Self {
        Redacted(s)
    }
}

// ── Frame I/O helpers ──────────────────────────────────────────────────

use std::io::{self, BufRead, Write};

/// Read one JSON-LP line and deserialize into `T`.
pub fn read_frame<T: for<'de> Deserialize<'de>>(reader: &mut impl BufRead) -> io::Result<Option<T>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str::<T>(trimmed).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Serialize `frame` and write as one JSON-LP line (append `\n` + flush).
pub fn write_frame(writer: &mut impl Write, frame: &impl Serialize) -> io::Result<()> {
    let json = serde_json::to_string(frame)?;
    writeln!(writer, "{}", json)?;
    writer.flush()?;
    Ok(())
}

/// Convenience: write a raw string as a JSON-LP line.
pub fn write_line(writer: &mut impl Write, line: &str) {
    let _unused = writeln!(writer, "{line}");
    let _unused = writer.flush();
}
