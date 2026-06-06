use std::net::TcpStream;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use dsx_proto::{self, AgentToHp};

/// Stores the most recently spawned gate daemon child process so it can be
/// killed during shutdown. Without this, `try_reconnect()` orphans every
/// process it spawns.
static HP_DAEMON: OnceLock<Mutex<Child>> = OnceLock::new();

/// Read HP port from port file.
pub(crate) fn hp_port() -> u16 {
    let path = dsx_types::platform::hp_port_path();
    std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Connect to HP, register, and return the TCP stream.
pub fn connect() -> Option<TcpStream> {
    let port = hp_port();
    if port == 0 {
        eprintln!("dsx-agent: gate not running (no hp.port) — continuing without AI");
        return None;
    }

    let mut stream = match TcpStream::connect(format!("127.0.0.1:{port}")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dsx-agent: cannot connect gate: {e}");
            return None;
        }
    };

    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let pid = std::process::id();
    let reg = AgentToHp::Register {
        kind: "Agent".into(),
        name: "dsx-agent".into(),
        pid,
    };
    let _ = dsx_proto::write_frame(&mut stream, &reg);

    eprintln!("dsx-agent: connected to gate on port {port}");
    Some(stream)
}

/// Kill the HP daemon child process spawned by `try_reconnect()`, if any.
/// Called during shutdown to prevent orphan processes.
pub fn kill_hp_daemon() {
    if let Some(lock) = HP_DAEMON.get() {
        if let Ok(mut child) = lock.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Try to (re)connect to HP daemon. If port file is stale, spawn a new HP.
pub fn try_reconnect() -> Option<TcpStream> {
    let port_path = dsx_types::platform::hp_port_path();

    let port = std::fs::read_to_string(&port_path).ok()
        .and_then(|s| s.trim().parse::<u16>().ok());

    if let Some(p) = port {
        if let Ok(mut stream) = TcpStream::connect(format!("127.0.0.1:{p}")) {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
            let reg = AgentToHp::Register {
                kind: "Agent".into(),
                name: "dsx-agent".into(),
                pid: std::process::id(),
            };
            let _ = dsx_proto::write_frame(&mut stream, &reg);
            return Some(stream);
        }
        let _ = std::fs::write(&port_path, "");
    }

    let current_exe = std::env::current_exe().ok()?;

    // current_exe IS the dsx umbrella binary — just run it with "gate" arg
    if let Ok(mut child) = std::process::Command::new(&current_exe).arg("gate")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(s) = std::fs::read_to_string(&port_path) {
                if let Ok(p) = s.trim().parse::<u16>() {
                    if let Ok(mut stream) = TcpStream::connect(format!("127.0.0.1:{p}")) {
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                        let reg = AgentToHp::Register {
                            kind: "Agent".into(),
                            name: "dsx-agent".into(),
                            pid: std::process::id(),
                        };
                        let _ = dsx_proto::write_frame(&mut stream, &reg);
                        // Store child for cleanup; kill previous if reconnecting
                        if let Some(lock) = HP_DAEMON.get() {
                            if let Ok(mut guard) = lock.lock() {
                                let _ = guard.kill();
                                let _ = guard.wait();
                                *guard = child;
                            }
                        } else {
                            let _ = HP_DAEMON.set(Mutex::new(child));
                        }
                        return Some(stream);
                    }
                }
            }
            if let Ok(Some(_)) = child.try_wait() { break; }
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    None
}
