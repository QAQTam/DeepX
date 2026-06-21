// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Line reading utilities for files and stdin

use std::io::{self, Read};

use encoding_rs::Encoding;

use crate::errors::{Result, SedError};
use crate::util::symlink::resolve_symlink_chain;

use super::encoding::detect_encoding;

/// Read all lines from files or stdin
///
/// Returns a tuple of:
/// - Vec<String>: Lines as strings (decoded with detected encoding)
/// - Vec<Vec<u8>>: Lines as raw bytes
/// - Vec<String>: Filename for each line (for F command)
/// - &'static Encoding: Detected encoding
/// - bool: Whether the input ends with a separator
pub fn read_all_lines(
    files: &[String],
    null_data: bool,
    follow_symlinks: bool,
    binary: bool,
) -> Result<(
    Vec<String>,
    Vec<Vec<u8>>,
    Vec<String>,
    &'static Encoding,
    bool,
)> {
    let mut all_lines_str = Vec::new();
    let mut all_lines_bytes = Vec::new();
    let mut all_filenames = Vec::new(); // Track which file each line came from
    let mut detected_encoding: &'static Encoding = encoding_rs::UTF_8;
    let mut first_file = true;
    let mut ends_with_separator = false; // Track whether the last input ends with separator

    let separator = if null_data { b'\0' } else { b'\n' };

    // On Windows in text mode (not binary), strip trailing \r from lines
    #[cfg(windows)]
    let strip_cr = !binary && !null_data;
    #[cfg(not(windows))]
    let strip_cr = {
        let _ = binary;
        false
    };

    if files.is_empty() || (files.len() == 1 && files[0] == "-") {
        let stdin = io::stdin();
        let mut buf: Vec<u8> = Vec::new();
        stdin.lock().read_to_end(&mut buf)?;

        // Detect encoding from stdin
        detected_encoding = detect_encoding(&buf);

        // Track whether stdin ends with separator
        ends_with_separator = !buf.is_empty() && buf[buf.len() - 1] == separator;

        let lines_b = split_bytes_into_lines_bytes(buf.clone(), separator, strip_cr);
        let lines_s =
            split_bytes_into_lines_with_encoding(&buf, separator, detected_encoding, strip_cr);
        let line_count = lines_s.len();
        all_lines_bytes.extend(lines_b);
        all_lines_str.extend(lines_s);
        all_filenames.extend(vec!["-".to_string(); line_count]);
    } else {
        for path in files {
            // Resolve symlinks if follow_symlinks is enabled
            let filename_for_f_command = if follow_symlinks {
                // Use non-strict mode (silent on errors) for F command
                resolve_symlink_chain(std::path::Path::new(path), false)?
                    .display()
                    .to_string()
            } else {
                // Not following symlinks - use original path
                path.clone()
            };

            // When follow_symlinks is enabled, try readlink first to give proper error
            if follow_symlinks {
                // Try to read link or stat the file to ensure it exists and is accessible
                if let Err(e) = std::fs::symlink_metadata(path) {
                    return Err(SedError::io("couldn't readlink", path, e));
                }
            }

            let mut file =
                std::fs::File::open(path).map_err(|e| SedError::io("can't read", path, e))?;
            let mut buf: Vec<u8> = Vec::new();
            file.read_to_end(&mut buf)?;

            // Detect encoding from first file
            if first_file {
                detected_encoding = detect_encoding(&buf);
                first_file = false;
            }

            // Track whether this file (the last one processed) ends with separator
            ends_with_separator = !buf.is_empty() && buf[buf.len() - 1] == separator;

            let lines_b = split_bytes_into_lines_bytes(buf.clone(), separator, strip_cr);
            let lines_s =
                split_bytes_into_lines_with_encoding(&buf, separator, detected_encoding, strip_cr);
            let line_count = lines_s.len();
            all_lines_bytes.extend(lines_b);
            all_lines_str.extend(lines_s);
            all_filenames.extend(vec![filename_for_f_command; line_count]);
        }
    }

    Ok((
        all_lines_str,
        all_lines_bytes,
        all_filenames,
        detected_encoding,
        ends_with_separator,
    ))
}

