//! Cross-platform PTY (pseudo-terminal) abstraction.
//!
//! - Windows: [`conpty`] (CreatePseudoConsole API)
//! - Unix: `libc::forkpty` (POSIX PTY)
//!
//! Provides a unified [`spawn`] entrypoint that replaces manual shell wrapping
//! (`pwsh -Command`/`bash -c`) with proper terminal semantics.

// ── Common types ──

use std::io;

/// Exit status of a PTY child process.
#[derive(Debug, Clone, Copy)]
pub struct ExitStatus {
    code: i32,
    success: bool,
}

impl ExitStatus {
    pub fn code(&self) -> i32 { self.code }
    pub fn success(&self) -> bool { self.success }
}

/// A spawned PTY process.
pub struct PtyProcess {
    inner: Imp,
    output: Option<Box<dyn io::Read + Send>>,
}

impl PtyProcess {
    /// Get the process PID.
    pub fn pid(&self) -> u32 {
        self.inner.pid()
    }

    /// Take ownership of the output reader. Call once.
    pub fn take_output(&mut self) -> Option<Box<dyn io::Read + Send>> {
        self.output.take()
    }

    /// Check if the process is still running.
    pub fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }

    /// Wait for process to exit, with optional timeout in milliseconds.
    /// Returns the exit status.
    pub fn wait(&self, timeout_millis: Option<u64>) -> io::Result<ExitStatus> {
        self.inner.wait(timeout_millis)
    }

    /// Kill the process with exit code 1.
    pub fn kill(&mut self) -> io::Result<()> {
        self.inner.kill()
    }
}

// ── Platform implementations ──

#[cfg(target_os = "windows")]
#[path = "pty_windows.rs"]
mod imp;
#[cfg(not(target_os = "windows"))]
#[path = "pty_unix.rs"]
mod imp;

use imp::Imp;

/// Spawn a command in a PTY.
///
/// `command` is the full shell command line (e.g. `"git log --oneline"`).
/// On Windows this is executed via `pwsh -Command`. On Unix via `sh -c`.
/// `cwd` optionally sets the working directory for the child process.
///
/// Returns a [`PtyProcess`] from which stdout can be read with PTY semantics
/// (ANSI colors preserved, `isatty()`=true for the child).
pub fn spawn(command: &str, cwd: Option<&str>) -> io::Result<PtyProcess> {
    imp::spawn(command, cwd)
}
