// Copyright (c) 2026 Red Authors
// License: MIT
//

// Custom high-performance regex engine for sed
// Implements BRE (Basic Regular Expression) and ERE (Extended Regular Expression)
//
// Architecture:
// - Three-level optimization: Literal → DFA → NFA+Backtracking
// - Specialized for sed use cases
// - Zero external dependencies (no fancy-regex, no regex crate)

pub mod ast;
pub mod backtrack;
pub mod dfa; // DFA compilation and deterministic execution
pub mod literal; // Literal string matching (fastest path)
pub mod nfa; // NFA construction via Thompson's algorithm
pub mod parser; // NFA backtracking for backreferences and multiline mode

use crate::errors::Result;
use crate::mbcs::is_multibyte_locale;
use ast::RegexNode;
use backtrack::NfaMatcher;
use dfa::DfaMatcher;
use literal::LiteralMatcher;
use std::collections::HashMap;

// Re-export Capture for use in other modules
pub use backtrack::Capture;

/// Check if a regex AST needs backtracking (can't be handled by DFA correctly)
/// This detects patterns like .* followed by specific characters that need
/// the NFA to backtrack to find the longest match.
fn needs_backtracking(node: &RegexNode) -> bool {
    match node {
        RegexNode::Sequence(nodes) => {
            // Check if there's a .* (Any with ZeroOrMore/OneOrMore) followed by something
            for i in 0..nodes.len() {
                let is_any_star = matches!(
                    &nodes[i],
                    RegexNode::ZeroOrMore(inner) | RegexNode::OneOrMore(inner)
                        if matches!(**inner, RegexNode::Any)
                );
                if is_any_star && i + 1 < nodes.len() {
                    // .* followed by something - needs backtracking
                    return true;
                }
                // Recursively check children
                if needs_backtracking(&nodes[i]) {
                    return true;
                }
            }
            false
        }
        RegexNode::Group { node: inner, .. } => needs_backtracking(inner),
        RegexNode::Alternation(alts) => alts.iter().any(needs_backtracking),
        RegexNode::ZeroOrMore(inner)
        | RegexNode::OneOrMore(inner)
        | RegexNode::ZeroOrOne(inner) => needs_backtracking(inner),
        RegexNode::Repeat { node: inner, .. } => needs_backtracking(inner),
        _ => false,
    }
}

/// Three-level optimization matcher
#[derive(Debug, Clone)]
pub enum Matcher {
    /// Level 1: Literal string matching (fastest path)
    Literal(LiteralMatcher),

    /// Level 2: DFA matching (deterministic, fast)
    Dfa(DfaMatcher),

    /// Level 3: NFA with backtracking (handles backreferences and multiline)
    Nfa(NfaMatcher),
}

impl Matcher {
    /// Compile a BRE or ERE pattern into a Matcher
    pub fn compile(pattern: &str, is_ere: bool, ignore_case: bool) -> Result<Self> {
        Self::compile_with_flags(pattern, is_ere, ignore_case, false, false)
    }

