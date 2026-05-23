//! Core types for the dsx-hp daemon: verdict, errors, process metadata.
//!
//! All types here are `Serialize + Deserialize` for JSON-LP IPC framing.


use serde::{Deserialize, Serialize};

// ── Verdict — judge() output ──

/// Health verdict — the sole output of `judge()`, serialized as JSON-LP frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Verdict {
    /// Process presumed dead — heartbeat timeout exceeded.
    Dead {
        pid: u32,
        name: String,
        reason: String,
        since_secs: u64,
    },
}

impl Verdict {
    /// Returns `true` for verdicts that demand immediate attention.
    pub fn is_critical(&self) -> bool {
        matches!(self, Verdict::Dead { .. })
    }
}

/// Process types that can register with the HP daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProcessKind {
    Agent,
    Tools,
    Tui,
}

impl std::fmt::Display for ProcessKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessKind::Agent => write!(f, "agent"),
            ProcessKind::Tools => write!(f, "tools"),
            ProcessKind::Tui => write!(f, "tui"),
        }
    }
}

// ── Errors ──

/// Errors returned by the `HealthProbe` service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HpError {
    ProcessNotFound(u32),
    DuplicateRegistration(u32),
}

impl std::fmt::Display for HpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HpError::ProcessNotFound(pid) => write!(f, "process {pid} not found"),
            HpError::DuplicateRegistration(pid) => write!(f, "process {pid} already registered"),
        }
    }
}

impl std::error::Error for HpError {}

// ── Process state structs ──

/// Full health state for a single process (returned by `query()`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessHealth {
    pub pid: u32,
    pub kind: ProcessKind,
    pub name: String,
    pub alive: bool,
    /// Unix-epoch seconds of the last received heartbeat.
    pub last_heartbeat: u64,
}

/// Lightweight process descriptor for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub pid: u32,
    pub kind: ProcessKind,
    pub name: String,
    pub alive: bool,
    pub uptime_secs: u64,
}
