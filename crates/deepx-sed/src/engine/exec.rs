// Copyright (c) 2026 Red Authors
// License: MIT
//

use std::borrow::Cow;
use std::collections::HashMap;
use std::process::Command as ProcessCommand;

use crate::errors::{Result, SedError};
use std::io::Write as IoWrite;

use super::addr::{AddressEvaluator, ExecutionContext};
use super::types::{Command, CommandResult, ReplacementTemplate, ReplacementToken, SedRegex};
use crate::parser::{
    build_char_to_byte_mapping, is_utf8_locale as parser_is_utf8_locale, Address, PrintTiming,
};

/// Trait for unified rendering to String or Vec<u8>
trait RenderSink {
    fn write_str(&mut self, s: &str);
    fn write_bytes(&mut self, b: &[u8]);
}

impl RenderSink for String {
    fn write_str(&mut self, s: &str) {
        self.push_str(s);
    }
    fn write_bytes(&mut self, b: &[u8]) {
        self.push_str(&String::from_utf8_lossy(b));
    }
}

impl RenderSink for Vec<u8> {
    fn write_str(&mut self, s: &str) {
        self.extend_from_slice(s.as_bytes());
    }
    fn write_bytes(&mut self, b: &[u8]) {
        self.extend_from_slice(b);
    }
}

/// Render replacement template to any RenderSink (String or Vec<u8>)
///
/// Supports WholeMatch (&), Group backreferences (\1-\9), literals, and case transforms.
/// When captures is None, Group tokens produce empty strings.
fn render_replacement_to<S: RenderSink>(
    whole_match: &str,
    captures: Option<&HashMap<usize, crate::regex::Capture>>,
    template: &ReplacementTemplate,
    sink: &mut S,
) {
    let mut uppercase_next = false;
    let mut lowercase_next = false;
    let mut uppercase_all = false;
    let mut lowercase_all = false;

    for token in &template.tokens {
        match token {
            ReplacementToken::Literal(s) => {
                let transformed = apply_case_transform(
                    s,
                    &mut uppercase_next,
                    &mut lowercase_next,
                    uppercase_all,
                    lowercase_all,
                );
                sink.write_str(&transformed);
            }
            ReplacementToken::LiteralBytes(bytes) => {
                // Apply case transformation to ASCII bytes within LiteralBytes
                let transformed = apply_case_transform_bytes(
                    bytes,
                    &mut uppercase_next,
                    &mut lowercase_next,
                    uppercase_all,
                    lowercase_all,
                );
                sink.write_bytes(&transformed);
            }
            ReplacementToken::WholeMatch => {
                let transformed = apply_case_transform(
                    whole_match,
                    &mut uppercase_next,
                    &mut lowercase_next,
                    uppercase_all,
                    lowercase_all,
                );
                sink.write_str(&transformed);
            }
            ReplacementToken::Group(group_id) => {
                if *group_id == 0 {
                    let transformed = apply_case_transform(
                        whole_match,
                        &mut uppercase_next,
                        &mut lowercase_next,
                        uppercase_all,
                        lowercase_all,
                    );
                    sink.write_str(&transformed);
                } else if let Some(caps) = captures {
                    if let Some(cap) = caps.get(&(*group_id as usize)) {
                        // For case transforms, we need proper Unicode handling
                        // Use raw_bytes only in non-UTF-8 MBCS locales (Shift-JIS, EUC-JP)
                        // In UTF-8 locales, use cap.text for proper Unicode case conversion
                        let use_raw_bytes = cap.raw_bytes.is_some() && !parser_is_utf8_locale();

                        if use_raw_bytes {
                            // Non-UTF-8 MBCS mode - use raw bytes, ASCII-only transforms
                            let transformed = apply_case_transform_bytes(
                                cap.raw_bytes.as_ref().unwrap(),
                                &mut uppercase_next,
                                &mut lowercase_next,
                                uppercase_all,
                                lowercase_all,
                            );
                            sink.write_bytes(&transformed);
                        } else {
                            // UTF-8 or non-MBCS mode - use text for proper Unicode transforms
                            let transformed = apply_case_transform(
                                &cap.text,
                                &mut uppercase_next,
                                &mut lowercase_next,
                                uppercase_all,
                                lowercase_all,
                            );
                            sink.write_str(&transformed);
                        }
                    }
                }
            }
            ReplacementToken::UppercaseNext => uppercase_next = true,
            ReplacementToken::LowercaseNext => lowercase_next = true,
            ReplacementToken::UppercaseAll => uppercase_all = true,
            ReplacementToken::LowercaseAll => lowercase_all = true,
            ReplacementToken::EndCase => {
                uppercase_all = false;
                lowercase_all = false;
            }
        }
    }
}

/// Render replacement template to String
fn render_replacement(
    whole_match: &str,
    captures: Option<&HashMap<usize, crate::regex::Capture>>,
    template: &ReplacementTemplate,
) -> String {
    let mut result = String::new();
    render_replacement_to(whole_match, captures, template, &mut result);
    result
}

/// Render replacement template to bytes (for preserving invalid UTF-8)
fn render_replacement_to_bytes(
    whole_match: &str,
    captures: Option<&HashMap<usize, crate::regex::Capture>>,
    template: &ReplacementTemplate,
) -> Vec<u8> {
    let mut result = Vec::new();
    render_replacement_to(whole_match, captures, template, &mut result);
    result
}

/// Apply case transformation to a string based on current state
fn apply_case_transform(
    text: &str,
    uppercase_next: &mut bool,
    lowercase_next: &mut bool,
    uppercase_all: bool,
    lowercase_all: bool,
) -> String {
    let mut result = String::new();
    let mut chars = text.chars();

    if *uppercase_next || *lowercase_next {
        // Transform only first character
        if let Some(first) = chars.next() {
            if *uppercase_next {
                result.push_str(&first.to_uppercase().to_string());
            } else {
                result.push_str(&first.to_lowercase().to_string());
            }
            *uppercase_next = false;
            *lowercase_next = false;
        }
        // Rest of text unchanged
        result.push_str(&chars.collect::<String>());
    } else if uppercase_all {
        result = text.to_uppercase();
    } else if lowercase_all {
        result = text.to_lowercase();
    } else {
        result = text.to_string();
    }

    result
}

/// Apply case transformation to raw bytes (for LiteralBytes tokens)
/// Only transforms ASCII letters, preserves high bytes (128-255) unchanged
fn apply_case_transform_bytes(
    bytes: &[u8],
    uppercase_next: &mut bool,
    lowercase_next: &mut bool,
    uppercase_all: bool,
    lowercase_all: bool,
) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());

    for (i, &byte) in bytes.iter().enumerate() {
        if i == 0 && (*uppercase_next || *lowercase_next) {
            // Transform only first ASCII letter
            if byte.is_ascii_lowercase() && *uppercase_next {
                result.push(byte.to_ascii_uppercase());
            } else if byte.is_ascii_uppercase() && *lowercase_next {
                result.push(byte.to_ascii_lowercase());
            } else {
                result.push(byte);
            }
            *uppercase_next = false;
            *lowercase_next = false;
        } else if uppercase_all && byte.is_ascii_lowercase() {
            result.push(byte.to_ascii_uppercase());
        } else if lowercase_all && byte.is_ascii_uppercase() {
            result.push(byte.to_ascii_lowercase());
        } else {
            result.push(byte);
        }
    }

    result
}