/// Split file content into lines (both string and byte representations)
pub fn split_file_content(
    content: Vec<u8>,
    null_data: bool,
    binary: bool,
) -> (Vec<String>, Vec<Vec<u8>>, &'static Encoding, bool) {
    let mut lines_str = Vec::new();
    let mut lines_bytes = Vec::new();
    let mut start: usize = 0;
    let separator = if null_data { b'\0' } else { b'\n' };

    // Detect encoding
    let encoding = detect_encoding(&content);

    // Track if content ends with separator
    let ends_with_separator = !content.is_empty() && content[content.len() - 1] == separator;

    // On Windows in text mode (not binary), strip trailing \r from lines
    #[cfg(windows)]
    let strip_cr = !binary && !null_data;
    #[cfg(not(windows))]
    let strip_cr = {
        let _ = binary;
        false
    };

    for i in 0..content.len() {
        if content[i] == separator {
            let slice = strip_trailing_cr(&content[start..i], strip_cr);
            let (decoded, _, _) = encoding.decode(slice);
            lines_str.push(decoded.into_owned());
            lines_bytes.push(slice.to_vec());
            start = i + 1;
        }
    }

    if start < content.len() {
        let slice = strip_trailing_cr(&content[start..], strip_cr);
        let (decoded, _, _) = encoding.decode(slice);
        lines_str.push(decoded.into_owned());
        lines_bytes.push(slice.to_vec());
    }

    (lines_str, lines_bytes, encoding, ends_with_separator)
}

/// Strip trailing \r from a byte slice (for Windows text mode)
#[inline]
fn strip_trailing_cr(slice: &[u8], strip: bool) -> &[u8] {
    if strip && !slice.is_empty() && slice[slice.len() - 1] == b'\r' {
        &slice[..slice.len() - 1]
    } else {
        slice
    }
}

/// Split bytes into lines, preserving raw bytes
/// When strip_cr is true, removes trailing \r before \n (Windows text mode)
fn split_bytes_into_lines_bytes(bytes: Vec<u8>, separator: u8, strip_cr: bool) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut start: usize = 0;
    for i in 0..bytes.len() {
        if bytes[i] == separator {
            let slice = strip_trailing_cr(&bytes[start..i], strip_cr);
            lines.push(slice.to_vec());
            start = i + 1;
        }
    }
    if start < bytes.len() {
        let slice = strip_trailing_cr(&bytes[start..], strip_cr);
        lines.push(slice.to_vec());
    }
    lines
}

/// Split bytes into lines with encoding conversion
/// When strip_cr is true, removes trailing \r before \n (Windows text mode)
fn split_bytes_into_lines_with_encoding(
    bytes: &[u8],
    separator: u8,
    encoding: &'static Encoding,
    strip_cr: bool,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut start: usize = 0;
    for i in 0..bytes.len() {
        if bytes[i] == separator {
            let slice = strip_trailing_cr(&bytes[start..i], strip_cr);
            let (decoded, _, _) = encoding.decode(slice);
            lines.push(decoded.into_owned());
            start = i + 1;
        }
    }
    if start < bytes.len() {
        let slice = strip_trailing_cr(&bytes[start..], strip_cr);
        let (decoded, _, _) = encoding.decode(slice);
        lines.push(decoded.into_owned());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_file_content_newline() {
        let content = b"line1\nline2\nline3".to_vec();
        let (lines, bytes, _, ends) = split_file_content(content, false, false);
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
        assert_eq!(bytes.len(), 3);
        assert!(!ends);
    }

    #[test]
    fn test_split_file_content_trailing_newline() {
        let content = b"line1\nline2\n".to_vec();
        let (lines, _, _, ends) = split_file_content(content, false, false);
        assert_eq!(lines, vec!["line1", "line2"]);
        assert!(ends);
    }

    #[test]
    fn test_split_file_content_null_data() {
        let content = b"line1\0line2\0".to_vec();
        let (lines, _, _, ends) = split_file_content(content, true, false);
        assert_eq!(lines, vec!["line1", "line2"]);
        assert!(ends);
    }
}
