//! Gate TCP bridge: frame reading.

use std::io::BufReader;
use std::net::TcpStream;

/// Read one frame from the gate TCP stream.
pub fn read_hp_frame(
    hp: &mut BufReader<TcpStream>,
) -> Result<Option<dsx_proto::HpToAgent>, String> {
    match dsx_proto::read_frame(hp) {
        Ok(Some(r)) => Ok(Some(r)),
        Ok(None) => {
            log::warn!("dsx-agent: gate connection closed (EOF)");
            Err("gate connection closed unexpectedly.".into())
        }
        Err(e) => {
            log::warn!("dsx-agent: gate parse error: {e}");
            Err(format!("gate protocol error: {}", e))
        }
    }
}
