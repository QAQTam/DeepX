//! deepx-daemon: background service managing agent processes.
//!
//! Listens on a TCP loopback socket, accepts frontend connections (Tauri, TUI),
//! and routes messages between frontends and per-session agent processes.
//!
//! Architecture:
//! ```text
//! Frontend ──TCP──→ Daemon ──stdin/stdout──→ Agent (per seed)
//! ```
//!
//! Protocol: 4-byte LE length prefix + JSON (FrontendToDaemon / DaemonToFrontend).

pub mod transport;
pub mod pool;
pub mod frontend;
mod main_loop;

pub use main_loop::run;

use std::path::PathBuf;

/// Path to the port file (daemon writes its TCP port here on startup).
pub fn port_path() -> PathBuf {
    deepx_types::platform::data_dir().join("deepxd.port")
}

/// Read the daemon's TCP port from the port file.
/// Returns None if the file doesn't exist or contains invalid data.
pub fn read_port() -> Option<u16> {
    let path = port_path();
    let data = std::fs::read_to_string(&path).ok()?;
    data.trim().parse().ok()
}

/// Write the daemon's TCP port to the port file.
pub fn write_port(port: u16) -> std::io::Result<()> {
    let path = port_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, port.to_string())
}
