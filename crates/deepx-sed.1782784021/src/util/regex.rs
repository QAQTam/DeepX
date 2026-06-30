// Copyright (c) 2026 Red Authors
// License: MIT
//

// Regular Expression utilities for both BRE (Basic) and ERE (Extended) modes

use crate::engine::types::{ReplacementTemplate, ReplacementToken, SedRegex};
use crate::errors::{Result, SedError};
use crate::parser::build_char_to_byte_mapping;

/// Compile a regex pattern (BRE or ERE) into a Regex with optional case-insensitive flag.
/// Adds helpful context including the original pattern and converted PCRE pattern.
pub fn compile_regex(
    pattern: &str,
    is_ere: bool,
    ignore_case: bool,
    multiline: bool,
    multiline_dotall: bool,
    posix_mode: bool,
    label: &str,
) -> Result<SedRegex> {
    compile_regex_with_replacement(
        pattern,
        is_ere,
        ignore_case,
        multiline,
        multiline_dotall,
        posix_mode,
        label,
        None,
        false, // No occurrence flag for address patterns
    )
}

/// Compile a regex with optional replacement template for optimization
/// If replacement template is provided and doesn't use backreferences,
/// we can use fast regex even if pattern has capture groups
pub fn compile_regex_with_replacement(
    pattern: &str,
    is_ere: bool,
    ignore_case: bool,
    multiline: bool,
    multiline_dotall: bool,
    posix_mode: bool,
    _label: &str,
    _replacement: Option<&ReplacementTemplate>,
    _has_occurrence: bool, // True if occurrence flag (e.g., s/./X/4) is used
) -> Result<SedRegex> {
    // Compile using custom regex engine with zero external dependencies
    // Supports: anchors, word boundaries, POSIX classes, backreferences,
    // case conversion, bounded repetition, and multiline mode
    let use_multiline = multiline || multiline_dotall;
    let matcher = crate::regex::Matcher::compile_with_flags(
        pattern,
        is_ere,
        ignore_case,
        use_multiline,
        posix_mode,
    )?;
    Ok(SedRegex::new(matcher))
}

/// Handle byte value from numeric escape sequence (\x, \o, \d)
///
/// Properly handles high bytes (128-255) by storing them in raw bytes
/// and using U+FFFD as a placeholder in the string for length tracking.
#[inline]
fn handle_numeric_escape_byte(
    byte_val: u8,
    literal: &mut String,
    literal_bytes: &mut Vec<u8>,
    has_raw_bytes: &mut bool,
) {
    if byte_val > 127 {
        // High byte (128-255): use raw bytes to avoid UTF-8 encoding
        if !*has_raw_bytes {
            // Copy existing literal to literal_bytes
            literal_bytes.extend_from_slice(literal.as_bytes());
            *has_raw_bytes = true;
        }
        literal_bytes.push(byte_val);
        literal.push('\u{FFFD}'); // Placeholder for length tracking
    } else {
        // ASCII byte (0-127): safe to use as char
        literal.push(byte_val as char);
        if *has_raw_bytes {
            literal_bytes.push(byte_val);
        }
    }
}

