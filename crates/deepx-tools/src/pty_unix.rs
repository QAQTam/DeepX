//! Unix PTY backend via `libc::forkpty`.

use std::io;
use std::os::unix::io::FromRawFd;

use super::ExitStatus;

pub struct Imp {
    pid: u32,
    detached: bool,
}

impl Imp {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn is_alive(&self) -> bool {
        unsafe { libc::kill(self.pid as i32, 0) == 0 }
    }

    pub fn wait(&self, _timeout_millis: Option<u64>) -> io::Result<ExitStatus> {
        let mut status: i32 = 0;
        let ret = unsafe { libc::waitpid(self.pid as i32, &mut status, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        let code = if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else if libc::WIFSIGNALED(status) {
            libc::WTERMSIG(status) + 128
        } else {
            -1
        };
        Ok(ExitStatus { code, success: code == 0 })
    }

    pub fn kill(&mut self) -> io::Result<()> {
        let ret = unsafe { libc::kill(self.pid as i32, libc::SIGKILL) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        // Reap the zombie
        unsafe { libc::waitpid(self.pid as i32, std::ptr::null_mut(), 0); }
        Ok(())
    }

    pub fn detach(&mut self) {
        self.detached = true;
    }
}

impl Drop for Imp {
    fn drop(&mut self) {
        if !self.detached {
            // Kill and reap child
            unsafe {
                libc::kill(self.pid as i32, libc::SIGKILL);
                libc::waitpid(self.pid as i32, std::ptr::null_mut(), 0);
            }
        }
    }
}

pub fn spawn(command: &str, cwd: Option<&str>) -> io::Result<super::PtyProcess> {
    use std::ffi::CString;

    let mut master_fd: libc::c_int = 0;

    let pid = unsafe {
        libc::forkpty(
            &mut master_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };

    if pid < 0 {
        return Err(io::Error::last_os_error());
    }

    if pid == 0 {
        // Child: set cwd, then execute shell with command
        if let Some(dir) = cwd {
            let dir_c = CString::new(dir).unwrap();
            if unsafe { libc::chdir(dir_c.as_ptr()) } != 0 {
                unsafe { libc::_exit(126); }
            }
        }

        let shell = if std::path::Path::new("/bin/bash").exists() {
            "/bin/bash"
        } else {
            "/bin/sh"
        };
        let shell_c = CString::new(shell).expect("shell path invalid");
        let arg_c = CString::new("-c").expect("-c is valid CString");
        let cmd_c = CString::new(command).expect("command contains NUL byte");

        unsafe {
            libc::execl(
                shell_c.as_ptr(),
                shell_c.as_ptr(),
                arg_c.as_ptr(),
                cmd_c.as_ptr(),
                std::ptr::null::<libc::c_char>(),
            );
            libc::_exit(127);
        }
    }

    // Parent: dup the fd for writing, wrap original for reading.
    // Both File objects own their fds and will close them on drop.
    let writer_fd = unsafe { libc::dup(master_fd) };
    if writer_fd < 0 {
        unsafe { libc::close(master_fd); }
        return Err(io::Error::last_os_error());
    }
    let reader = unsafe { std::fs::File::from_raw_fd(master_fd) };
    let writer = unsafe { std::fs::File::from_raw_fd(writer_fd) };
    let output: Box<dyn io::Read + Send> = Box::new(reader);
    let input: Option<Box<dyn io::Write + Send>> = Some(Box::new(writer));

    Ok(super::PtyProcess {
        inner: Imp {
            pid: pid as u32,
            detached: false,
        },
        output: Some(output),
        input,
    })
}
