//! deepx-daemon: background service managing agent processes.
//!
//! Listens on a local socket, accepts frontend connections (Tauri, TUI),
//! and routes messages between frontends and per-session agent processes.
//!
//! Architecture:
//! ```text
//! Frontend ──socket──→ Daemon ──stdin/stdout──→ Agent (per seed)
//! ```
//!
//! Protocol: 4-byte LE length prefix + JSON (FrontendToDaemon / DaemonToFrontend).

pub mod transport;
pub mod pool;
pub mod frontend;
mod main_loop;

pub use main_loop::run;

use std::path::PathBuf;

/// Default socket path for daemon communication.
pub fn socket_path() -> PathBuf {
    let dir = deepx_types::platform::data_dir();
    #[cfg(target_family = "unix")]
    { dir.join("deepxd.sock") }
    #[cfg(target_family = "windows")]
    { dir.join("deepxd.pipe") }
}
