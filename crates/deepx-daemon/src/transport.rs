//! Frame-based socket transport: 4-byte LE length prefix + JSON payload.
//!
//! Unix: Unix domain sockets. Windows: stub (falls back to direct spawn).

use std::io::{self, Read, Write};

use deepx_proto::{FrontendToDaemon, DaemonToFrontend};

/// Read one DaemonToFrontend frame from a stream. Returns None on clean EOF.
pub fn read_frame(stream: &mut impl Read) -> io::Result<Option<DaemonToFrontend>> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf) {
        Ok(()) => {},
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    let frame: DaemonToFrontend = serde_json::from_slice(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(frame))
}

/// Write one FrontendToDaemon frame to a stream.
pub fn write_frame(stream: &mut impl Write, frame: &FrontendToDaemon) -> io::Result<()> {
    let payload = serde_json::to_vec(frame)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()
}

// ── Unix domain socket ──

#[cfg(unix)]
pub mod unix {
    use std::io;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    pub fn bind(path: &Path) -> io::Result<UnixListener> {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        UnixListener::bind(path)
    }

    pub fn accept(listener: &UnixListener) -> io::Result<UnixStream> {
        let (stream, _addr) = listener.accept()?;
        stream.set_nonblocking(false)?;
        Ok(stream)
    }

    pub fn connect(path: &Path) -> io::Result<UnixStream> {
        UnixStream::connect(path)
    }
}

// ── Windows named pipe (TODO: needs `windows` crate) ──
// windows-sys does not expose CreateFileW / CreateNamedPipeW / PIPE_ACCESS_DUPLEX.
// The full `windows` crate is required. Defer until: interprocess crate or raw FFI.

#[cfg(windows)]
pub mod win {
    use std::io;
    use std::path::Path;

    /// TODO: needs `windows` crate for CreateFileW / CreateNamedPipeW.
    /// windows-sys does not expose these. Evaluate interprocess crate or raw FFI.
    pub fn bind(_path: &Path) -> io::Result<std::fs::File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows named pipe: needs `windows` crate (not windows-sys). Use direct child process fallback.",
        ))
    }

    pub fn accept(_pipe: std::fs::File) -> io::Result<std::fs::File> {
        Ok(_pipe) // unreachable
    }

    pub fn connect(_path: &Path) -> io::Result<std::fs::File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows named pipe: needs `windows` crate",
        ))
    }
}
