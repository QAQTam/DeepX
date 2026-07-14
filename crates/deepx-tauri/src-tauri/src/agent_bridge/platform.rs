//! Platform detection: OS info, PATH caching, toolchain version detection.
//!
//! Called once at startup from `main.rs` before Tauri initialization.

use std::sync::OnceLock;

/// Cached full system PATH captured at startup (Windows GUI apps get stripped PATH).
pub(crate) static SYSTEM_PATH: OnceLock<String> = OnceLock::new();

/// Capture the full system PATH at process startup, before any Windows GUI stripping.
/// Must be called from main() early, before Tauri initialization.
pub fn cache_system_path() {
    let mut path = std::env::var("PATH").unwrap_or_default();

    // On Windows GUI apps, the process PATH may be stripped. Read the full
    // system+user PATH from the registry as a reliable fallback.
    #[cfg(target_os = "windows")]
    {
        let reg_path = windows_reg_path();
        if !reg_path.is_empty() {
            // Merge with current PATH, deduplicating
            let mut seen: std::collections::HashSet<String> =
                path.split(';').map(|s| s.to_string()).collect();
            for segment in reg_path.split(';') {
                if !segment.is_empty() && seen.insert(segment.to_string()) {
                    if !path.is_empty() {
                        path.push(';');
                    }
                    path.push_str(segment);
                }
            }
        }
    }

    let _ = SYSTEM_PATH.set(path.clone());
    // Apply the full PATH to the current process so all child processes
    // (agent subprocess, pwsh via conpty, daemon, etc.) inherit it automatically.
    unsafe {
        std::env::set_var("PATH", &path);
    }
}

