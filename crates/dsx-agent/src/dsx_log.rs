use simplelog::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static LOGGER_INIT: OnceLock<bool> = OnceLock::new();
static HAS_SESSION: AtomicBool = AtomicBool::new(false);
static LOG_PATH: OnceLock<std::sync::Mutex<PathBuf>> = OnceLock::new();

fn level() -> LevelFilter {
    match std::env::var("DSX_LOG").as_deref() {
        Ok("trace") => LevelFilter::Trace,
        Ok("debug") => LevelFilter::Debug,
        Ok("info") => LevelFilter::Info,
        Ok("warn") => LevelFilter::Warn,
        Ok("error") => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}

fn log_dir() -> PathBuf {
    let mut p = config_dir();
    p.push("logs");
    let _ = std::fs::create_dir_all(&p);
    p
}

fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DSX_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    dsx_types::platform::data_dir()
}

/// Initialize file logging. Call once at startup.
/// Logs to {data_dir}/logs/ (overwritten each run).
/// Set DSX_LOG=debug|trace|info|warn|error to control verbosity.
pub fn init() {
    LOGGER_INIT.get_or_init(|| {
        let lvl = level();
        let mut path = log_dir();
        path.push("dsx.pending.log");

        let config = ConfigBuilder::new()
            .set_max_level(lvl)
            .set_time_format_rfc3339()
            .set_time_offset_to_local()
            .unwrap_or_else(|c| c)
            .build();

        let _ = LOG_PATH.set(std::sync::Mutex::new(path.clone()));

        let file = match std::fs::File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[dsx] log file create error: {}", e);
                return true;
            }
        };
        match WriteLogger::init(lvl, config, file) {
            Ok(()) => log::info!("Log started: {}", path.display()),
            Err(e) => eprintln!("[dsx] log: {}", e),
        }
        true
    });
}

/// Rename log to {seed}.{date}.log once session seed is known.
pub fn set_session(seed: &str) {
    if HAS_SESSION.swap(true, Ordering::Relaxed) { return; }
    let Some(lock) = LOG_PATH.get() else { return };
    let Ok(mut path) = lock.lock() else { return };
    let date = date_tag();
    let new_name = format!("{}.{}.log", seed, date);
    path.set_file_name(&new_name);
    let old_path = path.with_file_name("dsx.pending.log");
    let _ = std::fs::rename(&old_path, &*path);
    log::info!("log: {} → {}", old_path.display(), path.display());
}

fn date_tag() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64 + 719468); // 719468 = days from 0000-01-01 to 1970-01-01
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}
