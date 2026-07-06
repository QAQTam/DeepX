// Copyright (c) 2026 Red Authors
// License: MIT
//

// Common test utilities for sed compatibility verification
// When VERIFY_SED=1 is set, tests will also compare output with GNU sed
//
// Usage in other test files:
//   mod common;
//   use common::{compare_sed_red, verify_against_sed, ...};
//
// To run all tests with sed verification:
//   VERIFY_SED=1 cargo test

#![allow(dead_code)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Check if sed verification mode is enabled via environment variable
/// When enabled, tests should also compare their output with GNU sed
pub fn verify_sed_enabled() -> bool {
    std::env::var("VERIFY_SED")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Get path to red binary
pub fn red_binary() -> &'static str {
    // Check paths in order of preference
    if std::path::Path::new("target/release/red").exists() {
        "target/release/red"
    } else if std::path::Path::new("target/debug/red").exists() {
        "target/debug/red"
    } else if std::path::Path::new("target/llvm-cov-target/debug/red").exists() {
        // Coverage builds place binaries here
        "target/llvm-cov-target/debug/red"
    } else {
        // Fallback to debug (will fail with clear error if not found)
        "target/debug/red"
    }
}

/// Run a command with input and return output
pub fn run_cmd(cmd: &str, args: &[&str], input: &[u8], env: Option<(&str, &str)>) -> Output {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some((key, val)) = env {
        command.env(key, val);
    }

    let mut child = command.spawn().expect("Failed to spawn command");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).expect("Failed to write to stdin");
    }

    child.wait_with_output().expect("Failed to read output")
}

/// Result of comparing sed and red
#[derive(Debug)]
pub struct CompareResult {
    pub matches: bool,
    pub sed_stdout: Vec<u8>,
    pub sed_status: i32,
    pub red_stdout: Vec<u8>,
    pub red_status: i32,
}

/// On Windows/MSYS2, backslashes in command arguments are consumed by the MSYS2
/// argument processing layer. We need to double them for sed to receive the correct script.
#[cfg(windows)]
fn escape_script_for_msys2_sed(script: &str) -> String {
    script.replace("\\", "\\\\")
}

/// Compare sed and red output with raw bytes input
pub fn compare_sed_red_bytes(
    script: &str,
    input: &[u8],
    extra_args: &[&str],
    env: Option<(&str, &str)>,
) -> CompareResult {
    // Skip comparison on non-GNU sed
    if !is_gnu_sed() {
        eprintln!("warning: skipping sed comparison (non-GNU sed detected)");
        return CompareResult {
            matches: true,
            sed_stdout: vec![],
            sed_status: 0,
            red_stdout: vec![],
            red_status: 0,
        };
    }

    // On Windows/MSYS2, we need to double backslashes for sed because the MSYS2
    // argument processing layer consumes one level of backslashes
    #[cfg(windows)]
    let sed_script = escape_script_for_msys2_sed(script);
    #[cfg(not(windows))]
    let sed_script = script.to_string();

    let mut sed_args: Vec<String> = extra_args.iter().map(|s| s.to_string()).collect();
    // On Windows, use binary mode to match Unix behavior (LF instead of CRLF)
    #[cfg(windows)]
    sed_args.insert(0, "-b".to_string());
    sed_args.push("-e".to_string());
    sed_args.push(sed_script);

    let sed_args_ref: Vec<&str> = sed_args.iter().map(|s| s.as_str()).collect();

    let mut red_args: Vec<&str> = extra_args.to_vec();
    // On Windows, use binary mode to match Unix behavior (LF instead of CRLF)
    #[cfg(windows)]
    red_args.insert(0, "-b");
    red_args.push("-e");
    red_args.push(script);

    let sed_out = run_cmd("sed", &sed_args_ref, input, env);
    let red_out = run_cmd(red_binary(), &red_args, input, env);

    let sed_status = sed_out.status.code().unwrap_or(-1);
    let red_status = red_out.status.code().unwrap_or(-1);

    let matches = sed_out.stdout == red_out.stdout && sed_status == red_status;

    CompareResult {
        matches,
        sed_stdout: sed_out.stdout,
        sed_status,
        red_stdout: red_out.stdout,
        red_status,
    }
}

