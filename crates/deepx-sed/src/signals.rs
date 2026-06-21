// Copyright (c) 2026 Red Authors
// License: MIT
//

#[cfg(unix)]
pub mod unix {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// Global flag to indicate if a signal was received
    static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

    // Global list of temporary files to clean up on signal
    lazy_static::lazy_static! {
        static ref TEMP_FILES: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    }

    /// Initialize signal handlers for SIGINT and SIGTERM
    pub fn setup_signal_handlers() -> Result<(), Box<dyn std::error::Error>> {
        use signal_hook::consts::{SIGINT, SIGTERM};
        use signal_hook::iterator::Signals;
        use std::thread;

        let mut signals = Signals::new(&[SIGINT, SIGTERM])?;

        // Spawn a thread to handle signals
        thread::spawn(move || {
            for sig in signals.forever() {
                match sig {
                    SIGINT | SIGTERM => {
                        SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
                        cleanup_temp_files();
                        std::process::exit(130); // 128 + SIGINT(2) = standard shell convention
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    /// Register a temporary file for cleanup
    pub fn register_temp_file(path: String) {
        if let Ok(mut files) = TEMP_FILES.lock() {
            files.push(path);
        }
    }

    /// Unregister a temporary file (called after successful rename)
    pub fn unregister_temp_file(path: &str) {
        if let Ok(mut files) = TEMP_FILES.lock() {
            files.retain(|f| f != path);
        }
    }

    /// Clean up all registered temporary files
    fn cleanup_temp_files() {
        if let Ok(files) = TEMP_FILES.lock() {
            for file in files.iter() {
                // Silently try to remove the file (best-effort)
                let _ = std::fs::remove_file(file);
            }
        }
    }

    /// Check if a signal was received
    pub fn was_interrupted() -> bool {
        SIGNAL_RECEIVED.load(Ordering::SeqCst)
    }
}

#[cfg(not(unix))]
pub mod unix {
    /// No-op implementation for non-Unix systems
    pub fn setup_signal_handlers() -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn register_temp_file(_path: String) {}

    pub fn unregister_temp_file(_path: &str) {}

    pub fn was_interrupted() -> bool {
        false
    }
}