/// Helper for advanced replacement with template (supports backreferences)
/// Performs sed-style substitution. Returns (result, match_occurred, optional_bytes).
/// `match_occurred` is true if any match was found (even if replacement produces same string).
/// When `raw_bytes` is provided and template contains LiteralBytes tokens, also returns
/// the byte-level result for preserving invalid UTF-8 in pattern space.
fn sed_replace_with_template<'t>(
    regex: &SedRegex,
    text: &'t str,
    template: &ReplacementTemplate,
    global: bool,
    occurrence: Option<usize>,
    raw_bytes: Option<&[u8]>,
) -> (Cow<'t, str>, bool, Option<Vec<u8>>) {
    let matcher = &regex.matcher;

    // Check if replacement uses backreferences to determine which finder to use
    let uses_backrefs = template
        .tokens
        .iter()
        .any(|token| matches!(token, ReplacementToken::Group(_)));

    // Bind raw_bytes and char_to_byte_map together to avoid unwrap() calls
    // When byte_ctx is Some, we have both raw bytes and the mapping
    // Always track raw bytes if available - any substitution should update them
    let byte_ctx: Option<(&[u8], Vec<usize>)> =
        raw_bytes.map(|raw| (raw, build_char_to_byte_mapping(text, raw)));

    let mut result = String::new();
    let mut byte_result: Vec<u8> = Vec::new();
    let mut last_end = 0;
    let mut last_byte_end: usize = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut match_count = 0;
    // Track where the previous match ended - used to skip zero-length matches at that position
    // This prevents patterns like [a-z]* from matching both the letters AND an empty string after
    let mut prev_match_end_pos: Option<usize> = None;

    loop {
        // Safety: prevent infinite loops on pathological patterns
        if match_count > chars.len() * 10 {
            break;
        }

        // Find next match - use appropriate finder based on whether we need captures
        let match_data: Option<(
            usize,
            usize,
            String,
            Option<HashMap<usize, crate::regex::Capture>>,
        )> = if uses_backrefs {
            matcher
                .find_with_captures_from(text, last_end)
                .map(|(start, end, captures)| {
                    let matched_text: String = chars[start..end].iter().collect();
                    (start, end, matched_text, Some(captures))
                })
        } else {
            matcher
                .find_with_text_from(text, last_end)
                .map(|(matched_text, start, end)| (start, end, matched_text.to_string(), None))
        };

        let Some((start, end, matched_text, captures)) = match_data else {
            break;
        };

        // Skip zero-length match at the position where previous match ended
        // This prevents patterns like [a-z]* from matching both "abc" AND then "" at position 3
        // The rule: after any match ending at position P, skip the next zero-length match at P
        if start == end && prev_match_end_pos == Some(start) {
            // Don't replace, just advance past this character
            if end < chars.len() {
                result.push(chars[end]);
                if let Some((raw, map)) = &byte_ctx {
                    let byte_pos = if end < map.len() { map[end] } else { raw.len() };
                    let byte_next = if end + 1 < map.len() {
                        map[end + 1]
                    } else {
                        raw.len()
                    };
                    byte_result.extend_from_slice(&raw[byte_pos..byte_next]);
                    last_byte_end = byte_next;
                }
            }
            last_end = end + 1;
            prev_match_end_pos = None; // Reset so we can match at next position
            continue;
        }

        match_count += 1;

        // Skip if this is before the occurrence we're looking for
        // For occurrence+global (e.g., 2g), skip matches before Nth
        if let Some(kth) = occurrence {
            if match_count < kth {
                // For zero-length matches, we need to advance past them to avoid infinite loop
                if start == end {
                    result.push_str(&chars[last_end..end].iter().collect::<String>());
                    if let Some((raw, map)) = &byte_ctx {
                        let byte_end_pos = if end < map.len() { map[end] } else { raw.len() };
                        byte_result.extend_from_slice(&raw[last_byte_end..byte_end_pos]);
                        last_byte_end = byte_end_pos;
                    }
                    // Mark this as the previous match end so we don't match here again
                    prev_match_end_pos = Some(end);
                    // Advance past the zero-length match position
                    if end < chars.len() {
                        result.push(chars[end]);
                        if let Some((raw, map)) = &byte_ctx {
                            let byte_pos = if end < map.len() { map[end] } else { raw.len() };
                            let byte_next = if end + 1 < map.len() {
                                map[end + 1]
                            } else {
                                raw.len()
                            };
                            byte_result.extend_from_slice(&raw[byte_pos..byte_next]);
                            last_byte_end = byte_next;
                        }
                        last_end = end + 1;
                    } else {
                        last_end = end;
                    }
                } else {
                    result.push_str(&chars[last_end..end].iter().collect::<String>());
                    // Also update byte result
                    if let Some((raw, map)) = &byte_ctx {
                        let byte_end_pos = if end < map.len() { map[end] } else { raw.len() };
                        byte_result.extend_from_slice(&raw[last_byte_end..byte_end_pos]);
                        last_byte_end = byte_end_pos;
                    }
                    last_end = end;
                }
                continue;
            }
        }

        // Add text before match
        result.push_str(&chars[last_end..start].iter().collect::<String>());
        // Also add bytes before match
        if let Some((raw, map)) = &byte_ctx {
            let byte_start = if start < map.len() {
                map[start]
            } else {
                raw.len()
            };
            byte_result.extend_from_slice(&raw[last_byte_end..byte_start]);
        }

        // Render replacement
        let repl = render_replacement(&matched_text, captures.as_ref(), template);
        result.push_str(&repl);
        // Also render bytes replacement
        if byte_ctx.is_some() {
            let repl_bytes =
                render_replacement_to_bytes(&matched_text, captures.as_ref(), template);
            byte_result.extend_from_slice(&repl_bytes);
        }

        // Track where this match ended - used to skip zero-length matches at this position
        prev_match_end_pos = Some(end);

        // Handle zero-length matches: advance by one char to prevent infinite loop
        if start == end {
            if end < chars.len() {
                result.push(chars[end]);
                // Also add byte for this char
                if let Some((raw, map)) = &byte_ctx {
                    let byte_pos = if end < map.len() { map[end] } else { raw.len() };
                    let byte_next = if end + 1 < map.len() {
                        map[end + 1]
                    } else {
                        raw.len()
                    };
                    byte_result.extend_from_slice(&raw[byte_pos..byte_next]);
                    last_byte_end = byte_next;
                }
            }
            last_end = end + 1;
        } else {
            // Update last_byte_end for non-zero-length match
            if let Some((raw, map)) = &byte_ctx {
                last_byte_end = if end < map.len() { map[end] } else { raw.len() };
            }
            last_end = end;
        }

        // Stop after first replacement if not global mode
        if !global {
            break;
        }
    }

    // Add remaining text
    if last_end < chars.len() {
        result.push_str(&chars[last_end..].iter().collect::<String>());
    }
    // Add remaining bytes
    if let Some((raw, _)) = &byte_ctx {
        byte_result.extend_from_slice(&raw[last_byte_end..]);
    }

    // Return whether any match occurred (match_count > 0), not just whether string changed
    let matched = match_count > 0;
    let byte_result_opt = if byte_ctx.is_some() && matched {
        Some(byte_result)
    } else {
        None
    };
    if result == text {
        (Cow::Borrowed(text), matched, byte_result_opt)
    } else {
        (Cow::Owned(result), matched, byte_result_opt)
    }
}

/// Result of a substitution operation
struct SubstitutionOutcome {
    /// The new pattern space content (String)
    new_text: String,
    /// Whether a match occurred (regardless of whether text changed)
    matched: bool,
    /// Raw bytes if they were updated (for preserving invalid UTF-8)
    raw_bytes: Option<Vec<u8>>,
}

/// Extract raw bytes from replacement template's LiteralBytes and Literal tokens
/// This is used when we have a non-ASCII replacement but literal_replacement_bytes is None
fn extract_raw_bytes_from_template(template: &ReplacementTemplate) -> Vec<u8> {
    let mut result = Vec::new();
    for token in &template.tokens {
        match token {
            ReplacementToken::LiteralBytes(bytes) => {
                result.extend_from_slice(bytes);
            }
            ReplacementToken::Literal(s) => {
                result.extend_from_slice(s.as_bytes());
            }
            // Backreferences and case conversions are not supported in this path
            _ => {}
        }
    }
    result
}

