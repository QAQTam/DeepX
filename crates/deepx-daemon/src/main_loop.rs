//! deepxd — DeepX daemon entry point.
//!
//! Called from the unified binary via `deepx daemon`.

use std::io::Read;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deepx_proto::FrontendToDaemon;

use crate::{pool::AgentPool, frontend::FrontendManager, write_port};

/// Main entry point for the daemon (called from the unified binary).
pub fn run() {
    // ── Bind TCP listener on random port ──
    let (listener, port) = match crate::transport::bind() {
        Ok((l, p)) => {
            log::info!("deepxd starting on 127.0.0.1:{}", p);
            (l, p)
        }
        Err(e) => {
            log::error!("Failed to bind TCP listener: {e}");
            std::process::exit(1);
        }
    };

    // Write port to file for clients to discover
    if let Err(e) = write_port(port) {
        log::error!("Failed to write port file: {e}");
        std::process::exit(1);
    }

    // ── Initialize ──
    let mut pool = AgentPool::new();
    let frontends = Arc::new(Mutex::new(FrontendManager::new()));

    // Channel: reader threads → main loop (conn_id, parsed frame)
    let (frame_tx, frame_rx) = std::sync::mpsc::channel::<(usize, FrontendToDaemon)>();

    // Accept connections in background thread
    let (conn_tx, conn_rx) = std::sync::mpsc::channel::<TcpStream>();

    std::thread::spawn(move || {
        loop {
            match crate::transport::accept(&listener) {
                Ok(stream) => {
                    if conn_tx.send(stream).is_err() { break; }
                }
                Err(e) => {
                    log::error!("accept error: {e}");
                    break;
                }
            }
        }
    });

    // ── Main event loop ──
    let reap_interval = Duration::from_secs(60);
    let mut last_reap = std::time::Instant::now();

    loop {
        // Accept new frontend connections
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
