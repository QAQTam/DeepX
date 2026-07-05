//! deepxd — DeepX daemon entry point.
//!
//! Called from the unified binary via `deepx daemon`.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deepx_proto::FrontendToDaemon;

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
        // Windows uses named pipes — listener is created per-connection
        // in the accept thread below.
    }

    // ── Initialize ──
    let mut pool = AgentPool::new();
    let frontends = Arc::new(Mutex::new(FrontendManager::new()));

    // Channel: reader threads → main loop (conn_id, parsed frame)
    let (frame_tx, frame_rx) = std::sync::mpsc::channel::<(usize, FrontendToDaemon)>();

    // Accept connections in background thread
    #[cfg(unix)]
    let (conn_tx, conn_rx) = std::sync::mpsc::channel::<std::os::unix::net::UnixStream>();
    #[cfg(windows)]
    let (conn_tx, conn_rx) = std::sync::mpsc::channel::<std::fs::File>();
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
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            loop {
                let pipe = match crate::transport::win::bind(&path) {
                    Ok(p) => p,
                    Err(e) => {
                        log::error!("bind error: {e}");
                        break;
                    }
                };
                let connected = match crate::transport::win::accept(pipe) {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("accept error: {e}");
                        break;
                    }
                };
                if conn_tx.send(connected).is_err() { break; }
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
                // Clone the stream so we can keep one half for writing
                // and spawn a reader thread on the other.
                let reader_stream = match stream.try_clone() {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[DAEMON] try_clone failed: {e}");
                        continue;
                    }
                };

                let conn_id = frontends.lock().unwrap().add(Box::new(stream));
                log::info!("[DAEMON] frontend {} connected", conn_id);

                let frontends = Arc::clone(&frontends);
                let frame_tx = frame_tx.clone();

                std::thread::spawn(move || {
                    let mut r = reader_stream;
                    loop {
                        // ── 4-byte LE length prefix ──
                        let mut len_buf = [0u8; 4];
                        match r.read_exact(&mut len_buf) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                            Err(e) => {
                                log::error!("[DAEMON] frontend {} read error: {e}", conn_id);
                                break;
                            }
                        }
                        let len = u32::from_le_bytes(len_buf) as usize;
                        if len > 16 * 1024 * 1024 {
                            log::error!("[DAEMON] frontend {} frame too large ({len} bytes)", conn_id);
                            break;
                        }

                        // ── JSON payload ──
                        let mut payload = vec![0u8; len];
                        if let Err(e) = r.read_exact(&mut payload) {
                            log::error!("[DAEMON] frontend {} read payload: {e}", conn_id);
                            break;
                        }

                        let frame: FrontendToDaemon = match serde_json::from_slice(&payload) {
                            Ok(f) => f,
                            Err(e) => {
                                log::error!("[DAEMON] frontend {} bad frame: {e}", conn_id);
                                break;
                            }
                        };

                        // Send to main loop for processing
                        if frame_tx.send((conn_id, frame)).is_err() {
                            // Main loop has shut down
                            break;
                        }
                    }

                    // Stream EOF or read error — unregister this frontend
                    frontends.lock().unwrap().remove(conn_id);
                    log::info!("[DAEMON] frontend {} disconnected", conn_id);
                });
            }
        }
        #[cfg(windows)]
        {
            while let Ok(stream) = conn_rx.try_recv() {
                let reader_stream = match stream.try_clone() {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[DAEMON] try_clone failed: {e}");
                        continue;
                    }
                };

                let conn_id = frontends.lock().unwrap().add(Box::new(stream));
                log::info!("[DAEMON] frontend {} connected", conn_id);

                let frontends = Arc::clone(&frontends);
                let frame_tx = frame_tx.clone();

                std::thread::spawn(move || {
                    let mut r = reader_stream;
                    loop {
                        let mut len_buf = [0u8; 4];
                        match r.read_exact(&mut len_buf) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                            Err(e) => {
                                log::error!("[DAEMON] frontend {} read error: {e}", conn_id);
                                break;
                            }
                        }
                        let len = u32::from_le_bytes(len_buf) as usize;
                        if len > 16 * 1024 * 1024 {
                            log::error!("[DAEMON] frontend {} frame too large ({len} bytes)", conn_id);
                            break;
                        }

                        let mut payload = vec![0u8; len];
                        if let Err(e) = r.read_exact(&mut payload) {
                            log::error!("[DAEMON] frontend {} read payload: {e}", conn_id);
                            break;
                        }

                        let frame: FrontendToDaemon = match serde_json::from_slice(&payload) {
                            Ok(f) => f,
                            Err(e) => {
                                log::error!("[DAEMON] frontend {} bad frame: {e}", conn_id);
                                break;
                            }
                        };

                        if frame_tx.send((conn_id, frame)).is_err() {
                            break;
                        }
                    }

                    frontends.lock().unwrap().remove(conn_id);
                    log::info!("[DAEMON] frontend {} disconnected", conn_id);
                });
            }
        }

        // Process incoming frames from reader threads
        while let Ok((conn_id, frame)) = frame_rx.try_recv() {
            let mut f = frontends.lock().unwrap();
            if let Err(e) = f.handle_frame(conn_id, frame, &pool) {
                log::error!("[DAEMON] handle_frame {} failed: {e}", conn_id);
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
                frontends.lock().unwrap().broadcast(&event);
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
