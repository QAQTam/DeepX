// Copyright (c) 2026 Red Authors
// License: MIT
//

//! I/O utilities for sed operations
//!
//! This module provides file reading, encoding detection, and backup
//! suffix expansion functionality separated from the main orchestration logic.

mod encoding;
mod inplace;
mod lines;

// detect_encoding is used internally by lines module, not re-exported
pub use inplace::expand_backup_suffix;
pub use lines::{read_all_lines, split_file_content};

/// Helper function to conditionally flush output based on unbuffered flag
#[inline]
pub fn flush_output(out: &mut dyn std::io::Write, unbuffered: bool) {
    if unbuffered {
        let _ = out.flush();
    }
}

/// Get the line ending bytes for the current platform and mode.
/// On Windows without binary mode, returns CRLF. Otherwise returns LF.
#[inline]
pub fn line_ending(binary: bool) -> &'static [u8] {
    #[cfg(windows)]
    {
        if binary {
            b"\n"
        } else {
            b"\r\n"
        }
    }
    #[cfg(not(windows))]
    {
        let _ = binary; // suppress unused warning
        b"\n"
    }
}

/// Write a line to output with optional raw bytes and separator handling.
///
/// This unifies the output logic used in both `execute_over_lines` and
/// `process_stdin_line_by_line`, reducing code duplication.
#[inline]
pub fn write_output_line(
    content: &str,
    raw_bytes: Option<&[u8]>,
    null_data: bool,
    write_separator: bool,
    binary: bool,
    out: &mut dyn std::io::Write,
) {
    let output_bytes = raw_bytes.unwrap_or_else(|| content.as_bytes());
    let _ = out.write_all(output_bytes);
    if write_separator {
        if null_data {
            let _ = out.write_all(b"\0");
        } else {
            let _ = out.write_all(line_ending(binary));
        }
    }
}
