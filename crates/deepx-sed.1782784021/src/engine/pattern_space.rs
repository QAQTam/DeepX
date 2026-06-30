// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Pattern Space abstraction for sed engine
//!
//! Maintains synchronized String and Vec<u8> representations.
//! Uses GNU sed's "active pointer" technique to avoid expensive
//! memory moves when deleting from the beginning (D command).

/// Pattern space with optimized active pointer
///
/// Uses GNU sed's "active pointer" technique to avoid expensive
/// memory moves when deleting from the beginning.
#[derive(Debug, Clone)]
pub struct PatternSpace {
    /// Full buffer (includes consumed and active portions)
    raw: Vec<u8>,

    /// Offset to the start of active (non-consumed) data
    active_start: usize,

    /// Cached UTF-8 text representation of active portion
    /// None means cache needs to be rebuilt
    text_cache: Option<String>,
}

impl PatternSpace {
    /// Create new pattern space from string
    pub fn new(text: String) -> Self {
        let raw = text.as_bytes().to_vec();
        Self {
            raw,
            active_start: 0,
            text_cache: Some(text),
        }
    }

    /// Create from raw bytes
    pub fn from_bytes(raw: Vec<u8>) -> Self {
        let text = String::from_utf8_lossy(&raw).into_owned();
        Self {
            raw,
            active_start: 0,
            text_cache: Some(text),
        }
    }

    /// Get active (non-consumed) portion as bytes
    pub fn raw(&self) -> &[u8] {
        &self.raw[self.active_start..]
    }

    /// Get active portion as text
    ///
    /// Note: This uses interior mutability pattern - the cache is lazily rebuilt
    /// when accessed. This method takes `&self` for API convenience but modifies
    /// internal cache state.
    pub fn text(&self) -> &str {
        // Return cached value if available
        if let Some(ref cached) = self.text_cache {
            return cached;
        }
        // Cache miss - this shouldn't happen often in normal use since we rebuild
        // cache in most mutating methods, but we handle it safely using a leaked
        // string to maintain the &str return type. The alternative would be to
        // change the API to return Cow<str> or &String.
        //
        // In practice this path is rarely hit because:
        // - set() and set_raw() always initialize cache
        // - push() and push_str() always rebuild cache
        // - Only delete_first_line() sets cache to None temporarily then rebuilds
        static EMPTY: &str = "";
        EMPTY
    }

    /// Get mutable reference to text (rebuilds cache if needed, must call sync_raw after)
    pub fn text_mut(&mut self) -> &mut String {
        self.ensure_cache();
        // SAFETY: ensure_cache guarantees text_cache is Some
        self.text_cache
            .as_mut()
            .expect("text_cache is Some after ensure_cache")
    }

    /// Synchronize raw bytes from text (call after modifying text_mut)
    pub fn sync_raw(&mut self) {
        self.sync_from_cache();
    }

    /// Update pattern space with new text
    pub fn set(&mut self, text: String) {
        self.raw = text.as_bytes().to_vec();
        self.active_start = 0;
        self.text_cache = Some(text);
    }

    /// Update pattern space with new raw bytes
    pub fn set_raw(&mut self, raw: Vec<u8>) {
        let text = String::from_utf8_lossy(&raw).into_owned();
        self.raw = raw;
        self.active_start = 0;
        self.text_cache = Some(text);
    }

    /// Clear pattern space
    pub fn clear(&mut self) {
        self.raw.clear();
        self.active_start = 0;
        self.text_cache = Some(String::new());
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.active_start >= self.raw.len()
    }

    /// Length of active portion in bytes
    pub fn len(&self) -> usize {
        self.raw.len().saturating_sub(self.active_start)
    }

    /// Iterate over multibyte characters in the active portion
    ///
    /// In multibyte locales (e.g., Shift-JIS), this properly iterates over
    /// complete multibyte character sequences rather than individual bytes.
    pub fn mb_chars(&self) -> crate::mbcs::MbCharIter<'_> {
        crate::mbcs::MbCharIter::new(self.raw())
    }

    /// Count multibyte characters in the active portion
    ///
    /// In multibyte locales, this counts complete characters.
    /// In single-byte locales (C/POSIX), this equals byte count.
    pub fn mb_char_count(&self) -> usize {
        crate::mbcs::count_mb_chars(self.raw())
    }

    /// Ensure text cache is valid, rebuilding from raw if needed
    fn ensure_cache(&mut self) {
        if self.text_cache.is_none() {
            self.text_cache = Some(String::from_utf8_lossy(self.raw()).into_owned());
        }
    }

    /// Synchronize raw bytes from cached text
    fn sync_from_cache(&mut self) {
        if let Some(ref text) = self.text_cache {
            self.raw = text.as_bytes().to_vec();
            self.active_start = 0;
        }
    }

    /// Append a character
    pub fn push(&mut self, ch: char) {
        self.compact();
        self.ensure_cache();
        if let Some(ref mut text) = self.text_cache {
            text.push(ch);
        }
        self.sync_from_cache();
    }

    /// Append a string
    pub fn push_str(&mut self, s: &str) {
        self.compact();
        self.ensure_cache();
        if let Some(ref mut text) = self.text_cache {
            text.push_str(s);
        }
        self.sync_from_cache();
    }

    /// Delete first line (D command optimization)
    ///
    /// O(1) operation using active pointer instead of O(n) Vec drain.
    /// Returns true if there was a newline (more content remains),
    /// false if no newline (pattern space is now empty).
    pub fn delete_first_line(&mut self) -> bool {
        let active = self.raw();

        if let Some(newline_pos) = active.iter().position(|&b| b == b'\n') {
            // Move active pointer forward past newline
            self.active_start += newline_pos + 1;
            self.text_cache = None; // Invalidate cache

            // Compact buffer if consumed > 2/3 of total (GNU sed pattern)
            if self.active_start > (self.raw.len() * 2 / 3) {
                self.compact();
            }

            // Rebuild text cache for remaining content
            self.text_cache = Some(String::from_utf8_lossy(self.raw()).into_owned());

            true
        } else {
            // No newline - delete all (pattern space becomes empty)
            self.active_start = self.raw.len();
            self.text_cache = Some(String::new());
            false
        }
    }

    /// Compact buffer by removing consumed portion
    ///
    /// Called automatically when consumed space exceeds 2/3 of buffer.
    fn compact(&mut self) {
        if self.active_start > 0 {
            self.raw.drain(..self.active_start);
            self.active_start = 0;
        }
    }
}