/// Try byte-level substitution for non-UTF-8 patterns
/// Returns Some(outcome) if byte-level path was used, None otherwise
fn try_byte_substitution(
    current: &str,
    raw: &[u8],
    pat_bytes: &[u8],
    repl_bytes: &[u8],
    global: bool,
) -> SubstitutionOutcome {
    if let Some(new_raw) = replace_bytes(raw, pat_bytes, repl_bytes, global) {
        let new_text = String::from_utf8_lossy(&new_raw).into_owned();
        SubstitutionOutcome {
            new_text,
            matched: true,
            raw_bytes: Some(new_raw),
        }
    } else {
        SubstitutionOutcome {
            new_text: current.to_string(),
            matched: false,
            raw_bytes: None,
        }
    }
}

/// Try literal string substitution (fast path, 10-100x faster than regex)
/// Returns the substitution outcome with optional byte-level result for ASCII patterns
fn try_literal_substitution(
    current: &str,
    raw: &[u8],
    lit_pat: &str,
    lit_repl: &str,
    replacement: &ReplacementTemplate,
    literal_replacement_bytes: &Option<Vec<u8>>,
    global: bool,
) -> SubstitutionOutcome {
    let matched = current.contains(lit_pat);
    let new_text = if global {
        current.replace(lit_pat, lit_repl)
    } else {
        current.replacen(lit_pat, lit_repl, 1)
    };

    // For ASCII patterns, also apply to raw bytes to preserve invalid UTF-8
    let raw_bytes = if matched && lit_pat.is_ascii() {
        let pat_bytes = lit_pat.as_bytes();
        let repl_bytes_owned: Vec<u8>;
        let repl_bytes: &[u8] = if let Some(rb) = literal_replacement_bytes {
            rb.as_slice()
        } else if lit_repl.is_ascii() {
            lit_repl.as_bytes()
        } else {
            repl_bytes_owned = extract_raw_bytes_from_template(replacement);
            if repl_bytes_owned.is_empty() {
                return SubstitutionOutcome {
                    new_text,
                    matched,
                    raw_bytes: None,
                };
            }
            &repl_bytes_owned
        };
        replace_bytes(raw, pat_bytes, repl_bytes, global)
    } else {
        None
    };

    SubstitutionOutcome {
        new_text,
        matched,
        raw_bytes,
    }
}

/// Perform regex-based substitution
fn do_regex_substitution(
    current: &str,
    raw: &[u8],
    regex: &SedRegex,
    replacement: &ReplacementTemplate,
    global: bool,
    occurrence: Option<usize>,
) -> SubstitutionOutcome {
    // In MBCS locales, use byte-based matching to handle non-UTF-8 encodings correctly
    if crate::mbcs::is_multibyte_locale() {
        return do_regex_substitution_mb(raw, regex, replacement, global, occurrence);
    }

    let (new_value, matched, byte_result) =
        sed_replace_with_template(regex, current, replacement, global, occurrence, Some(raw));

    SubstitutionOutcome {
        new_text: new_value.into_owned(),
        matched,
        raw_bytes: byte_result,
    }
}

/// MBCS-aware regex substitution that works with raw bytes
fn do_regex_substitution_mb(
    raw: &[u8],
    regex: &SedRegex,
    replacement: &ReplacementTemplate,
    global: bool,
    occurrence: Option<usize>,
) -> SubstitutionOutcome {
    use crate::mbcs::MbText;

    let matcher = &regex.matcher;
    let mb_text = MbText::new(raw);

    let mut result_bytes: Vec<u8> = Vec::new();
    let mut last_byte_end: usize = 0;
    let mut match_count = 0;
    let mut had_match = false;
    // Track where the previous match ended - used to skip zero-length matches at that position
    let mut prev_match_end_pos: Option<usize> = None;

    loop {
        // Safety: prevent infinite loops
        if match_count > mb_text.byte_len() * 10 {
            break;
        }

        // Find next match using bytes
        let match_data = matcher.find_with_captures_bytes_from(raw, last_byte_end);

        let Some((start_byte, end_byte, captures)) = match_data else {
            break;
        };

        // Skip zero-length match at the position where previous match ended
        // This prevents patterns like [a-z]* from matching both "abc" AND then "" at position 3
        if start_byte == end_byte && prev_match_end_pos == Some(start_byte) {
            if end_byte < raw.len() {
                result_bytes.push(raw[end_byte]);
                last_byte_end = end_byte + 1;
            } else {
                // At end of string with zero-length match - break to avoid infinite loop
                break;
            }
            prev_match_end_pos = None;
            continue;
        }

        match_count += 1;

        // Check occurrence number - for occurrence+global (e.g., 2g), skip matches before Nth
        if let Some(kth) = occurrence {
            if match_count < kth {
                // For zero-length matches, we need to advance past them to avoid infinite loop
                if start_byte == end_byte {
                    result_bytes.extend_from_slice(&raw[last_byte_end..end_byte]);
                    // Mark this as the previous match end so we don't match here again
                    prev_match_end_pos = Some(end_byte);
                    // Advance past the zero-length match position
                    if end_byte < raw.len() {
                        result_bytes.push(raw[end_byte]);
                        last_byte_end = end_byte + 1;
                    } else {
                        last_byte_end = end_byte;
                    }
                } else {
                    result_bytes.extend_from_slice(&raw[last_byte_end..end_byte]);
                    last_byte_end = end_byte;
                }
                continue;
            }
        }

        // Add bytes before match
        result_bytes.extend_from_slice(&raw[last_byte_end..start_byte]);

        // Render replacement using bytes sink to preserve MBCS bytes
        let matched_bytes = &raw[start_byte..end_byte];
        let matched_text = String::from_utf8_lossy(matched_bytes);
        let repl_bytes = render_replacement_to_bytes(&matched_text, Some(&captures), replacement);
        result_bytes.extend_from_slice(&repl_bytes);

        had_match = true;

        // Track where this match ended - used to skip zero-length matches at this position
        prev_match_end_pos = Some(end_byte);

        // Handle zero-length matches: advance by at least one byte to prevent infinite loop
        if start_byte == end_byte {
            if end_byte < raw.len() {
                result_bytes.push(raw[end_byte]);
                last_byte_end = end_byte + 1;
            } else {
                // At end of string - break to avoid infinite loop
                last_byte_end = end_byte;
                break;
            }
        } else {
            last_byte_end = end_byte;
        }

        // Stop after first match if not global
        if !global {
            break;
        }
    }

    // Add remaining bytes
    result_bytes.extend_from_slice(&raw[last_byte_end..]);

    if had_match {
        let new_text = String::from_utf8_lossy(&result_bytes).into_owned();
        SubstitutionOutcome {
            new_text,
            matched: true,
            raw_bytes: Some(result_bytes),
        }
    } else {
        SubstitutionOutcome {
            new_text: String::from_utf8_lossy(raw).into_owned(),
            matched: false,
            raw_bytes: None,
        }
    }
}