/// Compare sed and red output for given script and input
pub fn compare_sed_red(script: &str, input: &str, extra_args: &[&str]) -> CompareResult {
    compare_sed_red_bytes(script, input.as_bytes(), extra_args, None)
}

/// Compare sed and red with specific locale
pub fn compare_with_locale(locale: &str, script: &str, input: &[u8]) -> CompareResult {
    compare_sed_red_bytes(script, input, &[], Some(("LC_ALL", locale)))
}

/// Check if a locale is available
pub fn locale_available(locale: &str) -> bool {
    let output = Command::new("locale")
        .arg("-a")
        .output()
        .expect("Failed to run locale -a");
    let available = String::from_utf8_lossy(&output.stdout);
    available.lines().any(|l| l == locale)
}

/// Check if the system sed is GNU sed
/// Returns false on Windows/MSYS2 sed which has different backreference behavior
pub fn is_gnu_sed() -> bool {
    let output = Command::new("sed").arg("--version").output().ok();

    match output {
        Some(out) => {
            let version = String::from_utf8_lossy(&out.stdout);
            version.contains("GNU sed")
        }
        None => false,
    }
}

/// Assert comparison matches, with helpful error message
#[macro_export]
macro_rules! assert_sed_match {
    ($r:expr, $desc:expr) => {
        assert!(
            $r.matches,
            "{}:\nsed stdout: {:?}\nred stdout: {:?}\nsed status: {}\nred status: {}",
            $desc,
            String::from_utf8_lossy(&$r.sed_stdout),
            String::from_utf8_lossy(&$r.red_stdout),
            $r.sed_status,
            $r.red_status
        );
    };
}

/// Verify a test against GNU sed if VERIFY_SED=1 is set
/// Use this in existing tests to add optional sed verification
///
/// # Example
/// ```ignore
/// use common::verify_against_sed;
///
/// #[test]
/// fn test_something() {
///     // ... your normal test assertions ...
///
///     // Optionally verify against GNU sed
///     verify_against_sed!("s/foo/bar/", "foo\n", &[]);
/// }
/// ```
#[macro_export]
macro_rules! verify_against_sed {
    ($script:expr, $input:expr, $extra_args:expr) => {
        if common::verify_sed_enabled() {
            let result = common::compare_sed_red($script, $input, $extra_args);
            assert!(
                result.matches,
                "GNU sed mismatch for script {:?}:\nsed: {:?} (status {})\nred: {:?} (status {})",
                $script,
                String::from_utf8_lossy(&result.sed_stdout),
                result.sed_status,
                String::from_utf8_lossy(&result.red_stdout),
                result.red_status
            );
        }
    };
    ($script:expr, $input:expr, $extra_args:expr, $locale:expr) => {
        if common::verify_sed_enabled() {
            let result = common::compare_with_locale($locale, $script, $input);
            assert!(
                result.matches,
                "GNU sed mismatch (locale {}) for script {:?}:\nsed: {:?} (status {})\nred: {:?} (status {})",
                $locale,
                $script,
                String::from_utf8_lossy(&result.sed_stdout),
                result.sed_status,
                String::from_utf8_lossy(&result.red_stdout),
                result.red_status
            );
        }
    };
}

/// Verify bytes against GNU sed if VERIFY_SED=1 is set
#[macro_export]
macro_rules! verify_against_sed_bytes {
    ($script:expr, $input:expr, $extra_args:expr) => {
        if common::verify_sed_enabled() {
            let result = common::compare_sed_red_bytes($script, $input, $extra_args, None);
            assert!(
                result.matches,
                "GNU sed mismatch for script {:?}:\nsed: {:?} (status {})\nred: {:?} (status {})",
                $script,
                String::from_utf8_lossy(&result.sed_stdout),
                result.sed_status,
                String::from_utf8_lossy(&result.red_stdout),
                result.red_status
            );
        }
    };
}