    /// Compile a BRE or ERE pattern with multiline flag
    pub fn compile_with_flags(
        pattern: &str,
        is_ere: bool,
        ignore_case: bool,
        multiline: bool,
        posix_mode: bool,
    ) -> Result<Self> {
        // In multibyte locales (Shift-JIS, EUC-JP, etc.), we need to be careful
        // DFA matcher doesn't have MBCS support, but Literal matcher can handle
        // ASCII-only patterns safely (ASCII bytes don't overlap with MBCS lead bytes)
        let is_mbcs = is_multibyte_locale();

        // Parse pattern to AST
        let compiled = if is_ere {
            parser::parse_ere(pattern, posix_mode)?
        } else {
            parser::parse_bre(pattern, posix_mode)?
        };

        // Level 1: Check if literal pattern - use byte-level matching (fastest path)
        // In MBCS locales, only use Literal matcher for ASCII-only patterns
        // (ASCII bytes 0x00-0x7F don't overlap with MBCS lead bytes)
        if literal::is_literal(&compiled.ast) {
            if let Some(literal_str) = literal::to_literal_string(&compiled.ast) {
                // In MBCS locales, only allow ASCII-only patterns for Literal matcher
                let is_ascii_pattern = literal_str.bytes().all(|b| b < 128);
                if !is_mbcs || is_ascii_pattern {
                    return Ok(Matcher::Literal(LiteralMatcher::new(
                        literal_str,
                        ignore_case,
                    )));
                }
            }
        }

        // Level 2: Try DFA compilation (faster than NFA backtracking)
        // Skip DFA for multiline mode since it doesn't support it yet
        // Also skip in MBCS locales - DFA doesn't have MBCS support
        // Also skip for patterns that need backtracking (e.g., .* followed by specific chars)
        let needs_bt = needs_backtracking(&compiled.ast);
        if !multiline && !is_mbcs && !needs_bt {
            match DfaMatcher::compile(pattern, is_ere, ignore_case, posix_mode) {
                Ok(dfa) => return Ok(Matcher::Dfa(dfa)),
                Err(_) => {
                    // DFA failed (has backreferences or other unsupported features)
                    // Fall through to NFA with backtracking
                }
            }
        }

        // Level 3: NFA with backtracking (handles backreferences, multiline, and MBCS)
        match NfaMatcher::compile_with_flags(pattern, is_ere, ignore_case, multiline, posix_mode) {
            Ok(nfa) => Ok(Matcher::Nfa(nfa)),
            Err(e) => Err(e),
        }
    }

    /// Check if pattern matches text
    pub fn is_match(&self, text: &str) -> bool {
        match self {
            Matcher::Literal(m) => m.is_match(text),
            Matcher::Dfa(m) => m.is_match(text),
            Matcher::Nfa(m) => m.is_match(text),
        }
    }

    /// Replace first/all matches in text
    pub fn replace(&self, text: &str, replacement: &str, global: bool) -> String {
        match self {
            Matcher::Literal(m) => {
                if global {
                    m.replace_all(text, replacement)
                } else {
                    m.replace_first(text, replacement)
                }
            }
            Matcher::Dfa(m) => {
                if global {
                    m.replace_all(text, replacement)
                } else {
                    m.replace_first(text, replacement)
                }
            }
            Matcher::Nfa(m) => {
                if global {
                    m.replace_all(text, replacement)
                } else {
                    m.replace_first(text, replacement)
                }
            }
        }
    }

    /// Check if this matcher uses literal optimization
    pub fn is_literal(&self) -> bool {
        matches!(self, Matcher::Literal(_))
    }

    /// Find first match and return captures (for backreference replacements)
    pub fn find_with_captures(
        &self,
        text: &str,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        match self {
            Matcher::Nfa(m) => m.find_with_captures_pub(text),
            // Other matchers don't support captures
            _ => None,
        }
    }

    /// Find first match in raw bytes and return captures with byte offsets
    /// This method is for MBCS locales where we need to work with raw bytes
    pub fn find_with_captures_bytes(
        &self,
        bytes: &[u8],
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        match self {
            Matcher::Nfa(m) => m.find_with_captures_bytes(bytes),
            // In non-MB locales, fall back to string matching
            _ => {
                let text = String::from_utf8_lossy(bytes);
                self.find_with_captures(&text)
            }
        }
    }

    /// Find first match in raw bytes starting from a byte offset
    pub fn find_with_captures_bytes_from(
        &self,
        bytes: &[u8],
        start_byte: usize,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        match self {
            Matcher::Nfa(m) => m.find_with_captures_bytes_from(bytes, start_byte),
            // Literal matcher: use byte-level matching (no UTF-8 conversion needed)
            Matcher::Literal(m) => {
                if start_byte >= bytes.len() {
                    return None;
                }
                // Use byte-level find to avoid from_utf8_lossy corruption in MBCS locales
                m.find_bytes_from(bytes, start_byte).map(|pos| {
                    let end = pos + m.pattern_bytes().len();
                    (pos, end, HashMap::new())
                })
            }
            // DFA matcher: fall back to string matching (only used in non-MBCS locales)
            Matcher::Dfa(_) => {
                if start_byte >= bytes.len() {
                    return None;
                }
                // This path is only reached in non-MBCS locales where UTF-8 is safe
                let substring = String::from_utf8_lossy(&bytes[start_byte..]);
                self.find_with_captures(&substring)
                    .map(|(s, e, c)| (s + start_byte, e + start_byte, c))
            }
        }
    }