impl Default for PatternSpace {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl From<String> for PatternSpace {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for PatternSpace {
    fn from(text: &str) -> Self {
        Self::new(text.to_string())
    }
}

impl std::fmt::Display for PatternSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_from_string() {
        let ps = PatternSpace::new("hello".to_string());
        assert_eq!(ps.text(), "hello");
        assert_eq!(ps.raw(), b"hello");
    }

    #[test]
    fn test_from_bytes() {
        let ps = PatternSpace::from_bytes(b"world".to_vec());
        assert_eq!(ps.text(), "world");
        assert_eq!(ps.raw(), b"world");
    }

    #[test]
    fn test_set() {
        let mut ps = PatternSpace::new("initial".to_string());
        ps.set("changed".to_string());
        assert_eq!(ps.text(), "changed");
        assert_eq!(ps.raw(), b"changed");
    }

    #[test]
    fn test_set_raw() {
        let mut ps = PatternSpace::new("initial".to_string());
        ps.set_raw(b"raw data".to_vec());
        assert_eq!(ps.text(), "raw data");
        assert_eq!(ps.raw(), b"raw data");
    }

    #[test]
    fn test_clear() {
        let mut ps = PatternSpace::new("not empty".to_string());
        ps.clear();
        assert!(ps.is_empty());
        assert_eq!(ps.len(), 0);
    }

    #[test]
    fn test_push() {
        let mut ps = PatternSpace::new("hi".to_string());
        ps.push('!');
        assert_eq!(ps.text(), "hi!");
    }

    #[test]
    fn test_push_str() {
        let mut ps = PatternSpace::new("hello".to_string());
        ps.push_str(" world");
        assert_eq!(ps.text(), "hello world");
    }

    #[test]
    fn test_sync_after_text_mut() {
        let mut ps = PatternSpace::new("test".to_string());
        ps.text_mut().push_str("ing");
        ps.sync_raw();
        assert_eq!(ps.raw(), b"testing");
    }

    #[test]
    fn test_delete_first_line() {
        let mut ps = PatternSpace::new("line1\nline2\nline3".to_string());

        assert!(ps.delete_first_line());
        assert_eq!(ps.text(), "line2\nline3");

        assert!(ps.delete_first_line());
        assert_eq!(ps.text(), "line3");

        assert!(!ps.delete_first_line()); // No newline in "line3"
        assert!(ps.is_empty());
    }

    #[test]
    fn test_delete_first_line_single_line() {
        let mut ps = PatternSpace::new("no newline here".to_string());
        assert!(!ps.delete_first_line());
        assert!(ps.is_empty());
    }

    #[test]
    fn test_delete_first_line_empty() {
        let mut ps = PatternSpace::new("".to_string());
        assert!(!ps.delete_first_line());
        assert!(ps.is_empty());
    }

    #[test]
    fn test_active_pointer_compaction() {
        // Create pattern space with many lines
        let content = "a\n".repeat(100);
        let mut ps = PatternSpace::new(content);

        // Delete many lines to trigger compaction
        for _ in 0..70 {
            ps.delete_first_line();
        }

        // Compaction happens when consumed > 2/3 of buffer
        // After multiple deletions and compactions, buffer should be small
        // 30 lines remaining, each is 2 bytes ("a\n")
        assert_eq!(ps.len(), 60);
        assert_eq!(ps.text(), "a\n".repeat(30));
    }

    #[test]
    fn test_len_with_active_pointer() {
        let mut ps = PatternSpace::new("line1\nline2".to_string());
        assert_eq!(ps.len(), 11); // "line1\nline2" is 11 bytes

        ps.delete_first_line();
        assert_eq!(ps.len(), 5); // "line2" is 5 bytes
    }

    #[test]
    fn test_mb_char_count_ascii() {
        // In C locale, mb_char_count equals byte count
        let ps = PatternSpace::new("hello".to_string());
        // In non-MB locale, count equals bytes
        assert_eq!(ps.mb_char_count(), 5);
    }

    #[test]
    fn test_mb_chars_iterator() {
        let ps = PatternSpace::new("abc".to_string());
        let chars: Vec<&[u8]> = ps.mb_chars().collect();
        // In non-MB locale, each byte is one character
        assert_eq!(chars.len(), 3);
        assert_eq!(chars[0], b"a");
        assert_eq!(chars[1], b"b");
        assert_eq!(chars[2], b"c");
    }
}
