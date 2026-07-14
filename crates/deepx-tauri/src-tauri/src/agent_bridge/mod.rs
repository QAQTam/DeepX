//! AgentRegistry — manages multiple agent child processes, one per session.
//!
//! Architecture (v9 — direct child process spawn):
//! - Each session gets its own agent subprocess, spawned directly via stdin/stdout pipes.
//! - A per-agent reader thread dispatches Agent2Ui events from stdout to Tauri events.
//! - Tauri commands write Ui2Agent frames directly to the agent's stdin pipe.
//! - `shutdown_all()` kills all child processes directly.

pub mod platform;
pub mod registry;
pub mod util;
pub mod commands;

// Re-export all public API so external callers see the same paths.
pub use platform::*;
pub use registry::*;
pub use util::*;
pub use commands::*;