    /// Replace with template support (for & whole match replacement)
    /// Returns an Option with (matched_text, start, end) for each match
    pub fn find_with_text<'t>(&self, text: &'t str) -> Option<(&'t str, usize, usize)> {
        match self {
            Matcher::Literal(m) => {
                m.find(text).map(|start| {
                    let chars: Vec<char> = text.chars().collect();
                    let pattern_len = m.pattern().chars().count();
                    let end = start + pattern_len;
                    // Convert to byte indices
                    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                    let byte_end = byte_start + m.pattern().len();
                    (&text[byte_start..byte_end], start, end)
                })
            }
            Matcher::Dfa(m) => m.find(text).map(|(start, end)| {
                let chars: Vec<char> = text.chars().collect();
                // Need to convert char indices to byte indices
                let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
                (&text[byte_start..byte_end], start, end)
            }),
            Matcher::Nfa(m) => m.find(text).map(|(start, end)| {
                let chars: Vec<char> = text.chars().collect();
                let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
                (&text[byte_start..byte_end], start, end)
            }),
        }
    }

    /// Find match starting from a specific position
    pub fn find_with_text_from<'t>(
        &self,
        text: &'t str,
        start_from: usize,
    ) -> Option<(&'t str, usize, usize)> {
        match self {
            Matcher::Nfa(m) => {
                // NFA has proper find_from that maintains anchor positions
                m.find_from(text, start_from).map(|(start, end)| {
                    let chars: Vec<char> = text.chars().collect();
                    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                    let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
                    (&text[byte_start..byte_end], start, end)
                })
            }
            Matcher::Literal(m) => {
                // Literal matcher: search in substring (anchors not supported anyway)
                let chars: Vec<char> = text.chars().collect();
                if start_from > chars.len() {
                    return None;
                }
                let remaining: String = chars[start_from..].iter().collect();
                m.find(&remaining).map(|byte_rel_start| {
                    // Convert byte position to character position in remaining substring
                    let rel_start = remaining[..byte_rel_start].chars().count();
                    let start = start_from + rel_start;
                    let pattern_len = m.pattern().chars().count();
                    let end = start + pattern_len;
                    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                    let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
                    (&text[byte_start..byte_end], start, end)
                })
            }
            Matcher::Dfa(m) => {
                // DFA matcher: search in substring (anchors not supported anyway)
                let chars: Vec<char> = text.chars().collect();
                if start_from > chars.len() {
                    return None;
                }
                let remaining: String = chars[start_from..].iter().collect();
                m.find(&remaining).map(|(rel_start, rel_end)| {
                    let start = start_from + rel_start;
                    let end = start_from + rel_end;
                    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                    let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
                    (&text[byte_start..byte_end], start, end)
                })
            }
        }
    }

    /// Find with captures starting from a specific position
    /// For Literal and DFA matchers, returns empty captures (no groups captured)
    pub fn find_with_captures_from(
        &self,
        text: &str,
        start_from: usize,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        match self {
            Matcher::Nfa(m) => m.find_with_captures_from(text, start_from),
            // For Literal and DFA matchers, return match with empty captures
            // This allows backreferences like \1 to work (they'll just be empty)
            Matcher::Literal(_) | Matcher::Dfa(_) => self
                .find_with_text_from(text, start_from)
                .map(|(_, start, end)| (start, end, HashMap::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_matcher_compile() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        assert!(matcher.is_literal());
    }

    #[test]
    fn test_literal_matcher_is_match() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("foobar"));
        assert!(!matcher.is_match("bar"));
    }

    #[test]
    fn test_literal_matcher_replace() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        assert_eq!(matcher.replace("foo", "bar", false), "bar");
        assert_eq!(matcher.replace("foofoo", "bar", false), "barfoo");
        assert_eq!(matcher.replace("foofoo", "bar", true), "barbar");
    }

    #[test]
    fn test_non_literal_pattern() {
        let matcher = Matcher::compile("a*", false, false).unwrap();
        assert!(!matcher.is_literal());
    }

    #[test]
    fn test_non_literal_pattern_with_dot() {
        let matcher = Matcher::compile("a.b", false, false).unwrap();
        assert!(!matcher.is_literal());
    }

    // DFA integration tests

    #[test]
    fn test_dfa_simple_pattern() {
        let matcher = Matcher::compile("a*b", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        assert!(matcher.is_match("b"));
        assert!(matcher.is_match("ab"));
        assert!(matcher.is_match("aaab"));
    }

    #[test]
    fn test_dfa_alternation() {
        let matcher = Matcher::compile("foo\\|bar", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("bar"));
        assert!(!matcher.is_match("baz"));
    }

    #[test]
    fn test_dfa_char_class() {
        let matcher = Matcher::compile("[0-9]\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        assert!(matcher.is_match("123"));
        assert!(!matcher.is_match("abc"));
    }

    #[test]
    fn test_dfa_replace() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert_eq!(matcher.replace("aaa", "X", false), "X");
        assert_eq!(matcher.replace("aaa aaa", "X", true), "X X");
    }

    #[test]
    fn test_matcher_selection_priority() {
        // Literal patterns should use Literal matcher
        let lit = Matcher::compile("hello", false, false).unwrap();
        assert!(matches!(lit, Matcher::Literal(_)));

        // Regex patterns should use DFA matcher
        let dfa = Matcher::compile("hel*o", false, false).unwrap();
        assert!(matches!(dfa, Matcher::Dfa(_)));
    }

    // Backreference tests

    #[test]
    fn test_groups_in_pattern() {
        // Test pattern with groups
        let matcher = Matcher::compile("\\(.\\)\\(.\\)", false, false).unwrap();
        assert!(
            matches!(matcher, Matcher::Nfa(_)),
            "Should use NFA for groups"
        );
        assert!(matcher.is_match("hello"), "Should match 'hello'");

        // Test find_with_captures
        if let Some((start, end, captures)) = matcher.find_with_captures("hello") {
            println!("Match at {}-{}, captures: {:?}", start, end, captures);
            assert_eq!(start, 0);
            assert_eq!(end, 2, "Should match first two chars");
            assert_eq!(captures.len(), 2, "Should have 2 captures");
            assert_eq!(captures.get(&1).map(|c| c.text.as_str()), Some("h"));
            assert_eq!(captures.get(&2).map(|c| c.text.as_str()), Some("e"));
        } else {
            panic!("Should find match with captures");
        }
    }

    #[test]
    fn test_backref_in_pattern() {
        // Test backreference in pattern: \(foo\)\1 should match "foofoo"
        let matcher = Matcher::compile("\\(foo\\)\\1", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
        assert!(matcher.is_match("foofoo"), "Should match 'foofoo'");
        assert!(!matcher.is_match("foobar"), "Should not match 'foobar'");
    }

    #[test]
    fn test_posix_char_class() {
        // Test POSIX character class
        let matcher = Matcher::compile("[[:alpha:]]*", false, false).unwrap();
        println!("Matcher type: {:?}", std::mem::discriminant(&matcher));
        assert!(matcher.is_match("abc"), "Should match 'abc'");
        assert!(matcher.is_match("abc123"), "Should match 'abc123'");
    }

    #[test]
    fn test_negated_charset() {
        // Test negated character class [^a] - should match anything except 'a'
        let matcher = Matcher::compile("[^a]", false, false).unwrap();
        assert!(
            matches!(matcher, Matcher::Nfa(_)),
            "Should use NFA for negated charset"
        );

        assert!(matcher.is_match("b"), "Should match 'b'");
        assert!(matcher.is_match("xyz"), "Should match 'xyz'");
        assert!(!matcher.is_match("a"), "Should not match 'a'");

        // Test negated range [^a-c]
        let matcher2 = Matcher::compile("[^a-c]", false, false).unwrap();
        assert!(
            matches!(matcher2, Matcher::Nfa(_)),
            "Should use NFA for negated range"
        );

        assert!(!matcher2.is_match("a"), "Should not match 'a'");
        assert!(!matcher2.is_match("b"), "Should not match 'b'");
        assert!(!matcher2.is_match("c"), "Should not match 'c'");
        assert!(matcher2.is_match("d"), "Should match 'd'");
        assert!(matcher2.is_match("xyz"), "Should match 'xyz'");
    }

    // Tests for find_with_text
    #[test]
    fn test_find_with_text_literal() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        let result = matcher.find_with_text("hello foo world");
        assert!(result.is_some());
        let (matched, start, end) = result.unwrap();
        assert_eq!(matched, "foo");
        assert_eq!(start, 6);
        assert_eq!(end, 9);
    }

    #[test]
    fn test_find_with_text_dfa() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        let result = matcher.find_with_text("hello aaa world");
        assert!(result.is_some());
        let (matched, start, end) = result.unwrap();
        assert_eq!(matched, "aaa");
        assert_eq!(start, 6);
        assert_eq!(end, 9);
    }

    #[test]
    fn test_find_with_text_nfa() {
        let matcher = Matcher::compile("\\(a\\)\\1", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
        let result = matcher.find_with_text("hello aa world");
        assert!(result.is_some());
        let (matched, start, end) = result.unwrap();
        assert_eq!(matched, "aa");
        assert_eq!(start, 6);
        assert_eq!(end, 8);
    }

    // Tests for find_with_text_from
    #[test]
    fn test_find_with_text_from_literal() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        // First occurrence at index 0
        let result1 = matcher.find_with_text_from("foo bar foo", 0);
        assert!(result1.is_some());
        assert_eq!(result1.unwrap().1, 0);

        // Search from after first match
        let result2 = matcher.find_with_text_from("foo bar foo", 3);
        assert!(result2.is_some());
        assert_eq!(result2.unwrap().1, 8);
    }

    #[test]
    fn test_find_with_text_from_literal_past_end() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        let result = matcher.find_with_text_from("foo", 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_with_text_from_dfa() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        // First match at 0
        let result1 = matcher.find_with_text_from("aaa bbb aaa", 0);
        assert!(result1.is_some());
        assert_eq!(result1.unwrap().0, "aaa");

        // Search from after first match
        let result2 = matcher.find_with_text_from("aaa bbb aaa", 4);
        assert!(result2.is_some());
        assert_eq!(result2.unwrap().1, 8);
    }

    #[test]
    fn test_find_with_text_from_dfa_past_end() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        let result = matcher.find_with_text_from("aaa", 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_with_text_from_nfa() {
        let matcher = Matcher::compile("\\(a\\)\\1", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
        // "xx aa yy aa" - first "aa" is at index 3, second at index 9
        // Starting from 3 should find "aa" at 3
        let result = matcher.find_with_text_from("xx aa yy aa", 3);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, 3); // Found at position 3
    }

    // Tests for find_with_captures_from
    #[test]
    fn test_find_with_captures_from_literal() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        let result = matcher.find_with_captures_from("bar foo baz foo", 4);
        assert!(result.is_some());
        let (start, end, captures) = result.unwrap();
        assert_eq!(start, 4);
        assert_eq!(end, 7);
        assert!(captures.is_empty()); // Literal matcher has no captures
    }

    #[test]
    fn test_find_with_captures_from_dfa() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        // "bb aaa cc aaa" - searching from position 4 (after "bb a")
        // should find next 'aa' in "aa cc aaa" at relative 0, so absolute 4
        let result = matcher.find_with_captures_from("bb aaa cc aaa", 4);
        assert!(result.is_some());
        let (start, end, captures) = result.unwrap();
        assert_eq!(start, 4); // Found at position 4
        assert_eq!(end, 6);
        assert!(captures.is_empty()); // DFA matcher has no captures
    }

    #[test]
    fn test_find_with_captures_from_nfa() {
        let matcher = Matcher::compile("\\(a\\)\\(b\\)", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
        let result = matcher.find_with_captures_from("xx ab yy", 0);
        assert!(result.is_some());
        let (start, end, captures) = result.unwrap();
        assert_eq!(start, 3);
        assert_eq!(end, 5);
        assert_eq!(captures.len(), 2);
        assert_eq!(captures.get(&1).map(|c| c.text.as_str()), Some("a"));
        assert_eq!(captures.get(&2).map(|c| c.text.as_str()), Some("b"));
    }

    // Tests for find_with_captures_bytes
    #[test]
    fn test_find_with_captures_bytes_nfa() {
        let matcher = Matcher::compile("\\(a\\)\\(b\\)", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
        let result = matcher.find_with_captures_bytes(b"hello ab world");
        assert!(result.is_some());
        let (start, end, captures) = result.unwrap();
        assert_eq!(start, 6);
        assert_eq!(end, 8);
        assert_eq!(captures.len(), 2);
    }

    #[test]
    fn test_find_with_captures_bytes_literal_fallback() {
        // Note: find_with_captures_bytes for Literal matcher falls back to
        // find_with_captures which returns None for non-NFA matchers
        let matcher = Matcher::compile("foo", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Literal(_)));
        let result = matcher.find_with_captures_bytes(b"hello foo world");
        assert!(result.is_none()); // Literal matcher doesn't support captures
    }

    // Tests for find_with_captures_bytes_from
    #[test]
    fn test_find_with_captures_bytes_from_nfa() {
        let matcher = Matcher::compile("\\(a\\)\\(b\\)", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
        let result = matcher.find_with_captures_bytes_from(b"ab xx ab", 2);
        assert!(result.is_some());
        let (start, end, _) = result.unwrap();
        assert_eq!(start, 6);
        assert_eq!(end, 8);
    }

    #[test]
    fn test_find_with_captures_bytes_from_literal() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Literal(_)));
        let result = matcher.find_with_captures_bytes_from(b"foo bar foo", 4);
        assert!(result.is_some());
        let (start, end, captures) = result.unwrap();
        assert_eq!(start, 8);
        assert_eq!(end, 11);
        assert!(captures.is_empty());
    }

    #[test]
    fn test_find_with_captures_bytes_from_literal_past_end() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        let result = matcher.find_with_captures_bytes_from(b"foo", 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_with_captures_bytes_from_dfa() {
        // DFA matcher goes through string conversion path in find_with_captures_bytes_from
        // but then calls find_with_captures which returns None for DFA matchers
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        let result = matcher.find_with_captures_bytes_from(b"aaa bbb aaa", 4);
        // DFA matcher doesn't support captures through this path
        assert!(result.is_none());
    }

    #[test]
    fn test_find_with_captures_bytes_from_dfa_past_end() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        let result = matcher.find_with_captures_bytes_from(b"aaa", 100);
        assert!(result.is_none());
    }

    // Tests for multiline mode (forces NFA)
    #[test]
    fn test_multiline_mode_uses_nfa() {
        let matcher = Matcher::compile_with_flags("^foo", false, false, true, false).unwrap();
        assert!(matches!(matcher, Matcher::Nfa(_)));
    }

    // Test ERE mode
    #[test]
    fn test_ere_mode() {
        let matcher = Matcher::compile("a+", true, false).unwrap();
        assert!(matcher.is_match("aaa"));
        assert!(!matcher.is_match("bbb"));
    }

    // Test ignore case
    #[test]
    fn test_ignore_case() {
        let matcher = Matcher::compile("foo", false, true).unwrap();
        assert!(matcher.is_match("FOO"));
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("FoO"));
    }

    // Test find_with_captures returns None for non-NFA
    #[test]
    fn test_find_with_captures_returns_none_for_literal() {
        let matcher = Matcher::compile("foo", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Literal(_)));
        let result = matcher.find_with_captures("foo bar");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_with_captures_returns_none_for_dfa() {
        let matcher = Matcher::compile("a\\+", false, false).unwrap();
        assert!(matches!(matcher, Matcher::Dfa(_)));
        let result = matcher.find_with_captures("aaa bar");
        assert!(result.is_none());
    }
}
