use std::path::PathBuf;

/// Cross-platform home directory.
/// - Windows: `USERPROFILE`
/// - Unix: `HOME`
pub fn home_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_default()
    } else {
        std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
    }
}

/// deepx data directory (config, sessions, plans).
/// - Windows: `%USERPROFILE%\.deepx`
/// - Unix: `$XDG_CONFIG_HOME/deepx` or `$HOME/.config/deepx`
pub fn data_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        home_dir().join(".deepx")
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".config"))
            .join("deepx")
    }
}

/// deepx config file path.
pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

/// deepx HP port file path.
pub fn hp_port_path() -> PathBuf {
    data_dir().join("hp.port")
}

/// deepx sessions directory.
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// deepx plans directory.
pub fn plans_dir() -> PathBuf {
    data_dir().join("plans")
}

/// deepx workspace path file.
pub fn workspace_path() -> PathBuf {
    data_dir().join("workspace.txt")
}

/// Kill a process by PID (cross-platform).
/// - Windows: `taskkill /F /PID`
/// - Unix: `kill -9`
pub fn kill_process(pid: u32) {
    if cfg!(target_os = "windows") {
        drop(std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output());
    } else {
        drop(std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output());
    }
}