/// Perform the core substitution operation
///
/// Dispatches to the appropriate substitution path:
/// 1. Byte-level for non-UTF-8 patterns
/// 2. Literal string for simple patterns (10-100x faster)
/// 3. Regex for complex patterns
#[allow(clippy::too_many_arguments)]
fn perform_substitution(
    current: &str,
    ctx: &ExecutionContext,
    pattern: &SedRegex,
    replacement: &ReplacementTemplate,
    global: bool,
    occurrence: Option<usize>,
    use_last: bool,
    last_s_regex: Option<&SedRegex>,
    literal_pattern: &Option<String>,
    literal_replacement: &Option<String>,
    literal_pattern_bytes: &Option<Vec<u8>>,
    literal_replacement_bytes: &Option<Vec<u8>>,
) -> SubstitutionOutcome {
    let raw = ctx.pattern_space.raw();

    // Path 1: Byte-level for non-UTF-8 patterns
    if let (Some(pat_bytes), Some(repl_bytes)) = (literal_pattern_bytes, literal_replacement_bytes)
    {
        return try_byte_substitution(current, raw, pat_bytes, repl_bytes, global);
    }

    // Path 2: Literal string fast path
    if let (Some(lit_pat), Some(lit_repl)) = (literal_pattern, literal_replacement) {
        return try_literal_substitution(
            current,
            raw,
            lit_pat,
            lit_repl,
            replacement,
            literal_replacement_bytes,
            global,
        );
    }

    // Path 3: Regex
    let effective_regex = if use_last {
        last_s_regex.unwrap_or(pattern)
    } else {
        pattern
    };
    do_regex_substitution(
        current,
        raw,
        effective_regex,
        replacement,
        global,
        occurrence,
    )
}

/// Replace occurrences of `pattern` with `replacement` in `src` bytes
///
/// If `global` is false, only replace the first occurrence.
/// Returns None if pattern not found, otherwise returns the modified bytes.
fn replace_bytes(src: &[u8], pattern: &[u8], replacement: &[u8], global: bool) -> Option<Vec<u8>> {
    if pattern.is_empty() {
        return None;
    }

    // Find first occurrence
    let pos = src.windows(pattern.len()).position(|w| w == pattern)?;

    let mut result = Vec::with_capacity(src.len());
    result.extend_from_slice(&src[..pos]);
    result.extend_from_slice(replacement);

    if !global {
        result.extend_from_slice(&src[pos + pattern.len()..]);
    } else {
        // For global, continue replacing in remaining part
        let mut remaining = &src[pos + pattern.len()..];
        while let Some(next_pos) = remaining.windows(pattern.len()).position(|w| w == pattern) {
            result.extend_from_slice(&remaining[..next_pos]);
            result.extend_from_slice(replacement);
            remaining = &remaining[next_pos + pattern.len()..];
        }
        result.extend_from_slice(remaining);
    }

    Some(result)
}

/// Handle translation (y command) at the byte level for C locale
fn translate_bytes(raw: &[u8], from_bytes: &[u8], to_bytes: &[u8]) -> Vec<u8> {
    // Build byte-to-byte translation table
    let mut byte_table: [u8; 256] = [0; 256];
    for i in 0..256 {
        byte_table[i] = i as u8;
    }
    let last_to_byte = to_bytes.last().copied();
    for (idx, &from_byte) in from_bytes.iter().enumerate() {
        let to_byte = if idx < to_bytes.len() {
            to_bytes[idx]
        } else if let Some(last_byte) = last_to_byte {
            last_byte
        } else {
            0
        };
        byte_table[from_byte as usize] = to_byte;
    }

    // Apply byte-level translation
    raw.iter().map(|&b| byte_table[b as usize]).collect()
}

/// Handle translation (y command) at the character level with byte output for U+FFFD
/// Uses from_bytes to match invalid UTF-8 bytes in both pattern and input
fn translate_chars_to_bytes(
    raw: &[u8],
    from_chars: &[char],
    from_bytes: &[u8],
    to_bytes: &[u8],
) -> Vec<u8> {
    use crate::parser::build_char_to_byte_mapping;

    let mut result = Vec::with_capacity(raw.len());
    let mut i = 0;

    // Build mapping from char index to byte positions in from_bytes
    let from_str: String = from_chars.iter().collect();
    let from_byte_starts = build_char_to_byte_mapping(&from_str, from_bytes);

    // Build mapping for to_bytes as well
    let to_str: String = String::from_utf8_lossy(to_bytes).into_owned();
    let to_byte_starts = build_char_to_byte_mapping(&to_str, to_bytes);

    // Helper to get byte range for char at index
    let get_from_range = |idx: usize| -> (usize, usize) {
        let start = from_byte_starts[idx];
        let end = from_byte_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(from_bytes.len());
        (start, end)
    };
    let get_to_range = |idx: usize| -> (usize, usize) {
        let start = to_byte_starts[idx];
        let end = to_byte_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(to_bytes.len());
        (start, end)
    };

    while i < raw.len() {
        // Try to decode a UTF-8 character from raw bytes
        let remaining = &raw[i..];
        let decoded = std::str::from_utf8(remaining);

        if let Ok(s) = decoded {
            // Valid UTF-8 from this point
            if let Some(c) = s.chars().next() {
                // Find if this char is in 'from'
                if let Some(idx) = from_chars.iter().position(|&fc| fc == c) {
                    // Use corresponding bytes from to_bytes
                    if idx < to_byte_starts.len() {
                        let (to_start, to_end) = get_to_range(idx);
                        result.extend_from_slice(&to_bytes[to_start..to_end]);
                    } else if let Some(&last) = to_bytes.last() {
                        result.push(last);
                    }
                } else {
                    // Not in translation set, copy original bytes
                    result.extend_from_slice(&raw[i..i + c.len_utf8()]);
                }
                i += c.len_utf8();
            } else {
                i += 1; // Should not happen
            }
        } else {
            // Invalid UTF-8 byte, check if it matches any byte in from_bytes
            let byte = raw[i];
            let mut matched = false;

            // Look for this byte in from_bytes (handling individual invalid bytes)
            for char_idx in 0..from_chars.len() {
                let (start, end) = get_from_range(char_idx);
                // Check if this is a single byte and matches
                if end - start == 1 && from_bytes[start] == byte {
                    // Found matching invalid byte in from pattern
                    // Get corresponding bytes from to_bytes
                    if char_idx < to_byte_starts.len() {
                        let (to_start, to_end) = get_to_range(char_idx);
                        result.extend_from_slice(&to_bytes[to_start..to_end]);
                    } else if let Some(&last) = to_bytes.last() {
                        result.push(last);
                    }
                    matched = true;
                    break;
                }
            }

            if !matched {
                // Single invalid byte not in translation set, copy as-is
                result.push(byte);
            }
            i += 1;
        }
    }

    result
}

/// Handle translation (y command) at the multibyte character level for non-UTF-8 multibyte locales
/// Uses libc mbrtowc to properly handle stateful encodings like Shift-JIS
fn translate_mb_chars(raw: &[u8], from_bytes: &[u8], to_bytes: &[u8]) -> Vec<u8> {
    use crate::mbcs::MbCharIter;

    // Parse from_bytes and to_bytes into multibyte character sequences
    let from_chars: Vec<&[u8]> = MbCharIter::new(from_bytes).collect();
    let to_chars: Vec<&[u8]> = MbCharIter::new(to_bytes).collect();

    // Build mapping from each from_char to corresponding to_char
    // Use the last to_char for excess from_chars (GNU sed behavior)
    let last_to = to_chars.last().copied();

    let mut result = Vec::with_capacity(raw.len());

    // Iterate over input as multibyte characters
    for mb_char in MbCharIter::new(raw) {
        // Look for this mb_char in from_chars
        if let Some(idx) = from_chars.iter().position(|&fc| fc == mb_char) {
            // Found - use corresponding to_char
            let replacement = if idx < to_chars.len() {
                to_chars[idx]
            } else {
                last_to.unwrap_or(mb_char)
            };
            result.extend_from_slice(replacement);
        } else {
            // Not found - keep original
            result.extend_from_slice(mb_char);
        }
    }

    result
}

