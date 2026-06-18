//! Unix PTY backend via `libc::forkpty`.

use std::io;
use std::os::fd::RawFd;
use std::os::unix::io::FromRawFd;

use super::ExitStatus;

pub struct Imp {
    pid: u32,
    master_fd: RawFd,
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
        Ok(())
    }
}

impl Drop for Imp {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.master_fd);
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
        let shell_c = CString::new(shell).unwrap();
        let arg_c = CString::new("-c").unwrap();
        let cmd_c = CString::new(command).unwrap();

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

    // Parent: wrap master_fd into a reader
    let file = unsafe { std::fs::File::from_raw_fd(master_fd) };
    let output: Box<dyn io::Read + Send> = Box::new(file);

    Ok(super::PtyProcess {
        inner: Imp {
            pid: pid as u32,
            master_fd,
        },
        output: Some(output),
    })
}