/// Detect OS version and store it for injection into the system prompt [SESSION] block.
/// Must be called from main() early, before any session is created.
pub fn detect_os_info() {
    #[cfg(target_os = "windows")]
    {
        let info = windows_os_info();
        if !info.is_empty() {
            let _ = deepx_config::prompt::OS_INFO.set(info);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let info = unix_os_info();
        let _ = deepx_config::prompt::OS_INFO.set(info);
    }
    // Detect shell + toolchain versions
    let tools = detect_tools();
    let _ = deepx_config::prompt::TOOLS_INFO.set(tools);
}

// ═══════════════════════════════════════════════════════════════
// Windows Registry FFI (PATH + OS version detection)
// ═══════════════════════════════════════════════════════════════

#[cfg(target_os = "windows")]
fn windows_reg_path() -> String {
    unsafe {
        // Win32 FFI declarations
        unsafe extern "system" {
            fn RegOpenKeyExW(
                hkey: isize,
                subkey: *const u16,
                _uloptions: u32,
                _samdesired: u32,
                phkresult: *mut isize,
            ) -> i32;
            fn RegQueryValueExW(
                hkey: isize,
                value: *const u16,
                _reserved: *const u8,
                pdwtype: *mut u32,
                pbdata: *mut u8,
                pcbdata: *mut u32,
            ) -> i32;
            fn RegCloseKey(hkey: isize) -> i32;
        }

        const HKEY_LOCAL_MACHINE: isize = -2147483646i64 as isize; // 0x80000002
        const HKEY_CURRENT_USER: isize = -2147483647i64 as isize; // 0x80000001
        const KEY_READ: u32 = 0x20019;

        let mut result = String::new();

        for (hkey, subkey_str) in [
            (
                HKEY_LOCAL_MACHINE,
                "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment\0",
            ),
            (HKEY_CURRENT_USER, "Environment\0"),
        ] {
            let subkey_wide: Vec<u16> = subkey_str.encode_utf16().collect();
            let value_name: Vec<u16> = "PATH\0".encode_utf16().collect();
            let mut key_handle: isize = 0;

            if RegOpenKeyExW(hkey, subkey_wide.as_ptr(), 0, KEY_READ, &mut key_handle) != 0 {
                continue;
            }

            let mut data_type: u32 = 0;
            let mut data_size: u32 = 0;

            if RegQueryValueExW(
                key_handle,
                value_name.as_ptr(),
                std::ptr::null(),
                &mut data_type,
                std::ptr::null_mut(),
                &mut data_size,
            ) != 0
                || data_size == 0
            {
                RegCloseKey(key_handle);
                continue;
            }

            let mut buf: Vec<u16> = vec![0u16; (data_size / 2) as usize + 1];
            if RegQueryValueExW(
                key_handle,
                value_name.as_ptr(),
                std::ptr::null(),
                &mut data_type,
                buf.as_mut_ptr() as *mut u8,
                &mut data_size,
            ) != 0
            {
                RegCloseKey(key_handle);
                continue;
            }
            RegCloseKey(key_handle);

            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let path = String::from_utf16_lossy(&buf[..len]);

            if !result.is_empty() {
                result.push(';');
            }
            result.push_str(&path);
        }

        result
    }
}

/// Read a string value from a Windows registry key (returns empty if not found).
#[cfg(target_os = "windows")]
fn reg_read_string(hkey: isize, subkey_str: &str, value_name_str: &str) -> String {
    unsafe {
        unsafe extern "system" {
            fn RegOpenKeyExW(
                hkey: isize,
                subkey: *const u16,
                _uloptions: u32,
                _samdesired: u32,
                phkresult: *mut isize,
            ) -> i32;
            fn RegQueryValueExW(
                hkey: isize,
                value: *const u16,
                _reserved: *const u8,
                pdwtype: *mut u32,
                pbdata: *mut u8,
                pcbdata: *mut u32,
            ) -> i32;
            fn RegCloseKey(hkey: isize) -> i32;
        }
        const KEY_READ: u32 = 0x20019;
        let subkey_wide: Vec<u16> = subkey_str.encode_utf16().collect();
        let value_wide: Vec<u16> = value_name_str.encode_utf16().collect();
        let mut key_handle: isize = 0;
        if RegOpenKeyExW(hkey, subkey_wide.as_ptr(), 0, KEY_READ, &mut key_handle) != 0 {
            return String::new();
        }
        let mut data_type: u32 = 0;
        let mut data_size: u32 = 0;
        if RegQueryValueExW(
            key_handle,
            value_wide.as_ptr(),
            std::ptr::null(),
            &mut data_type,
            std::ptr::null_mut(),
            &mut data_size,
        ) != 0
            || data_size == 0
        {
            RegCloseKey(key_handle);
            return String::new();
        }
        let mut buf: Vec<u16> = vec![0u16; (data_size / 2) as usize + 1];
        if RegQueryValueExW(
            key_handle,
            value_wide.as_ptr(),
            std::ptr::null(),
            &mut data_type,
            buf.as_mut_ptr() as *mut u8,
            &mut data_size,
        ) != 0
        {
            RegCloseKey(key_handle);
            return String::new();
        }
        RegCloseKey(key_handle);
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }
}

/// Read a REG_DWORD value from the registry. Returns 0 on failure.
#[cfg(target_os = "windows")]
fn reg_read_dword(hkey: isize, subkey_str: &str, value_name_str: &str) -> u32 {
    unsafe {
        unsafe extern "system" {
            fn RegOpenKeyExW(
                hkey: isize,
                subkey: *const u16,
                _uloptions: u32,
                _samdesired: u32,
                phkresult: *mut isize,
            ) -> i32;
            fn RegQueryValueExW(
                hkey: isize,
                value: *const u16,
                _reserved: *const u8,
                pdwtype: *mut u32,
                pbdata: *mut u8,
                pcbdata: *mut u32,
            ) -> i32;
            fn RegCloseKey(hkey: isize) -> i32;
        }
        const KEY_READ: u32 = 0x20019;
        let subkey_wide: Vec<u16> = subkey_str.encode_utf16().collect();
        let value_wide: Vec<u16> = value_name_str.encode_utf16().collect();
        let mut key_handle: isize = 0;
        if RegOpenKeyExW(hkey, subkey_wide.as_ptr(), 0, KEY_READ, &mut key_handle) != 0 {
            return 0;
        }
        let mut data_type: u32 = 0;
        let mut data: u32 = 0;
        let mut data_size: u32 = 4;
        if RegQueryValueExW(
            key_handle,
            value_wide.as_ptr(),
            std::ptr::null(),
            &mut data_type,
            &mut data as *mut u32 as *mut u8,
            &mut data_size,
        ) != 0
        {
            RegCloseKey(key_handle);
            return 0;
        }
        RegCloseKey(key_handle);
        data
    }
}

/// Build an OS info string like "Windows NT 10.0.26200.8737 (25H2)".
#[cfg(target_os = "windows")]
fn windows_os_info() -> String {
    let major = reg_read_dword(
        -2147483646i64 as isize,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0",
        "CurrentMajorVersionNumber\0",
    );
    let minor = reg_read_dword(
        -2147483646i64 as isize,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0",
        "CurrentMinorVersionNumber\0",
    );
    let build_str = reg_read_string(
        -2147483646i64 as isize,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0",
        "CurrentBuild\0",
    );
    let ubr = reg_read_dword(
        -2147483646i64 as isize,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0",
        "UBR\0",
    );
    if build_str.is_empty() {
        return String::new();
    }
    let display = reg_read_string(
        -2147483646i64 as isize,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0",
        "DisplayVersion\0",
    );
    if display.is_empty() {
        format!("Windows NT {}.{}.{}.{}", major, minor, build_str, ubr)
    } else {
        format!(
            "Windows NT {}.{}.{}.{} ({})",
            major, minor, build_str, ubr, display
        )
    }
}

/// Detect OS info on Unix via uname.
#[cfg(not(target_os = "windows"))]
fn unix_os_info() -> String {
    use std::process::Command;
    let sysname = Command::new("uname")
        .arg("-s")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let release = Command::new("uname")
        .arg("-r")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if sysname.is_empty() {
        return String::new();
    }
    if release.is_empty() {
        sysname
    } else {
        format!("{} {}", sysname, release)
    }
}

/// Quick scan of shell version and common toolchains on PATH.
fn detect_tools() -> String {
    use std::process::Command;
    /// Run a command, return first line of output or empty.
    fn try_version(cmd: &str, args: &[&str]) -> Option<String> {
        let child = Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        let output = child.wait_with_output().ok()?;
        // Some tools (python, java) output version to stderr
        let raw = if output.stdout.is_empty() {
            &output.stderr
        } else {
            &output.stdout
        };
        let s = String::from_utf8_lossy(raw);
        let first_line = s.lines().next().unwrap_or("").trim().to_string();
        if first_line.is_empty() {
            None
        } else {
            Some(first_line)
        }
    }
    // Ordered: shell first, then important toolchains
    let probes: &[(&str, &[&str])] = &[
        #[cfg(target_os = "windows")]
        ("pwsh", &["--version"]),
        #[cfg(not(target_os = "windows"))]
        ("bash", &["--version"]),
        ("rustc", &["--version"]),
        ("cargo", &["--version"]),
        ("python", &["--version"]),
        ("python3", &["--version"]),
        ("node", &["--version"]),
        ("git", &["--version"]),
        ("java", &["--version"]),
    ];
    let mut parts: Vec<String> = Vec::new();
    for (cmd, args) in probes {
        if let Some(v) = try_version(cmd, args) {
            // Compact: "rustc 1.92.0" or "pwsh 7.4.6"
            // Keep first 60 chars to avoid junk
            let short = if v.len() > 80 {
                let boundary = v.floor_char_boundary(77);
                format!("{}...", &v[..boundary])
            } else {
                v
            };
            parts.push(short);
        }
    }
    parts.join(" | ")
}