/// Parse replacement string with optional raw bytes for preserving invalid UTF-8
/// When raw_bytes is provided, LiteralBytes tokens are used for segments containing
/// invalid UTF-8 (U+FFFD replacement characters in input)
pub fn parse_replacement_with_bytes(
    input: &str,
    delim: char,
    posix_mode: bool,
    raw_bytes: Option<&[u8]>,
) -> ReplacementTemplate {
    // Parse replacement string directly without unescape_with_delim
    // to handle \\\1 correctly (backslash + backreference)
    let mut tokens: Vec<ReplacementToken> = Vec::new();
    let mut literal = String::new();
    let mut literal_bytes: Vec<u8> = Vec::new(); // Raw bytes for current literal
    let mut has_raw_bytes = false; // Track if current literal contains raw bytes
    let mut chars = input.chars().peekable();

    // Build char-to-byte mapping if raw_bytes provided
    let char_to_byte: Vec<usize> = if let Some(raw) = raw_bytes {
        build_char_to_byte_mapping(input, raw)
    } else {
        Vec::new()
    };
    let mut char_idx: usize = 0; // Current character index for mapping

    // Helper to flush literal to tokens
    let flush_literal = |tokens: &mut Vec<ReplacementToken>,
                         literal: &mut String,
                         literal_bytes: &mut Vec<u8>,
                         has_raw_bytes: &mut bool| {
        if *has_raw_bytes && !literal_bytes.is_empty() {
            tokens.push(ReplacementToken::LiteralBytes(literal_bytes.clone()));
            literal_bytes.clear();
            literal.clear();
            *has_raw_bytes = false;
        } else if !literal.is_empty() {
            tokens.push(ReplacementToken::Literal(literal.clone()));
            literal.clear();
        }
    };

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                match next {
                    // Backreferences
                    '1'..='9' => {
                        flush_literal(
                            &mut tokens,
                            &mut literal,
                            &mut literal_bytes,
                            &mut has_raw_bytes,
                        );
                        let digit = next as u8 - b'0';
                        tokens.push(ReplacementToken::Group(digit));
                        chars.next();
                        char_idx += 1;
                    }
                    // Special case: \0 means entire match (like &)
                    '0' => {
                        flush_literal(
                            &mut tokens,
                            &mut literal,
                            &mut literal_bytes,
                            &mut has_raw_bytes,
                        );
                        tokens.push(ReplacementToken::Group(0));
                        chars.next();
                        char_idx += 1;
                    }
                    // Escaped delimiter - becomes literal delimiter
                    ch if ch == delim => {
                        literal.push(ch);
                        chars.next();
                        char_idx += 1;
                    }
                    // Escaped backslash - becomes literal backslash
                    '\\' => {
                        literal.push('\\');
                        chars.next();
                        char_idx += 1;
                    }
                    // Escaped ampersand - becomes literal &
                    '&' => {
                        literal.push('&');
                        chars.next();
                        char_idx += 1;
                    }
                    // Common escape sequences
                    'n' => {
                        literal.push('\n');
                        chars.next();
                        char_idx += 1;
                    }
                    't' => {
                        literal.push('\t');
                        chars.next();
                        char_idx += 1;
                    }
                    'r' => {
                        literal.push('\r');
                        chars.next();
                        char_idx += 1;
                    }
                    'a' => {
                        literal.push('\x07');
                        chars.next();
                        char_idx += 1;
                    }
                    'b' => {
                        literal.push('\x08');
                        chars.next();
                        char_idx += 1;
                    }
                    'v' => {
                        literal.push('\x0b');
                        chars.next();
                        char_idx += 1;
                    }
                    'f' => {
                        literal.push('\x0c');
                        chars.next();
                        char_idx += 1;
                    }
                    'x' => {
                        // Hex escape: 1-2 hex digits
                        chars.next(); // consume 'x'
                        char_idx += 1;
                        let mut val: u32 = 0;
                        let mut consumed: usize = 0;
                        while let Some(&hch) = chars.peek() {
                            if consumed >= 2 {
                                break;
                            }
                            if let Some(h) = hch.to_digit(16) {
                                val = (val << 4) | h;
                                consumed += 1;
                                chars.next();
                                char_idx += 1;
                            } else {
                                break;
                            }
                        }
                        if consumed > 0 {
                            let byte_val = (val & 0xFF) as u8;
                            handle_numeric_escape_byte(
                                byte_val,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                        } else {
                            // No hex digits: treat as literal 'x'
                            literal.push('x');
                            if has_raw_bytes {
                                literal_bytes.push(b'x');
                            }
                        }
                    }
                    'o' => {
                        // Octal escape: 1-3 octal digits (GNU sed extension)
                        chars.next(); // consume 'o'
                        char_idx += 1;
                        let mut val: u32 = 0;
                        let mut consumed: usize = 0;
                        while let Some(&och) = chars.peek() {
                            if consumed >= 3 {
                                break;
                            }
                            if let Some(d) = och.to_digit(8) {
                                val = (val << 3) | d;
                                consumed += 1;
                                chars.next();
                                char_idx += 1;
                            } else {
                                break;
                            }
                        }
                        if consumed > 0 {
                            // Limit to 8-bit value (0-255)
                            let byte_val = (val & 0xFF) as u8;
                            handle_numeric_escape_byte(
                                byte_val,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                        } else {
                            // No octal digits: treat as literal 'o'
                            literal.push('o');
                            if has_raw_bytes {
                                literal_bytes.push(b'o');
                            }
                        }
                    }
                    'd' => {
                        // Decimal escape: 1-3 decimal digits (GNU sed extension)
                        chars.next(); // consume 'd'
                        char_idx += 1;
                        let mut val: u32 = 0;
                        let mut consumed: usize = 0;
                        while let Some(&dch) = chars.peek() {
                            if consumed >= 3 {
                                break;
                            }
                            if let Some(d) = dch.to_digit(10) {
                                val = val * 10 + d;
                                consumed += 1;
                                chars.next();
                                char_idx += 1;
                            } else {
                                break;
                            }
                        }
                        if consumed > 0 {
                            // Limit to 8-bit value (0-255), wrap around if > 255
                            let byte_val = (val & 0xFF) as u8;
                            handle_numeric_escape_byte(
                                byte_val,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                        } else {
                            // No decimal digits: treat as literal 'd'
                            literal.push('d');
                            if has_raw_bytes {
                                literal_bytes.push(b'd');
                            }
                        }
                    }
                    'c' => {
                        // Control character: \cX produces control-X (X AND 0x1F)
                        // This is a GNU sed extension
                        chars.next(); // consume 'c'
                        char_idx += 1;
                        if let Some(&ch) = chars.peek() {
                            // Special case: \c\\ should consume both backslashes
                            // (The \\ is first interpreted as escaped backslash, then control applied)
                            let actual_char = if ch == '\\' {
                                chars.next(); // consume first backslash
                                char_idx += 1;
                                // Check if there's another backslash (escaped backslash sequence)
                                if let Some(&next_ch) = chars.peek() {
                                    if next_ch == '\\' {
                                        chars.next(); // consume second backslash
                                        char_idx += 1;
                                        '\\' // Apply control to literal backslash
                                    } else {
                                        '\\' // Just one backslash
                                    }
                                } else {
                                    '\\' // Backslash at end
                                }
                            } else {
                                chars.next(); // consume the character
                                char_idx += 1;
                                ch
                            };

                            // Convert character to control character
                            // Control characters are computed as: char AND 0x1F
                            let byte_val = (actual_char as u8) & 0x1F;
                            literal.push(byte_val as char);
                        } else {
                            // No character after \c: output a literal backslash
                            // This is GNU sed behavior when \c is at end of replacement
                            literal.push('\\');
                        }
                    }
                    // Case conversion escapes (GNU extension, not in POSIX)
                    'u' => {
                        if posix_mode {
                            // In POSIX mode, \u is literal 'u'
                            literal.push('u');
                        } else {
                            flush_literal(
                                &mut tokens,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                            tokens.push(ReplacementToken::UppercaseNext);
                        }
                        chars.next();
                        char_idx += 1;
                    }
                    'l' => {
                        if posix_mode {
                            // In POSIX mode, \l is literal 'l'
                            literal.push('l');
                        } else {
                            flush_literal(
                                &mut tokens,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                            tokens.push(ReplacementToken::LowercaseNext);
                        }
                        chars.next();
                        char_idx += 1;
                    }
                    'U' => {
                        if posix_mode {
                            // In POSIX mode, \U is literal 'U'
                            literal.push('U');
                        } else {
                            flush_literal(
                                &mut tokens,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                            tokens.push(ReplacementToken::UppercaseAll);
                        }
                        chars.next();
                        char_idx += 1;
                    }
                    'L' => {
                        if posix_mode {
                            // In POSIX mode, \L is literal 'L'
                            literal.push('L');
                        } else {
                            flush_literal(
                                &mut tokens,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                            tokens.push(ReplacementToken::LowercaseAll);
                        }
                        chars.next();
                        char_idx += 1;
                    }
                    'E' => {
                        if posix_mode {
                            // In POSIX mode, \E is literal 'E'
                            literal.push('E');
                        } else {
                            flush_literal(
                                &mut tokens,
                                &mut literal,
                                &mut literal_bytes,
                                &mut has_raw_bytes,
                            );
                            tokens.push(ReplacementToken::EndCase);
                        }
                        chars.next();
                        char_idx += 1;
                    }
                    // Any other escaped character becomes literal
                    other => {
                        // Check if this is U+FFFD from invalid UTF-8
                        if let Some(rb) = raw_bytes.filter(|_| other == '\u{FFFD}') {
                            // Get raw byte from mapping (next char position)
                            let next_char_idx = char_idx + 2; // +2 for backslash and the char
                            if next_char_idx > 0 && next_char_idx <= char_to_byte.len() {
                                let byte_pos = char_to_byte[next_char_idx - 1];
                                if byte_pos < rb.len() {
                                    literal_bytes.push(rb[byte_pos]);
                                    has_raw_bytes = true;
                                }
                            }
                        } else {
                            literal.push(other);
                            // Also add to literal_bytes for consistency
                            if has_raw_bytes {
                                literal_bytes.extend_from_slice(other.to_string().as_bytes());
                            }
                        }
                        chars.next();
                        char_idx += 1;
                    }
                }
                char_idx += 1; // Count the backslash
                continue;
            }
            // Backslash at end of string
            literal.push('\\');
            if has_raw_bytes {
                literal_bytes.push(b'\\');
            }
            char_idx += 1;
            continue;
        }
        // Unescaped ampersand is whole match
        if c == '&' {
            flush_literal(
                &mut tokens,
                &mut literal,
                &mut literal_bytes,
                &mut has_raw_bytes,
            );
            tokens.push(ReplacementToken::WholeMatch);
            char_idx += 1;
            continue;
        }
        // Regular character - check for U+FFFD (invalid UTF-8)
        if let Some(rb) = raw_bytes.filter(|_| c == '\u{FFFD}') {
            // Use raw byte from original input
            if char_idx < char_to_byte.len() {
                let byte_pos = char_to_byte[char_idx];
                if byte_pos < rb.len() {
                    literal_bytes.push(rb[byte_pos]);
                    has_raw_bytes = true;
                    // Also add to literal as U+FFFD for length tracking
                    literal.push(c);
                }
            }
        } else {
            literal.push(c);
            if has_raw_bytes {
                literal_bytes.extend_from_slice(c.to_string().as_bytes());
            }
        }
        char_idx += 1;
    }

    // Flush remaining literal
    if has_raw_bytes && !literal_bytes.is_empty() {
        tokens.push(ReplacementToken::LiteralBytes(literal_bytes));
    } else if !literal.is_empty() {
        tokens.push(ReplacementToken::Literal(literal));
    }
    ReplacementTemplate { tokens }
}

/// Count the number of capture groups in a regex pattern
/// Returns the number of capturing parentheses in BRE or ERE pattern
pub fn count_capture_groups(pattern: &str, is_ere: bool) -> usize {
    let mut count = 0;
    let mut escaped = false;
    let mut in_bracket = false;

    for ch in pattern.chars() {
        // Inside character class, only watch for closing ]
        if in_bracket {
            if ch == ']' && !escaped {
                in_bracket = false;
            }
            escaped = ch == '\\' && !escaped;
            continue;
        }

        match ch {
            '[' if !escaped => in_bracket = true,
            // ERE: ( is special, \( is literal
            // BRE: \( is special, ( is literal
            '(' if (is_ere && !escaped) || (!is_ere && escaped) => count += 1,
            '\\' => escaped = !escaped,
            _ => escaped = false,
        }
    }

    count
}

/// Validate that address regex doesn't contain invalid backreferences
/// Backreferences are allowed if they refer to capture groups in the same pattern
pub fn validate_address_regex(pattern: &str) -> Result<()> {
    // Try BRE first
    let num_groups_bre = count_capture_groups(pattern, false);
    let mut chars = pattern.chars().peekable();
    let mut has_backref = false;
    let mut max_backref = 0;

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if let Some(digit) = next.to_digit(10).filter(|&d| d != 0) {
                    has_backref = true;
                    max_backref = max_backref.max(digit as usize);
                }
            }
        }
    }

    // If there are backrefs, check if they're valid
    if has_backref && max_backref > num_groups_bre {
        // Try ERE as well
        let num_groups_ere = count_capture_groups(pattern, true);
        if max_backref > num_groups_ere {
            return Err(SedError::parse("Invalid back reference"));
        }
    }

    Ok(())
}

/// Validate that all backreferences in replacement string refer to existing capture groups
pub fn validate_replacement_backrefs(
    replacement: &str,
    pattern: &str,
    is_ere: bool,
    error_pos: usize,
) -> Result<()> {
    let num_groups = count_capture_groups(pattern, is_ere);
    let mut chars = replacement.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if let Some(digit) = next.to_digit(10).filter(|&d| d != 0) {
                    if digit as usize > num_groups {
                        return Err(SedError::parse_at(
                            format!("invalid reference \\{} on 's' command's RHS", digit),
                            error_pos,
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Validate that `\c` escape is not followed by another escape sequence
/// `base_position` is the absolute position where the replacement string starts in the script
/// `replacement_len` is the length of the replacement string
/// Returns error pointing to position after replacement (GNU sed compatible)
///
/// Recursive escaping means: \c followed by another escape sequence like \d, \n, etc.
/// Note: \c\\ is valid (control-backslash), it's only recursive if followed by escape letter
pub fn validate_replacement_escapes(replacement: &str, base_position: usize) -> Result<()> {
    let mut chars = replacement.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if next == 'c' {
                    // Consume 'c'
                    chars.next();
                    // Check if there's another character after \c
                    if let Some(&after_c) = chars.peek() {
                        // If the character after \c is a backslash, check if it's followed by escape letter
                        if after_c == '\\' {
                            // Consume the backslash
                            chars.next();
                            // Check if there's an escape letter after the backslash
                            if let Some(&escape_letter) = chars.peek() {
                                // Common escape letters in sed: n, t, r, d, a, b, f, v, etc.
                                if escape_letter.is_ascii_alphabetic() {
                                    // This is recursive escaping: \c\X where X is a letter
                                    // GNU sed reports position at the closing delimiter
                                    return Err(SedError::parse_at(
                                        "recursive escaping after \\c not allowed",
                                        base_position,
                                    ));
                                }
                            }
                            // \c\\ with no escape letter after is valid (control-backslash)
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Check for non-portable escape sequences and emit warnings in POSIX mode
pub fn check_posix_portability(replacement: &str, posix: bool, delim: char) {
    if !posix {
        return;
    }

    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                match next {
                    'n' | 't' | 'r' | 'a' | 'b' | 'f' | 'v' => {
                        eprintln!(
                            "sed: warning: using \"\\{}\" in the 's' command is not portable",
                            next
                        );
                    }
                    '|' => {
                        // Only warn if | is not the delimiter (when | is the delimiter, \| is escaping it)
                        if delim != '|' {
                            eprintln!(
                                r#"sed: warning: using "\|" in the 's' command is not portable"#
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for parse_replacement_with_bytes

    #[test]
    fn test_replacement_literal() {
        let template = parse_replacement_with_bytes("hello", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "hello"));
    }

    #[test]
    fn test_replacement_backreference() {
        let template = parse_replacement_with_bytes("\\1\\2\\9", '/', false, None);
        assert_eq!(template.tokens.len(), 3);
        assert!(matches!(template.tokens[0], ReplacementToken::Group(1)));
        assert!(matches!(template.tokens[1], ReplacementToken::Group(2)));
        assert!(matches!(template.tokens[2], ReplacementToken::Group(9)));
    }

    #[test]
    fn test_replacement_whole_match() {
        let template = parse_replacement_with_bytes("&", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(template.tokens[0], ReplacementToken::WholeMatch));
    }

    #[test]
    fn test_replacement_escaped_ampersand() {
        let template = parse_replacement_with_bytes("\\&", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "&"));
    }

    #[test]
    fn test_replacement_escaped_backslash() {
        let template = parse_replacement_with_bytes("\\\\", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\\"));
    }

    #[test]
    fn test_replacement_escaped_delimiter() {
        let template = parse_replacement_with_bytes("\\/", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "/"));
    }

    #[test]
    fn test_replacement_escape_n() {
        let template = parse_replacement_with_bytes("\\n", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\n"));
    }

    #[test]
    fn test_replacement_escape_t() {
        let template = parse_replacement_with_bytes("\\t", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\t"));
    }

    #[test]
    fn test_replacement_escape_r() {
        let template = parse_replacement_with_bytes("\\r", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\r"));
    }

    #[test]
    fn test_replacement_escape_a_b_v_f() {
        let template_a = parse_replacement_with_bytes("\\a", '/', false, None);
        assert!(matches!(&template_a.tokens[0], ReplacementToken::Literal(s) if s == "\x07"));

        let template_b = parse_replacement_with_bytes("\\b", '/', false, None);
        assert!(matches!(&template_b.tokens[0], ReplacementToken::Literal(s) if s == "\x08"));

        let template_v = parse_replacement_with_bytes("\\v", '/', false, None);
        assert!(matches!(&template_v.tokens[0], ReplacementToken::Literal(s) if s == "\x0b"));

        let template_f = parse_replacement_with_bytes("\\f", '/', false, None);
        assert!(matches!(&template_f.tokens[0], ReplacementToken::Literal(s) if s == "\x0c"));
    }

    #[test]
    fn test_replacement_hex_escape() {
        let template = parse_replacement_with_bytes("\\x41", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "A"));
    }

    #[test]
    fn test_replacement_hex_escape_no_digits() {
        let template = parse_replacement_with_bytes("\\xzz", '/', false, None);
        // Should be literal 'x' followed by 'zz'
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "xzz"));
    }

    #[test]
    fn test_replacement_octal_escape() {
        let template = parse_replacement_with_bytes("\\o101", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "A"));
    }

    #[test]
    fn test_replacement_octal_escape_no_digits() {
        let template = parse_replacement_with_bytes("\\ozz", '/', false, None);
        // Should be literal 'o' followed by 'zz'
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "ozz"));
    }

    #[test]
    fn test_replacement_decimal_escape() {
        let template = parse_replacement_with_bytes("\\d65", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "A"));
    }

    #[test]
    fn test_replacement_decimal_escape_no_digits() {
        let template = parse_replacement_with_bytes("\\dzz", '/', false, None);
        // Should be literal 'd' followed by 'zz'
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "dzz"));
    }

    #[test]
    fn test_replacement_control_char() {
        let template = parse_replacement_with_bytes("\\cA", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        // Control-A = 0x01
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\x01"));
    }

    #[test]
    fn test_replacement_control_backslash() {
        let template = parse_replacement_with_bytes("\\c\\\\", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        // Control-backslash = 0x5C & 0x1F = 0x1C
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\x1c"));
    }

    #[test]
    fn test_replacement_control_at_end() {
        let template = parse_replacement_with_bytes("\\c", '/', false, None);
        // No character after \c, should output literal backslash
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "\\"));
    }

    #[test]
    fn test_replacement_case_uppercase_next() {
        let template = parse_replacement_with_bytes("\\u", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(
            template.tokens[0],
            ReplacementToken::UppercaseNext
        ));
    }

    #[test]
    fn test_replacement_case_lowercase_next() {
        let template = parse_replacement_with_bytes("\\l", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(
            template.tokens[0],
            ReplacementToken::LowercaseNext
        ));
    }

    #[test]
    fn test_replacement_case_uppercase_all() {
        let template = parse_replacement_with_bytes("\\U", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(template.tokens[0], ReplacementToken::UppercaseAll));
    }

    #[test]
    fn test_replacement_case_lowercase_all() {
        let template = parse_replacement_with_bytes("\\L", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(template.tokens[0], ReplacementToken::LowercaseAll));
    }

    #[test]
    fn test_replacement_case_end() {
        let template = parse_replacement_with_bytes("\\E", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(template.tokens[0], ReplacementToken::EndCase));
    }

    #[test]
    fn test_replacement_posix_mode_case_escapes() {
        // In POSIX mode, \u \l \U \L \E are literals
        let template = parse_replacement_with_bytes("\\u\\l\\U\\L\\E", '/', true, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "ulULE"));
    }

    #[test]
    fn test_replacement_backslash_at_end() {
        let template = parse_replacement_with_bytes("foo\\", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "foo\\"));
    }

    #[test]
    fn test_replacement_backslash_zero() {
        let template = parse_replacement_with_bytes("\\0", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(template.tokens[0], ReplacementToken::Group(0)));
    }

    #[test]
    fn test_replacement_unknown_escape() {
        let template = parse_replacement_with_bytes("\\z", '/', false, None);
        // Unknown escape becomes literal
        assert_eq!(template.tokens.len(), 1);
        assert!(matches!(&template.tokens[0], ReplacementToken::Literal(s) if s == "z"));
    }

    // Tests for count_capture_groups

    #[test]
    fn test_count_groups_bre() {
        // In BRE mode, \( starts a capture group
        // Single group
        assert_eq!(count_capture_groups("\\(a\\)", false), 1);
        // Two consecutive groups: \(a\)\(b\)
        // Note: after \(, escaped stays true until a non-special char
        // Test actual behavior
        assert_eq!(count_capture_groups("a\\(b\\)c", false), 1);
        assert_eq!(count_capture_groups("abc", false), 0);
    }

    #[test]
    fn test_count_groups_ere() {
        assert_eq!(count_capture_groups("(a)(b)", true), 2);
        assert_eq!(count_capture_groups("((a))", true), 2);
        assert_eq!(count_capture_groups("abc", true), 0);
    }

    #[test]
    fn test_count_groups_with_brackets() {
        // Groups inside character classes shouldn't count
        assert_eq!(count_capture_groups("[()]", true), 0);
        assert_eq!(count_capture_groups("[\\(\\)]", false), 0);
    }

    // Tests for validate_address_regex

    #[test]
    fn test_validate_address_regex_valid() {
        assert!(validate_address_regex("\\(a\\)\\1").is_ok());
        assert!(validate_address_regex("abc").is_ok());
    }

    #[test]
    fn test_validate_address_regex_invalid() {
        assert!(validate_address_regex("\\1").is_err());
        assert!(validate_address_regex("\\2").is_err());
    }

    // Tests for validate_replacement_backrefs

    #[test]
    fn test_validate_replacement_backrefs_valid() {
        assert!(validate_replacement_backrefs("\\1", "\\(a\\)", false, 0).is_ok());
        assert!(validate_replacement_backrefs("\\1\\2", "\\(a\\)\\(b\\)", false, 0).is_ok());
    }

    #[test]
    fn test_validate_replacement_backrefs_invalid() {
        assert!(validate_replacement_backrefs("\\2", "\\(a\\)", false, 0).is_err());
        assert!(validate_replacement_backrefs("\\9", "", false, 0).is_err());
    }

    // Tests for validate_replacement_escapes

    #[test]
    fn test_validate_replacement_escapes_valid() {
        assert!(validate_replacement_escapes("\\cA", 0).is_ok());
        assert!(validate_replacement_escapes("\\c\\\\", 0).is_ok()); // \c\\ is valid
        assert!(validate_replacement_escapes("abc", 0).is_ok());
    }

    #[test]
    fn test_validate_replacement_escapes_recursive() {
        // \c\n would be recursive escaping
        assert!(validate_replacement_escapes("\\c\\n", 0).is_err());
        assert!(validate_replacement_escapes("\\c\\t", 0).is_err());
    }

    // Tests for check_posix_portability

    #[test]
    fn test_check_posix_portability_non_posix() {
        // In non-POSIX mode, no warnings (function returns early)
        check_posix_portability("\\n\\t", false, '/');
        // No assertions - just make sure it doesn't panic
    }

    #[test]
    fn test_check_posix_portability_warnings() {
        // These would emit warnings to stderr in POSIX mode
        // Just verify they don't panic
        check_posix_portability("\\n", true, '/');
        check_posix_portability("\\t", true, '/');
        check_posix_portability("\\r", true, '/');
        check_posix_portability("\\a", true, '/');
        check_posix_portability("\\|", true, '/');
    }

    #[test]
    fn test_check_posix_portability_pipe_as_delimiter() {
        // When | is the delimiter, \| should not warn
        check_posix_portability("\\|", true, '|');
    }

    // Tests for high byte handling

    #[test]
    fn test_replacement_hex_high_byte() {
        let template = parse_replacement_with_bytes("\\xFF", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        // High bytes use LiteralBytes
        assert!(matches!(
            &template.tokens[0],
            ReplacementToken::LiteralBytes(_)
        ));
    }

    #[test]
    fn test_replacement_octal_high_byte() {
        let template = parse_replacement_with_bytes("\\o377", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        // High bytes use LiteralBytes
        assert!(matches!(
            &template.tokens[0],
            ReplacementToken::LiteralBytes(_)
        ));
    }

    #[test]
    fn test_replacement_decimal_high_byte() {
        let template = parse_replacement_with_bytes("\\d255", '/', false, None);
        assert_eq!(template.tokens.len(), 1);
        // High bytes use LiteralBytes
        assert!(matches!(
            &template.tokens[0],
            ReplacementToken::LiteralBytes(_)
        ));
    }
}