/// Format pattern space content for 'l' command output
///
/// Escapes non-printable characters and wraps lines at max_line_len.
fn format_list_output(raw: &[u8], max_line_len: usize, _null_data: bool) -> Vec<String> {
    // Helper to escape a byte for 'l' command output
    // GNU sed's l command outputs:
    // - Printable ASCII (32-126) as-is (except backslash)
    // - Backslash as \\
    // - Control chars: \a \b \t \n \v \f \r
    // - Other control chars (0-31, 127) as \ooo
    // - Non-ASCII bytes (128-255) as \ooo
    fn escape_byte(b: u8) -> String {
        match b {
            b'\\' => "\\\\".to_string(),
            0x07 => "\\a".to_string(),
            0x08 => "\\b".to_string(),
            b'\t' => "\\t".to_string(),
            b'\n' => "\\n".to_string(),
            0x0B => "\\v".to_string(),
            0x0C => "\\f".to_string(),
            b'\r' => "\\r".to_string(),
            // Printable ASCII (space through tilde, excluding backslash handled above)
            0x20..=0x7E => (b as char).to_string(),
            // Control chars (0-31) and DEL (127) - output as octal
            0x00..=0x1F | 0x7F => format!("\\{:03o}", b),
            // Non-ASCII bytes (128-255) - output as octal
            0x80..=0xFF => format!("\\{:03o}", b),
        }
    }

    let mut output_lines = Vec::new();

    // Process raw bytes directly (not UTF-8 conversion!)
    // GNU sed's l command outputs the entire pattern space as one logical line,
    // with embedded newlines escaped as \n, and only one $ at the very end
    let tokens: Vec<String> = raw.iter().map(|&b| escape_byte(b)).collect();

    let mut line_buf = String::new();
    let mut col_count: usize = 0;
    for tok in tokens.iter() {
        let tlen = tok.len(); // Use byte length since all escapes are ASCII
        let reserve_for = 1;
        if col_count + tlen + reserve_for > max_line_len {
            output_lines.push(format!("{}\\", line_buf));
            line_buf.clear();
            col_count = 0;
        }
        line_buf.push_str(tok);
        col_count += tlen;
    }
    // Add the final $ marker at the end
    if col_count + 1 > max_line_len {
        output_lines.push(format!("{}\\", line_buf));
        output_lines.push("$".to_string());
    } else {
        line_buf.push('$');
        output_lines.push(line_buf);
    }

    output_lines
}

/// Execute the translate (y) command
///
/// Returns the translated bytes based on locale settings:
/// - UTF-8 locale: character-level translation
/// - Multibyte non-UTF-8 (e.g., Shift-JIS): multibyte character translation
/// - C/POSIX single-byte: byte-level translation
fn execute_translate(
    raw: &[u8],
    from: &str,
    to: &str,
    from_bytes: Option<&Vec<u8>>,
    to_bytes: Option<&Vec<u8>>,
) -> Vec<u8> {
    let is_utf8 = parser_is_utf8_locale();
    let is_mb = crate::mbcs::is_multibyte_locale();

    if !is_utf8 && is_mb {
        // Multibyte non-UTF-8 locale (e.g., Shift-JIS)
        if let (Some(fb), Some(tb)) = (from_bytes, to_bytes) {
            return translate_mb_chars(raw, fb, tb);
        }
    } else if !is_utf8 {
        // C locale (single-byte) - use byte-level translation
        if let (Some(fb), Some(tb)) = (from_bytes, to_bytes) {
            return translate_bytes(raw, fb, tb);
        }
    } else {
        // UTF-8 locale - character-level translation
        let from_chars: Vec<char> = from.chars().collect();
        let to_chars: Vec<char> = to.chars().collect();
        let current = String::from_utf8_lossy(raw);

        // Check if we need byte-preserving path
        let input_has_invalid_utf8 = current.contains('\u{FFFD}');
        let pattern_has_invalid_bytes = from.contains('\u{FFFD}') || to.contains('\u{FFFD}');
        let needs_byte_path = input_has_invalid_utf8 || pattern_has_invalid_bytes;

        if needs_byte_path {
            if let (Some(fb), Some(tb)) = (from_bytes, to_bytes) {
                return translate_chars_to_bytes(raw, &from_chars, fb, tb);
            }
        } else {
            // Standard character-to-character translation
            let mut char_map: std::collections::HashMap<char, char> =
                std::collections::HashMap::new();
            let last_to_char = to_chars.last().copied();

            for (idx, &from_char) in from_chars.iter().enumerate() {
                let to_char = if idx < to_chars.len() {
                    to_chars[idx]
                } else if let Some(last_char) = last_to_char {
                    last_char
                } else {
                    '\0'
                };
                char_map.insert(from_char, to_char);
            }

            let translated: String = current
                .chars()
                .map(|c| *char_map.get(&c).unwrap_or(&c))
                .collect();

            return translated.into_bytes();
        }
    }

    // Fallback: return original bytes unchanged
    raw.to_vec()
}

