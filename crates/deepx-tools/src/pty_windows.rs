//! Windows PTY backend via `conpty`.

use std::io;
use std::process::Command;

use super::ExitStatus;

pub struct Imp {
    proc: conpty::Process,
}

impl Imp {
    pub fn pid(&self) -> u32 {
        self.proc.pid()
    }

    pub fn is_alive(&self) -> bool {
        self.proc.is_alive()
    }

    pub fn wait(&self, timeout_millis: Option<u64>) -> io::Result<ExitStatus> {
        let timeout = timeout_millis.map(|t| t as u32);
        self.proc.wait(timeout)
            .map(|_| ExitStatus { code: 0, success: true })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.proc.exit(1).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

pub fn spawn(command: &str, cwd: Option<&str>) -> io::Result<super::PtyProcess> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", command]);
    // Suppress console window flash
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

    let output: Box<dyn io::Read + Send> = Box::new(
        proc.output()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
    );

    Ok(super::PtyProcess {
        inner: Imp { proc },
        output: Some(output),
    })
}
