// Copyright (c) 2026 Red Authors
// License: MIT
//

// Literal pattern matching - fastest path for simple patterns.
// Handles patterns like s/foo/bar/ using String::replace().

use super::ast::*;

/// Check if AST represents a literal pattern (no regex metacharacters)
pub fn is_literal(ast: &RegexNode) -> bool {
    match ast {
        // Single literal is literal
        RegexNode::Literal(_) => true,

        // Sequence of literals is literal
        RegexNode::Sequence(nodes) => nodes.iter().all(|n| matches!(n, RegexNode::Literal(_))),

        // Everything else is NOT literal (has regex features)
        _ => false,
    }
}

/// Extract literal string from AST
/// Returns Some(string) if pattern is literal, None otherwise
pub fn to_literal_string(ast: &RegexNode) -> Option<String> {
    match ast {
        RegexNode::Literal(ch) => Some(ch.to_string()),

        RegexNode::Sequence(nodes) => {
            let mut result = String::new();
            for node in nodes {
                match node {
                    RegexNode::Literal(ch) => result.push(*ch),
                    _ => return None, // Non-literal in sequence
                }
            }
            Some(result)
        }

        _ => None,
    }
}

/// Extract literal string from replacement template
pub fn to_literal_replacement(
    template: &crate::engine::types::ReplacementTemplate,
) -> Option<String> {
    use crate::engine::types::ReplacementToken;

    let mut result = String::new();
    for token in &template.tokens {
        match token {
            ReplacementToken::Literal(s) => result.push_str(s),
            _ => return None, // Non-literal token
        }
    }
    Some(result)
}

/// Literal matcher - uses String::replace() for maximum performance
#[derive(Debug, Clone)]
pub struct LiteralMatcher {
    pattern: String,
    ignore_case: bool,
}

impl LiteralMatcher {
    /// Create new literal matcher
    pub fn new(pattern: String, ignore_case: bool) -> Self {
        LiteralMatcher {
            pattern,
            ignore_case,
        }
    }

    /// Check if text matches pattern
    pub fn is_match(&self, text: &str) -> bool {
        if self.ignore_case {
            text.to_lowercase().contains(&self.pattern.to_lowercase())
        } else {
            text.contains(&self.pattern)
        }
    }

    /// Find first match position
    pub fn find(&self, text: &str) -> Option<usize> {
        if self.ignore_case {
            let text_lower = text.to_lowercase();
            let pattern_lower = self.pattern.to_lowercase();
            text_lower.find(&pattern_lower)
        } else {
            text.find(&self.pattern)
        }
    }

    /// Replace first occurrence
    pub fn replace_first(&self, text: &str, replacement: &str) -> String {
        if self.ignore_case {
            // Case-insensitive replace - need to find manually
            let text_lower = text.to_lowercase();
            let pattern_lower = self.pattern.to_lowercase();
            if let Some(pos) = text_lower.find(&pattern_lower) {
                let mut result = String::new();
                result.push_str(&text[..pos]);
                result.push_str(replacement);
                result.push_str(&text[pos + self.pattern.len()..]);
                result
            } else {
                text.to_string()
            }
        } else {
            text.replacen(&self.pattern, replacement, 1)
        }
    }

