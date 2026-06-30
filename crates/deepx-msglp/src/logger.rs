//! Minimal file-based logger for the agent child process.
//! Writes to `agent.log` in the data directory.

use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

struct FileLogger {
    file: Mutex<File>,
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let mut f = self.file.lock().expect("agent log lock");
        let _ = writeln!(
            f,
            "[{}] {} | {}",
            record.level(),
            record.target(),
            record.args()
        );
        let _ = f.flush(); // ensure crash visibility
    }

    fn flush(&self) {
        let _ = self.file.lock().expect("agent log lock").flush();
    }
}

/// Initialize the log subscriber to write to `agent.log` under `data_dir`.
/// Panics if the log file cannot be created.
pub fn init_agent_logger(data_dir: &std::path::Path) -> Result<(), SetLoggerError> {
    let _ = std::fs::create_dir_all(data_dir);
    let log_path = data_dir.join("agent.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|e| panic!("Cannot open log file {}: {e}", log_path.display()));
    let logger = FileLogger {
        file: Mutex::new(file),
    };
    log::set_max_level(LevelFilter::Info);
    log::set_boxed_logger(Box::new(logger))
}