pub fn apply_commands_with_context(
    commands: &[Command],
    ctx: &mut ExecutionContext,
    evaluator: &mut AddressEvaluator,
    start_pc: usize,
) -> Result<Vec<CommandResult>> {
    let mut current = ctx.pattern_space.text().to_string();
    let mut results = Vec::new();
    let mut last_substitution = false;
    let mut deferred_after_current: Vec<String> = Vec::new();

    let mut pc: usize = start_pc;
    while pc < commands.len() {
        match commands[pc] {
            Command::Substitution(ref subst) => {
                let should_execute =
                    evaluator.evaluate_with_negation(subst.range.as_ref(), subst.negated, ctx)?;
                if should_execute {
                    // Determine effective regex for tracking last_s_regex
                    let effective_regex: &SedRegex = if subst.use_last {
                        evaluator.last_s_regex.as_ref().unwrap_or(&subst.pattern)
                    } else {
                        &subst.pattern
                    };

                    // Check if pattern space is too large for regex (> INT_MAX)
                    // GNU sed panics with exit code 4 if buffer > 2^31-1 bytes
                    if current.len() > i32::MAX as usize {
                        return Err(SedError::InPlace {
                            message: "regex input buffer length larger than INT_MAX".to_string(),
                        });
                    }

                    // Perform the core substitution using helper function
                    let outcome = perform_substitution(
                        &current,
                        ctx,
                        &subst.pattern,
                        &subst.replacement,
                        subst.global,
                        subst.occurrence,
                        subst.use_last,
                        evaluator.last_s_regex.as_ref(),
                        &subst.literal_pattern,
                        &subst.literal_replacement,
                        &subst.literal_pattern_bytes,
                        &subst.literal_replacement_bytes,
                    );

                    // For t/T command tracking: only set to true when substitution succeeds.
                    if outcome.matched {
                        last_substitution = true;
                    }
                    current = outcome.new_text;
                    let raw_bytes_updated = if let Some(bytes) = outcome.raw_bytes {
                        ctx.pattern_space.set_raw(bytes);
                        true
                    } else {
                        false
                    };

                    // Handle print and execute flags based on order
                    if subst.print
                        && outcome.matched
                        && matches!(subst.print_timing, PrintTiming::PrintThenExecute)
                    {
                        results.push(CommandResult::Print(current.clone(), None));
                    }

                    // If execute flag is set, execute the replacement result as shell command
                    if subst.execute && outcome.matched {
                        match ProcessCommand::new("sh").arg("-c").arg(&current).output() {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                current = if ctx.null_data {
                                    stdout.to_string()
                                } else {
                                    stdout.trim_end_matches('\n').to_string()
                                };
                            }
                            Err(e) => {
                                eprintln!("sed: command execution failed: {}", e);
                            }
                        }
                    }

                    // Update pattern space if substitution happened and raw bytes not already updated
                    if outcome.matched && !raw_bytes_updated {
                        ctx.pattern_space.set(current.clone());
                    }

                    if subst.print
                        && outcome.matched
                        && !matches!(subst.print_timing, PrintTiming::PrintThenExecute)
                    {
                        results.push(CommandResult::Print(current.clone(), None));
                    }
                    if let Some(ref path) = subst.write_file {
                        if outcome.matched {
                            if let Ok(mut f) = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(path)
                            {
                                let _ = writeln!(f, "{}", current);
                            }
                        }
                    }
                    evaluator.last_s_regex = Some(effective_regex.clone());
                }
                pc += 1;
            }
            Command::Print { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // If current matches pattern_space text, use raw bytes to preserve invalid UTF-8
                    let raw_bytes = if current == ctx.pattern_space.text() {
                        Some(ctx.pattern_space.raw().to_vec())
                    } else {
                        None
                    };
                    results.push(CommandResult::Print(current.clone(), raw_bytes));
                }
                pc += 1;
            }
            Command::PrintFirstLine { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Print only the first line of pattern space (up to first newline, not including it)
                    // writeln! will add the newline when printing
                    let first_line = if let Some(pos) = current.find('\n') {
                        &current[..pos]
                    } else {
                        &current
                    };
                    results.push(CommandResult::Print(first_line.to_string(), None));
                }
                pc += 1;
            }
            Command::LineNumber { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    results.push(CommandResult::Print(ctx.current_line_num.to_string(), None));
                }
                pc += 1;
            }
            Command::Delete { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    results.push(CommandResult::Delete);
                    return Ok(results);
                }
                pc += 1;
            }
            Command::Quit {
                ref range,
                negated,
                exit_code,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // BSD sed: if not in quiet mode, quitting prints the current pattern space
                    if !ctx.quiet_mode {
                        results.push(CommandResult::Print(current.clone(), None));
                    }
                    results.push(CommandResult::Quit(exit_code));
                    return Ok(results);
                }
                pc += 1;
            }
            Command::QuitSilent {
                ref range,
                negated,
                exit_code,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Q command NEVER prints pattern space (silent quit)
                    results.push(CommandResult::Quit(exit_code));
                    return Ok(results);
                }
                pc += 1;
            }
            Command::Append {
                ref range,
                negated,
                ref text,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Defer appended text until after the current line output
                    // Only append if text is Some (explicit text, even if empty)
                    // None means unterminated command - no text to append
                    if let Some(t) = text {
                        deferred_after_current.push(t.clone());
                    }
                }
                pc += 1;
            }
            Command::Insert {
                ref range,
                negated,
                ref text,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Insert text is output immediately when the command is encountered
                    // Only insert if text is Some (explicit text, even if empty)
                    if let Some(t) = text {
                        results.push(CommandResult::Print(t.clone(), None));
                    }
                }
                pc += 1;
            }
            Command::Change {
                ref range,
                negated,
                ref text,
            } => {
                // For negated ranges (addr!) or when no range, we treat each matching line independently
                if negated || range.is_none() {
                    let should_execute =
                        evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                    if should_execute {
                        // Replace current line with text and end the cycle
                        // Only print if text is Some (explicit text, even if empty)
                        if let Some(t) = text {
                            results.push(CommandResult::Print(t.clone(), None));
                        }
                        results.push(CommandResult::Delete);
                        return Ok(results);
                    }
                    pc += 1;
                    continue;
                }

                // Non-negated range: print once at the end of the range
                // SAFETY: range.is_none() was checked above, so range is Some here
                let range_ref = range.as_ref().expect("range is Some after is_none() check");
                let (matches, end_here) = evaluator.evaluate_range_for_change(range_ref, ctx)?;
                if matches {
                    if end_here {
                        // At the end of the range: print replacement once and end cycle
                        // Only print if text is Some (explicit text, even if empty)
                        if let Some(t) = text {
                            results.push(CommandResult::Print(t.clone(), None));
                        }
                        results.push(CommandResult::Delete);
                        return Ok(results);
                    } else {
                        // Inside the range: delete current line without printing
                        results.push(CommandResult::Delete);
                        return Ok(results);
                    }
                }
                pc += 1;
            }
            Command::N { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Flush deferred appends before performing N (BSD semantics)
                    for line in deferred_after_current.drain(..) {
                        results.push(CommandResult::Print(line, None));
                    }
                    // Continue execution from the next command after appending next input line
                    results.push(CommandResult::AppendNextAndResume {
                        resume_pc: pc + 1,
                        pattern_space: current.clone(),
                    });
                    return Ok(results);
                }
                pc += 1;
            }
            Command::BigD { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Use optimized O(1) delete_first_line instead of O(n) Vec allocation
                    if ctx.pattern_space.delete_first_line() {
                        // More content remains - restart cycle with remaining content
                        results.push(CommandResult::Restart);
                    } else {
                        // No newline found - pattern space is now empty, delete and move to next line
                        results.push(CommandResult::Delete);
                    }
                    return Ok(results);
                }
                pc += 1;
            }
            Command::HoldCopy { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    ctx.hold_space = current.clone();
                    // If current matches pattern_space text, raw bytes weren't modified
                    if current == ctx.pattern_space.text() {
                        ctx.hold_space_raw = ctx.pattern_space.raw().to_vec();
                    } else {
                        // Content was modified (e.g., by regex), use current's UTF-8 bytes
                        ctx.hold_space_raw = current.as_bytes().to_vec();
                    }
                }
                pc += 1;
            }
            Command::HoldAppend { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // GNU sed behavior: H ALWAYS appends \n + pattern_space, even to empty hold
                    // Update String hold_space
                    ctx.hold_space.push('\n');
                    ctx.hold_space.push_str(&current);
                    // Update raw bytes hold_space
                    let raw_to_append = if current == ctx.pattern_space.text() {
                        ctx.pattern_space.raw()
                    } else {
                        current.as_bytes()
                    };
                    ctx.hold_space_raw.push(b'\n');
                    ctx.hold_space_raw.extend_from_slice(raw_to_append);
                }
                pc += 1;
            }
            Command::GetCopy { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    current = ctx.hold_space.clone();
                    // Update ctx.pattern_space from raw bytes to preserve invalid UTF-8
                    ctx.pattern_space.set_raw(ctx.hold_space_raw.clone());
                    // Don't reset all_input_consumed flag - EOF status is independent of pattern space content
                }
                pc += 1;
            }
            Command::GetAppend { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // GNU sed behavior: G always appends \n + hold_space, even if hold is empty
                    current.push('\n');
                    current.push_str(&ctx.hold_space);
                    // Also append to pattern_space raw bytes to preserve invalid UTF-8
                    let mut new_raw = if current.len() > ctx.hold_space.len() + 1
                        && ctx.pattern_space.text()
                            == &current[..current.len() - ctx.hold_space.len() - 1]
                    {
                        // Pattern space wasn't modified, use raw bytes
                        ctx.pattern_space.raw().to_vec()
                    } else {
                        // Pattern space was modified, use current's UTF-8 bytes up to this point
                        current[..current.len() - ctx.hold_space.len() - 1]
                            .as_bytes()
                            .to_vec()
                    };
                    new_raw.push(b'\n');
                    new_raw.extend_from_slice(&ctx.hold_space_raw);
                    ctx.pattern_space.set_raw(new_raw);
                }
                pc += 1;
            }
            Command::Exchange { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Determine pattern space raw bytes before swap
                    let pattern_raw = if current == ctx.pattern_space.text() {
                        // Pattern space wasn't modified, use original raw bytes
                        ctx.pattern_space.raw().to_vec()
                    } else {
                        // Pattern was modified, use current's UTF-8 bytes
                        current.as_bytes().to_vec()
                    };
                    // Swap String values
                    std::mem::swap(&mut ctx.hold_space, &mut current);
                    // Swap raw bytes
                    let hold_raw = std::mem::replace(&mut ctx.hold_space_raw, pattern_raw);
                    ctx.pattern_space.set_raw(hold_raw);
                    // Don't reset all_input_consumed flag - EOF status is independent of pattern space content
                }
                pc += 1;
            }
            Command::Label { .. } => {
                pc += 1;
            }
            Command::Branch {
                ref range,
                negated,
                target_index,
                ..
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    if let Some(idx) = target_index {
                        // Use pre-resolved target index
                        pc = idx + 1;
                    } else {
                        // Empty label or fallback: branch to end of script
                        pc = commands.len();
                    }
                    continue;
                }
                pc += 1;
            }
            Command::Test {
                ref range,
                negated,
                target_index,
                ..
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute && last_substitution {
                    // Reset the substitution flag before taking any branch per sed semantics
                    last_substitution = false;
                    if let Some(idx) = target_index {
                        pc = idx + 1;
                    } else {
                        pc = commands.len();
                    }
                    continue;
                }
                last_substitution = false;
                pc += 1;
            }
            Command::TestNeg {
                ref range,
                negated,
                target_index,
                ..
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute && !last_substitution {
                    // Reset the substitution flag before taking any branch per sed semantics
                    last_substitution = false;
                    if let Some(idx) = target_index {
                        pc = idx + 1;
                    } else {
                        pc = commands.len();
                    }
                    continue;
                }
                last_substitution = false;
                pc += 1;
            }
            Command::Execute {
                ref range,
                negated,
                ref command,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Determine what command to execute
                    let cmd_to_run = match command {
                        Some(cmd) => cmd.clone(),
                        None => current.clone(),
                    };

                    // Execute the command using /bin/sh
                    match ProcessCommand::new("sh")
                        .arg("-c")
                        .arg(&cmd_to_run)
                        .output()
                    {
                        Ok(output) => {
                            let stdout_str = String::from_utf8_lossy(&output.stdout);
                            // Remove single trailing newline if present in normal mode
                            // In null-data mode, don't trim anything (output may contain null bytes as data)
                            let trimmed = if ctx.null_data {
                                // In null-data mode, keep output as-is (nulls are data, not separators)
                                stdout_str.as_ref()
                            } else {
                                // In normal mode, remove single trailing newline
                                stdout_str.strip_suffix('\n').unwrap_or(&stdout_str)
                            };

                            if command.is_none() {
                                // Standalone 'e' command: update pattern space with execution output
                                // but don't print it (autoprint will handle it)
                                current = trimmed.to_string();
                                ctx.pattern_space.set(current.clone());
                            } else {
                                // 'e' with explicit command: print the output
                                if !trimmed.is_empty() {
                                    results.push(CommandResult::Print(trimmed.to_string(), None));
                                }
                            }
                        }
                        Err(_) => {
                            // On error, pattern space remains unchanged
                        }
                    }
                }
                pc += 1;
            }
            Command::Clear { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Clear pattern space (set to empty string)
                    current = String::new();
                    ctx.pattern_space.clear();
                }
                pc += 1;
            }
            Command::PrintFilename { ref range, negated } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Print current filename (or "-" for stdin)
                    results.push(CommandResult::Print(ctx.current_filename.clone(), None));
                }
                pc += 1;
            }
            Command::Next => {
                if !ctx.quiet_mode {
                    results.push(CommandResult::Print(current.clone(), None));
                }
                // Check if there are more lines to read
                let has_next_line = if let Some(total) = ctx.total_lines {
                    ctx.current_line_num < total
                } else {
                    !ctx.all_input_consumed
                };

                if has_next_line {
                    // Continue execution from next command (pc + 1) with next line
                    results.push(CommandResult::NextLineAndResume { resume_pc: pc + 1 });
                } else {
                    // At EOF, quit
                    results.push(CommandResult::Delete);
                }
                return Ok(results);
            }
            Command::Write {
                ref range,
                negated,
                ref path,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        let _ = writeln!(f, "{}", current);
                    }
                }
                pc += 1;
            }
            Command::WriteFirstLine {
                ref range,
                negated,
                ref path,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Write only the first line (up to first newline, not including it)
                    let first_line = if let Some(pos) = current.find('\n') {
                        &current[..pos]
                    } else {
                        &current
                    };

                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        let _ = writeln!(f, "{}", first_line);
                    }
                }
                pc += 1;
            }
            Command::Read {
                ref range,
                negated,
                ref path,
            } => {
                // Special case: 0r file means prepend before first line
                let is_zero_prepend = matches!(
                    range,
                    Some(r) if matches!(r.start, Some(Address::Line(0))) && r.end.is_none()
                );

                if is_zero_prepend && ctx.current_line_num == 1 {
                    // Prepend mode: output file content BEFORE current line
                    if let Ok(content) = std::fs::read_to_string(path) {
                        for line in content.lines() {
                            results.push(CommandResult::Print(line.to_string(), None));
                        }
                    }
                } else {
                    // For all other cases (including ranges like 0,/pattern/r):
                    // execute on every line that matches the range
                    let should_execute =
                        evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                    if should_execute {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            for line in content.lines() {
                                deferred_after_current.push(line.to_string());
                            }
                        }
                    }
                }
                pc += 1;
            }
            Command::ReadLine {
                ref range,
                negated,
                ref path,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Read one line from file (maintains file position across invocations)
                    if let Some(line) = ctx.read_line_from_file(path) {
                        deferred_after_current.push(line);
                    }
                    // If EOF or error, silently do nothing (GNU sed behavior)
                }
                pc += 1;
            }
            Command::Translate {
                ref range,
                negated,
                ref from,
                ref to,
                ref from_bytes,
                ref to_bytes,
            } => {
                if evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)? {
                    let translated = execute_translate(
                        ctx.pattern_space.raw(),
                        from,
                        to,
                        from_bytes.as_ref(),
                        to_bytes.as_ref(),
                    );
                    ctx.pattern_space.set_raw(translated);
                    current = ctx.pattern_space.text().to_string();
                }
                pc += 1;
            }
            Command::List {
                ref range,
                negated,
                line_length,
            } => {
                let should_execute =
                    evaluator.evaluate_with_negation(range.as_ref(), negated, ctx)?;
                if should_execute {
                    // Use command-specific line length if provided, otherwise use global setting
                    let max_line_len = line_length.unwrap_or(ctx.line_length);
                    let raw = ctx.pattern_space.raw();

                    // Use helper to format output
                    let output_lines = format_list_output(raw, max_line_len, ctx.null_data);
                    for line in output_lines {
                        results.push(CommandResult::Print(line, None));
                    }
                }
                pc += 1;
            }
        }
    }
    // If content wasn't modified (current equals the lossy text), preserve raw bytes
    let raw_bytes = if current == ctx.pattern_space.text() {
        Some(ctx.pattern_space.raw().to_vec())
    } else {
        // Content was modified, sync back and use None (will output String bytes)
        ctx.pattern_space.set(current.clone());
        None
    };
    results.push(CommandResult::Continue(current, raw_bytes));
    // Append commands output AFTER pattern space
    for line in deferred_after_current {
        results.push(CommandResult::Print(line, None));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_case_transform_simple() {
        let mut up_next = false;
        let mut lo_next = false;
        let result = apply_case_transform("hello", &mut up_next, &mut lo_next, false, false);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_apply_case_transform_uppercase_next() {
        let mut up_next = true;
        let mut lo_next = false;
        let result = apply_case_transform("hello", &mut up_next, &mut lo_next, false, false);
        assert_eq!(result, "Hello");
        assert!(!up_next); // Should be reset after first char
    }

    #[test]
    fn test_apply_case_transform_lowercase_next() {
        let mut up_next = false;
        let mut lo_next = true;
        let result = apply_case_transform("HELLO", &mut up_next, &mut lo_next, false, false);
        assert_eq!(result, "hELLO");
        assert!(!lo_next);
    }

    #[test]
    fn test_apply_case_transform_uppercase_all() {
        let mut up_next = false;
        let mut lo_next = false;
        let result = apply_case_transform("hello", &mut up_next, &mut lo_next, true, false);
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn test_apply_case_transform_lowercase_all() {
        let mut up_next = false;
        let mut lo_next = false;
        let result = apply_case_transform("HELLO", &mut up_next, &mut lo_next, false, true);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_apply_case_transform_bytes_simple() {
        let mut up_next = false;
        let mut lo_next = false;
        let result = apply_case_transform_bytes(b"hello", &mut up_next, &mut lo_next, false, false);
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_apply_case_transform_bytes_uppercase_next() {
        let mut up_next = true;
        let mut lo_next = false;
        let result = apply_case_transform_bytes(b"hello", &mut up_next, &mut lo_next, false, false);
        assert_eq!(result, b"Hello");
        assert!(!up_next);
    }

    #[test]
    fn test_apply_case_transform_bytes_lowercase_next() {
        let mut up_next = false;
        let mut lo_next = true;
        let result = apply_case_transform_bytes(b"HELLO", &mut up_next, &mut lo_next, false, false);
        assert_eq!(result, b"hELLO");
        assert!(!lo_next);
    }

    #[test]
    fn test_apply_case_transform_bytes_uppercase_all() {
        let mut up_next = false;
        let mut lo_next = false;
        let result = apply_case_transform_bytes(b"hello", &mut up_next, &mut lo_next, true, false);
        assert_eq!(result, b"HELLO");
    }

    #[test]
    fn test_apply_case_transform_bytes_lowercase_all() {
        let mut up_next = false;
        let mut lo_next = false;
        let result = apply_case_transform_bytes(b"HELLO", &mut up_next, &mut lo_next, false, true);
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_apply_case_transform_bytes_preserves_high_bytes() {
        let mut up_next = false;
        let mut lo_next = false;
        // High bytes (>127) should be preserved unchanged
        let input = b"a\xc0\xc1b";
        let result = apply_case_transform_bytes(input, &mut up_next, &mut lo_next, true, false);
        assert_eq!(result, b"A\xc0\xc1B");
    }

    #[test]
    fn test_render_replacement_literal() {
        let template = ReplacementTemplate {
            tokens: vec![ReplacementToken::Literal("hello".to_string())],
        };
        let result = render_replacement("match", None, &template);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_render_replacement_whole_match() {
        let template = ReplacementTemplate {
            tokens: vec![ReplacementToken::WholeMatch],
        };
        let result = render_replacement("matched", None, &template);
        assert_eq!(result, "matched");
    }

    #[test]
    fn test_render_replacement_group_zero() {
        let template = ReplacementTemplate {
            tokens: vec![ReplacementToken::Group(0)],
        };
        let result = render_replacement("matched", None, &template);
        assert_eq!(result, "matched");
    }

    #[test]
    fn test_render_replacement_with_case_transforms() {
        let template = ReplacementTemplate {
            tokens: vec![
                ReplacementToken::UppercaseNext,
                ReplacementToken::Literal("hello".to_string()),
            ],
        };
        let result = render_replacement("", None, &template);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_render_replacement_uppercase_all() {
        let template = ReplacementTemplate {
            tokens: vec![
                ReplacementToken::UppercaseAll,
                ReplacementToken::Literal("hello".to_string()),
                ReplacementToken::EndCase,
            ],
        };
        let result = render_replacement("", None, &template);
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn test_render_replacement_lowercase_all() {
        let template = ReplacementTemplate {
            tokens: vec![
                ReplacementToken::LowercaseAll,
                ReplacementToken::Literal("HELLO".to_string()),
                ReplacementToken::EndCase,
            ],
        };
        let result = render_replacement("", None, &template);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_render_replacement_to_bytes_with_literal_bytes() {
        let template = ReplacementTemplate {
            tokens: vec![ReplacementToken::LiteralBytes(vec![0xC0, 0xC1])],
        };
        let result = render_replacement_to_bytes("", None, &template);
        assert_eq!(result, vec![0xC0, 0xC1]);
    }

    #[test]
    fn test_replace_bytes_simple() {
        let result = replace_bytes(b"hello world", b"world", b"rust", false);
        assert_eq!(result, Some(b"hello rust".to_vec()));
    }

    #[test]
    fn test_replace_bytes_global() {
        let result = replace_bytes(b"aaa", b"a", b"X", true);
        assert_eq!(result, Some(b"XXX".to_vec()));
    }

    #[test]
    fn test_replace_bytes_no_match() {
        let result = replace_bytes(b"hello", b"xyz", b"abc", false);
        assert!(result.is_none());
    }

    #[test]
    fn test_replace_bytes_empty_pattern() {
        let result = replace_bytes(b"hello", b"", b"abc", false);
        assert!(result.is_none());
    }

    #[test]
    fn test_translate_bytes() {
        let result = translate_bytes(b"abc", b"abc", b"xyz");
        assert_eq!(result, b"xyz");
    }

    #[test]
    fn test_translate_bytes_partial() {
        let result = translate_bytes(b"abcdef", b"ace", b"XYZ");
        assert_eq!(result, b"XbYdZf");
    }

    #[test]
    fn test_format_list_output_simple() {
        let result = format_list_output(b"hello", 70, false);
        assert_eq!(result, vec!["hello$"]);
    }

    #[test]
    fn test_format_list_output_with_tab() {
        let result = format_list_output(b"a\tb", 70, false);
        assert_eq!(result, vec!["a\\tb$"]);
    }

    #[test]
    fn test_format_list_output_with_newline() {
        let result = format_list_output(b"a\nb", 70, false);
        assert_eq!(result, vec!["a\\nb$"]);
    }

    #[test]
    fn test_format_list_output_with_backslash() {
        let result = format_list_output(b"a\\b", 70, false);
        assert_eq!(result, vec!["a\\\\b$"]);
    }

    #[test]
    fn test_format_list_output_with_null() {
        let result = format_list_output(b"a\x00b", 70, false);
        assert_eq!(result, vec!["a\\000b$"]);
    }

    #[test]
    fn test_format_list_output_with_high_byte() {
        let result = format_list_output(b"a\xfeb", 70, false);
        assert_eq!(result, vec!["a\\376b$"]);
    }

    #[test]
    fn test_format_list_output_wrapping() {
        // Very short line length to force wrapping
        let result = format_list_output(b"abc", 3, false);
        assert!(result.len() >= 2); // Should wrap
    }

    #[test]
    fn test_extract_raw_bytes_from_template() {
        let template = ReplacementTemplate {
            tokens: vec![
                ReplacementToken::LiteralBytes(vec![0xC0]),
                ReplacementToken::Literal("hi".to_string()),
            ],
        };
        let result = extract_raw_bytes_from_template(&template);
        assert_eq!(result, vec![0xC0, b'h', b'i']);
    }

    #[test]
    fn test_render_sink_string() {
        let mut sink = String::new();
        sink.write_str("hello");
        sink.write_bytes(b" world");
        assert_eq!(sink, "hello world");
    }

    #[test]
    fn test_render_sink_vec() {
        let mut sink: Vec<u8> = Vec::new();
        sink.write_str("hello");
        sink.write_bytes(b" world");
        assert_eq!(sink, b"hello world");
    }
}
