//! deepxd — DeepX daemon entry point.
//!
//! Called from the unified binary via `deepx daemon`.

use std::time::Duration;

use crate::{pool::AgentPool, frontend::FrontendManager, socket_path};

/// Main entry point for the daemon (called from the unified binary).
pub fn run() {
    let path = socket_path();
    log::info!("deepxd starting, socket={}", path.display());

    // ── Bind socket ──
    #[cfg(unix)]
    let listener = match transport::unix::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to bind socket {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    #[cfg(windows)]
    {
        log::error!("Windows daemon not yet supported");
        return;
    }

    // ── Initialize ──
    let mut pool = AgentPool::new();
    let mut frontends = FrontendManager::new();

    // Accept connections in background thread
    #[cfg(unix)]
    let (conn_tx, conn_rx) = std::sync::mpsc::channel::<std::os::unix::net::UnixStream>();
    #[cfg(unix)]
    {
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        s.set_nonblocking(false).ok();
                        if conn_tx.send(s).is_err() { break; }
                    }
                    Err(e) => {
                        log::error!("accept error: {e}");
                        break;
                    }
                }
            }
        });
    }

    // ── Main event loop ──
    let reap_interval = Duration::from_secs(60);
    let mut last_reap = std::time::Instant::now();

    loop {
        // Accept new frontend connections
        #[cfg(unix)]
        {
            while let Ok(stream) = conn_rx.try_recv() {
                let conn_id = frontends.add(Box::new(stream));
                // Spawn reader for this frontend
                // (simplified — full impl would need bidirectional stream access)
                log::info!("[DAEMON] frontend {} connected", conn_id);
            }
        }

        // Reap idle agents
        if last_reap.elapsed() >= reap_interval {
            pool.reap_idle();
            last_reap = std::time::Instant::now();
        }

        // Process agent events
        match pool.event_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => {
                frontends.broadcast(&event);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::error!("Agent event channel disconnected — exiting");
                break;
            }
        }
    }

    pool.shutdown_all();
    log::info!("deepxd stopped");
}
