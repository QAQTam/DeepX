//! Frame-based TCP transport: 4-byte LE length prefix + JSON payload.
//!
//! Binds on 127.0.0.1:0 (OS-assigned random port), writes port to a `.port` file.
//! Cross-platform — no Unix sockets or Windows named pipes needed.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};

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

// ── TCP loopback transport ──

/// Bind a TCP listener on 127.0.0.1:0 (OS picks a free port).
/// Returns the listener and the assigned port number.
pub fn bind() -> io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Accept a single connection. Sets blocking mode.
pub fn accept(listener: &TcpListener) -> io::Result<TcpStream> {
    let (stream, _addr) = listener.accept()?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}

/// Connect to the daemon at 127.0.0.1 on the given port.
pub fn connect(port: u16) -> io::Result<TcpStream> {
    TcpStream::connect(("127.0.0.1", port))
}
