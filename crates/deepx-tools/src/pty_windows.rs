//! Windows PTY backend via `conpty`.
//!
//! Uses `pwsh -Command` (stdin pipe) to avoid `-EncodedCommand` (Base64)
//! which triggers Windows Defender false positives (Trojan:Win32/ClickFix).
//! Variables and special characters are preserved via PTY stdin input.

use std::io;
use std::io::Write as _;
use std::process::Command;

use super::ExitStatus;

pub struct Imp {
    proc: conpty::Process,
    exit_cached: Option<ExitStatus>,
    detached: bool,
}

impl Imp {
    pub fn pid(&self) -> u32 {
        self.proc.pid()
    }

    /// Check if the child process has exited.
    /// Uses non-blocking wait instead of conpty's is_alive()
    /// (which checks the conpty host, not the actual command).
    pub fn is_alive(&mut self) -> bool {
        // wait(Some(0)) = non-blocking poll. Err means still running.
        match self.proc.wait(Some(0)) {
            Ok(_) => {
                self.exit_cached = Some(ExitStatus { code: 0, success: true });
                false
            }
            Err(_) => true,
        }
    }

    pub fn wait(&self, timeout_millis: Option<u64>) -> io::Result<ExitStatus> {
        if let Some(ref es) = self.exit_cached {
            return Ok(es.clone());
        }
        let timeout = timeout_millis.map(|t| t as u32);
        self.proc.wait(timeout)
            .map(|_| ExitStatus { code: 0, success: true })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.proc.exit(1).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn detach(&mut self) {
        self.detached = true;
    }
}

impl Drop for Imp {
    fn drop(&mut self) {
        if !self.detached {
            let _ = self.proc.exit(1);
            let _ = self.proc.wait(None);
        }
    }
}

pub fn spawn(command: &str, cwd: Option<&str>) -> io::Result<super::PtyProcess> {
    let mut cmd = Command::new("pwsh");
    // -Command - : read commands from stdin (avoids -EncodedCommand Defender FP).
    cmd.args(["-NoLogo", "-NoProfile", "-Command", "-"]);
    // Suppress console window flash.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut proc = conpty::Process::spawn(cmd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    // Write the command to pwsh via PTY stdin.
    // Prepend env-var overrides so pager calls don't block.
    let full_command = format!(
        "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $env:GIT_PAGER='cat'; $env:PAGER='cat'; $env:SYSTEMD_PAGER='cat'; {}\n",
        command
    );
    {
        let mut stdin = proc.input()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        stdin.write_all(full_command.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        stdin.flush()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    }

    let output: Box<dyn io::Read + Send> = Box::new(
        proc.output()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
    );

    // No further stdin needed — command already written.
    Ok(super::PtyProcess {
        inner: Imp { proc, exit_cached: None, detached: false },
        output: Some(output),
        input: None,
    })
}
