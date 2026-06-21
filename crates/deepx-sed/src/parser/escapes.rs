// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Escape sequence handling for sed text commands (y, a, i, c)
//!
//! This module provides functions to decode escape sequences in text contexts.
//!
//! ## Escape semantics in GNU sed
//!
//! Different sed contexts support different escape sequences:
//!
//! ### Text commands (y, a, i, c) - handled by this module
//! - `\n`, `\t`, `\r`, `\f`, `\v`, `\a`, `\b` - standard escapes
//! - `\\` - literal backslash
//! - `\cX` - control character (X ^ 0x40)
//! - `\NNN` - NOT supported (treated as literal characters)
//!
//! ### s/// replacement - handled by util/regex.rs
//! - All standard escapes plus:
//! - `\oNNN` - octal escape (GNU extension)
//! - `\xNN` - hex escape (GNU extension)
//! - `\dNNN` - decimal escape (GNU extension)
//! - `\1`-`\9` - backreferences
//! - `\u`, `\l`, `\U`, `\L`, `\E` - case conversion
//!
//! ### Regex patterns - handled by regex/parser.rs
//! - BRE/ERE specific escape sequences

/// Decode standard escape sequences in a string.
///
/// Supports:
/// - `\n`, `\t`, `\r`, `\f`, `\v`, `\a`, `\b` - standard escapes
/// - `\\` - literal backslash
/// - `\cX` - control character (X ^ 0x40)
/// - `\x` - any other escaped char becomes itself
///
/// Does NOT support bare octal escapes (`\NNN`) - this matches GNU sed
/// behavior for the `y` command and text insertion commands (a, i, c).
pub fn decode_standard_escapes(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.peek().cloned() {
            Some('n') => {
                out.push('\n');
                it.next();
            }
            Some('t') => {
                out.push('\t');
                it.next();
            }
            Some('r') => {
                out.push('\r');
                it.next();
            }
            Some('f') => {
                out.push('\x0c');
                it.next();
            }
            Some('v') => {
                out.push('\x0b');
                it.next();
            }
            Some('a') => {
                out.push('\x07');
                it.next();
            }
            Some('b') => {
                out.push('\x08');
                it.next();
            }
            Some('\\') => {
                out.push('\\');
                it.next();
            }
            Some('c') => {
                // Control character: \cX produces X ^ 0x40
                // For lowercase letters, convert to uppercase first
                it.next(); // consume 'c'
                if let Some(ch) = it.next() {
                    // Handle \c\\ case: the next char after \c might be a backslash
                    // In that case, consume the escaped char
                    let actual_char = if ch == '\\' {
                        if let Some(escaped) = it.next() {
                            escaped
                        } else {
                            '\\'
                        }
                    } else {
                        ch
                    };
                    // Convert lowercase to uppercase before XOR
                    let effective_byte = if actual_char.is_ascii_lowercase() {
                        (actual_char as u8) - 32
                    } else {
                        actual_char as u8
                    };
                    let ctrl = (effective_byte ^ 0x40) as char;
                    out.push(ctrl);
                }
                // If no char after \c, just skip (GNU sed behavior)
            }
            Some(other) => {
                // Any other escaped char becomes itself (including digits)
                // This matches GNU sed: \1 in y command is literal '1', not backreference
                out.push(other);
                it.next();
            }
            None => {
                out.push('\\');
            }
        }
    }
    out
}

/// Decode standard escape sequences directly to bytes (preserves invalid UTF-8).
///
/// Similar to `decode_standard_escapes` but operates on raw bytes and preserves
/// byte values that would be invalid UTF-8.
pub fn decode_standard_escapes_to_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => {
                    out.push(b'\n');
                    i += 2;
                }
                b't' => {
                    out.push(b'\t');
                    i += 2;
                }
                b'r' => {
                    out.push(b'\r');
                    i += 2;
                }
                b'f' => {
                    out.push(0x0c);
                    i += 2;
                }
                b'v' => {
                    out.push(0x0b);
                    i += 2;
                }
                b'a' => {
                    out.push(0x07);
                    i += 2;
                }
                b'b' => {
                    out.push(0x08);
                    i += 2;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 2;
                }
                b'c' => {
                    // Control character: \cX produces X ^ 0x40
                    // For lowercase letters, convert to uppercase first
                    i += 2; // skip \c
                    if i < bytes.len() {
                        // Handle \c\\ case
                        let actual_byte = if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            i += 1; // skip first backslash
                            let b = bytes[i];
                            i += 1;
                            b
                        } else {
                            let b = bytes[i];
                            i += 1;
                            b
                        };
                        // Convert lowercase to uppercase before XOR
                        let effective_byte = if actual_byte >= b'a' && actual_byte <= b'z' {
                            actual_byte - 32
                        } else {
                            actual_byte
                        };
                        out.push(effective_byte ^ 0x40);
                    }
                    // If no char after \c, just skip
                }
                other => {
                    // Any other escaped char becomes itself (including digits)
                    out.push(other);
                    i += 2;
                }
            }
        } else if bytes[i] == b'\\' {
            out.push(b'\\');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_escapes_basic() {
        assert_eq!(decode_standard_escapes("hello"), "hello");
        assert_eq!(decode_standard_escapes("hello\\nworld"), "hello\nworld");
        assert_eq!(
            decode_standard_escapes("\\t\\r\\f\\v\\a\\b"),
            "\t\r\x0c\x0b\x07\x08"
        );
        assert_eq!(decode_standard_escapes("\\\\"), "\\");
    }

    #[test]
    fn test_standard_escapes_control() {
        // \cA = 'A' ^ 0x40 = 0x01
        assert_eq!(decode_standard_escapes("\\cA"), "\x01");
        // \ca = 'a' -> 'A' -> 0x01
        assert_eq!(decode_standard_escapes("\\ca"), "\x01");
        // \c\\ case
        assert_eq!(decode_standard_escapes("\\c\\\\"), "\x1c");
    }

    #[test]
    fn test_standard_escapes_digits_are_literal() {
        // GNU sed does NOT support bare octal escapes in y command
        // \1 becomes literal '1', \101 becomes literal "101"
        assert_eq!(decode_standard_escapes("\\1"), "1");
        assert_eq!(decode_standard_escapes("\\101"), "101");
        assert_eq!(decode_standard_escapes("\\141\\142"), "141142");
    }

    #[test]
    fn test_standard_escapes_to_bytes_basic() {
        assert_eq!(decode_standard_escapes_to_bytes(b"hello"), b"hello");
        assert_eq!(
            decode_standard_escapes_to_bytes(b"hello\\nworld"),
            b"hello\nworld"
        );
    }

    #[test]
    fn test_standard_escapes_to_bytes_digits_literal() {
        // Digits are literal, not octal
        assert_eq!(decode_standard_escapes_to_bytes(b"\\1"), b"1");
        assert_eq!(decode_standard_escapes_to_bytes(b"\\377"), b"377");
    }
}