    /// Replace all occurrences
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        if self.ignore_case {
            // Case-insensitive replace all
            let mut result = String::new();
            let mut remaining = text;
            let pattern_lower = self.pattern.to_lowercase();

            while !remaining.is_empty() {
                let remaining_lower = remaining.to_lowercase();
                if let Some(pos) = remaining_lower.find(&pattern_lower) {
                    result.push_str(&remaining[..pos]);
                    result.push_str(replacement);
                    remaining = &remaining[pos + self.pattern.len()..];
                } else {
                    result.push_str(remaining);
                    break;
                }
            }
            result
        } else {
            text.replace(&self.pattern, replacement)
        }
    }

    /// Get pattern
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Get pattern as bytes
    pub fn pattern_bytes(&self) -> &[u8] {
        self.pattern.as_bytes()
    }

    /// Check if pattern is ASCII-only (safe for byte-level matching in MBCS locales)
    pub fn is_ascii_pattern(&self) -> bool {
        self.pattern.bytes().all(|b| b < 128)
    }

    /// Find first match in raw bytes (byte-level, no UTF-8 conversion)
    /// Returns byte offset of match start, or None if not found
    /// Note: Only works correctly for ASCII patterns or when byte-level matching is safe
    pub fn find_bytes(&self, bytes: &[u8]) -> Option<usize> {
        if self.ignore_case {
            // Case-insensitive byte matching only works for ASCII
            if !self.is_ascii_pattern() {
                return None;
            }
            let pattern_lower: Vec<u8> = self
                .pattern
                .bytes()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            bytes.windows(pattern_lower.len()).position(|window| {
                window
                    .iter()
                    .zip(pattern_lower.iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
            })
        } else {
            let pat = self.pattern.as_bytes();
            if pat.is_empty() {
                return Some(0);
            }
            bytes.windows(pat.len()).position(|window| window == pat)
        }
    }

    /// Find first match in raw bytes starting from a byte offset
    pub fn find_bytes_from(&self, bytes: &[u8], start: usize) -> Option<usize> {
        if start >= bytes.len() {
            return None;
        }
        self.find_bytes(&bytes[start..]).map(|pos| pos + start)
    }

    /// Check if bytes contain pattern (byte-level)
    pub fn is_match_bytes(&self, bytes: &[u8]) -> bool {
        self.find_bytes(bytes).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_literal_single() {
        let node = RegexNode::Literal('a');
        assert!(is_literal(&node));
        assert_eq!(to_literal_string(&node), Some("a".to_string()));
    }

    #[test]
    fn test_is_literal_sequence() {
        let node = RegexNode::Sequence(vec![
            RegexNode::Literal('f'),
            RegexNode::Literal('o'),
            RegexNode::Literal('o'),
        ]);
        assert!(is_literal(&node));
        assert_eq!(to_literal_string(&node), Some("foo".to_string()));
    }

    #[test]
    fn test_not_literal_any() {
        let node = RegexNode::Any;
        assert!(!is_literal(&node));
        assert_eq!(to_literal_string(&node), None);
    }

    #[test]
    fn test_not_literal_with_star() {
        let node = RegexNode::ZeroOrMore(Box::new(RegexNode::Literal('a')));
        assert!(!is_literal(&node));
    }

    #[test]
    fn test_not_literal_sequence_with_any() {
        let node = RegexNode::Sequence(vec![
            RegexNode::Literal('a'),
            RegexNode::Any,
            RegexNode::Literal('b'),
        ]);
        assert!(!is_literal(&node));
        assert_eq!(to_literal_string(&node), None);
    }

    #[test]
    fn test_literal_matcher_is_match() {
        let matcher = LiteralMatcher::new("foo".to_string(), false);
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("foobar"));
        assert!(matcher.is_match("barfoo"));
        assert!(!matcher.is_match("fo"));
        assert!(!matcher.is_match("bar"));
    }

    #[test]
    fn test_literal_matcher_replace_first() {
        let matcher = LiteralMatcher::new("foo".to_string(), false);
        assert_eq!(matcher.replace_first("foo", "bar"), "bar");
        assert_eq!(matcher.replace_first("foofoo", "bar"), "barfoo");
        assert_eq!(matcher.replace_first("xxxfoo", "bar"), "xxxbar");
        assert_eq!(matcher.replace_first("xxx", "bar"), "xxx");
    }

    #[test]
    fn test_literal_matcher_replace_all() {
        let matcher = LiteralMatcher::new("foo".to_string(), false);
        assert_eq!(matcher.replace_all("foo", "bar"), "bar");
        assert_eq!(matcher.replace_all("foofoo", "bar"), "barbar");
        assert_eq!(matcher.replace_all("xxxfooyyy", "bar"), "xxxbaryyy");
        assert_eq!(matcher.replace_all("xxx", "bar"), "xxx");
    }

    #[test]
    fn test_literal_matcher_case_insensitive() {
        let matcher = LiteralMatcher::new("foo".to_string(), true);
        assert!(matcher.is_match("FOO"));
        assert!(matcher.is_match("FoO"));
        assert_eq!(matcher.replace_first("FOO", "bar"), "bar");
        assert_eq!(matcher.replace_all("FoOFoo", "bar"), "barbar");
    }

    #[test]
    fn test_literal_matcher_find() {
        let matcher = LiteralMatcher::new("foo".to_string(), false);
        assert_eq!(matcher.find("foo"), Some(0));
        assert_eq!(matcher.find("barfoo"), Some(3));
        assert_eq!(matcher.find("bar"), None);
    }
}
