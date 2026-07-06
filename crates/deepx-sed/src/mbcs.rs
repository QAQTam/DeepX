// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Multibyte Character Set Support
//!
//! This module provides locale-aware multibyte character handling,
//! similar to GNU sed's mbcs.c. It uses libc functions (mbrlen, mbrtowc)
//! to properly handle stateful encodings like Shift-JIS.

use std::sync::OnceLock;

/// Global MB_CUR_MAX value, initialized once at startup
static MB_CUR_MAX: OnceLock<usize> = OnceLock::new();

/// Check if we're in a multibyte locale (MB_CUR_MAX > 1)
static IS_MULTIBYTE_LOCALE: OnceLock<bool> = OnceLock::new();

// FFI declarations for multibyte functions
#[cfg(unix)]
mod ffi {
    use std::os::raw::c_int;

    // mbstate_t is an opaque type, we'll use a fixed-size array
    // The actual size varies by platform, but 128 bytes should be enough
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MbStateT {
        _data: [u8; 128],
    }

    impl Default for MbStateT {
        fn default() -> Self {
            Self { _data: [0u8; 128] }
        }
    }

    // On Linux (glibc), MB_CUR_MAX is accessed via __ctype_get_mb_cur_max()
    // On macOS/BSD, it's a global variable ___mb_cur_max
    #[cfg(target_os = "linux")]
    extern "C" {
        #[link_name = "__ctype_get_mb_cur_max"]
        pub fn mb_cur_max() -> usize;
    }

    // On macOS, ___mb_cur_max() is a function that returns the current MB_CUR_MAX
    #[cfg(target_os = "macos")]
    extern "C" {
        #[link_name = "___mb_cur_max"]
        fn mb_cur_max_fn() -> libc::c_int;
    }

    #[cfg(target_os = "macos")]
    #[allow(unused_unsafe)]
    pub fn mb_cur_max() -> usize {
        unsafe { mb_cur_max_fn() as usize }
    }

    extern "C" {

        // Check if mbstate_t is in initial state
        pub fn mbsinit(ps: *const MbStateT) -> c_int;

        // Convert multibyte to wide character, return bytes consumed
        pub fn mbrtowc(pwc: *mut libc::wchar_t, s: *const u8, n: usize, ps: *mut MbStateT)
            -> isize;

        // Get length of multibyte character
        pub fn mbrlen(s: *const u8, n: usize, ps: *mut MbStateT) -> isize;
    }
}

/// Initialize multibyte support. Call this early in main().
/// This must be called after program startup to inherit the environment's locale.
#[cfg(unix)]
#[allow(unused_unsafe)] // On macOS ffi::mb_cur_max() is safe, on Linux it's extern "C"
pub fn initialize() {
    // First, call setlocale to inherit the environment's locale settings.
    // Without this, the C library uses the default "C" locale.
    unsafe {
        libc::setlocale(libc::LC_ALL, b"\0".as_ptr() as *const libc::c_char);
    }
    let max = unsafe { ffi::mb_cur_max() };
    MB_CUR_MAX.get_or_init(|| max);
    IS_MULTIBYTE_LOCALE.get_or_init(|| max > 1);
}

#[cfg(not(unix))]
pub fn initialize() {
    MB_CUR_MAX.get_or_init(|| 1);
    IS_MULTIBYTE_LOCALE.get_or_init(|| false);
}

/// Get the current locale's MB_CUR_MAX value
#[cfg(unix)]
#[inline]
#[allow(unused_unsafe)] // On macOS ffi::mb_cur_max() is safe, on Linux it's extern "C"
pub fn mb_cur_max() -> usize {
    *MB_CUR_MAX.get_or_init(|| unsafe { ffi::mb_cur_max() })
}

#[cfg(not(unix))]
#[inline]
pub fn mb_cur_max() -> usize {
    *MB_CUR_MAX.get_or_init(|| 1)
}

/// Check if we're in a multibyte locale
#[inline]
pub fn is_multibyte_locale() -> bool {
    *IS_MULTIBYTE_LOCALE.get_or_init(|| mb_cur_max() > 1)
}

/// Multibyte parsing state (wrapper around libc mbstate_t)
#[cfg(unix)]
#[derive(Clone)]
pub struct MbState {
    state: ffi::MbStateT,
}

#[cfg(not(unix))]
#[derive(Clone)]
pub struct MbState {
    _dummy: u8,
}

impl Default for MbState {
    fn default() -> Self {
        Self::new()
    }
}

impl MbState {
    #[cfg(unix)]
    pub fn new() -> Self {
        Self {
            state: ffi::MbStateT::default(),
        }
    }

    #[cfg(not(unix))]
    pub fn new() -> Self {
        Self { _dummy: 0 }
    }

    #[cfg(unix)]
    pub fn reset(&mut self) {
        self.state = ffi::MbStateT::default();
    }

    #[cfg(not(unix))]
    pub fn reset(&mut self) {
        self._dummy = 0;
    }

    #[cfg(unix)]
    pub fn is_initial(&self) -> bool {
        unsafe { ffi::mbsinit(&self.state) != 0 }
    }

    #[cfg(not(unix))]
    pub fn is_initial(&self) -> bool {
        true
    }

    /// Check if the given byte is part of a multibyte sequence.
    #[cfg(unix)]
    pub fn is_mb_char(&mut self, byte: u8) -> bool {
        if !is_multibyte_locale() {
            return false;
        }

        let was_pending = !self.is_initial();
        let result =
            unsafe { ffi::mbrtowc(std::ptr::null_mut(), &byte as *const u8, 1, &mut self.state) };

        match result {
            -2 => true, // Incomplete but valid multibyte sequence
            -1 => {
                self.reset();
                false // Invalid sequence
            }
            0 => true,        // NUL character
            1 => was_pending, // Valid byte, part of multibyte if we had pending
            _ => false,
        }
    }

    #[cfg(not(unix))]
    pub fn is_mb_char(&mut self, _byte: u8) -> bool {
        false
    }
}

/// Count the number of multibyte characters in a byte slice.
#[cfg(unix)]
pub fn count_mb_chars(bytes: &[u8]) -> usize {
    if !is_multibyte_locale() {
        return bytes.len();
    }

    let mut count = 0;
    let mut state = ffi::MbStateT::default();
    let mut i = 0;

    while i < bytes.len() {
        let remaining = bytes.len() - i;
        let result = unsafe { ffi::mbrlen(bytes[i..].as_ptr(), remaining, &mut state) };

        match result {
            -2 => {
                // Incomplete sequence at end
                count += remaining;
                break;
            }
            -1 => {
                // Invalid sequence - count as single byte
                count += 1;
                i += 1;
                state = ffi::MbStateT::default();
            }
            0 => {
                count += 1;
                i += 1;
            }
            n => {
                count += 1;
                i += n as usize;
            }
        }
    }

    count
}

#[cfg(not(unix))]
pub fn count_mb_chars(bytes: &[u8]) -> usize {
    bytes.len()
}

/// Iterator over multibyte characters in a byte slice.
#[cfg(unix)]
pub struct MbCharIter<'a> {
    bytes: &'a [u8],
    pos: usize,
    state: ffi::MbStateT,
}

#[cfg(not(unix))]
pub struct MbCharIter<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> MbCharIter<'a> {
    #[cfg(unix)]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            state: ffi::MbStateT::default(),
        }
    }

    #[cfg(not(unix))]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
}

impl<'a> Iterator for MbCharIter<'a> {
    type Item = &'a [u8];

    #[cfg(unix)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.bytes.len() {
            return None;
        }

        if !is_multibyte_locale() {
            let byte = &self.bytes[self.pos..self.pos + 1];
            self.pos += 1;
            return Some(byte);
        }

        let remaining = self.bytes.len() - self.pos;
        let result =
            unsafe { ffi::mbrlen(self.bytes[self.pos..].as_ptr(), remaining, &mut self.state) };

        let char_len = match result {
            -2 | -1 => {
                self.state = ffi::MbStateT::default();
                1
            }
            0 => 1,
            n => n as usize,
        };

        let start = self.pos;
        self.pos += char_len;
        Some(&self.bytes[start..self.pos])
    }

    #[cfg(not(unix))]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let byte = &self.bytes[self.pos..self.pos + 1];
        self.pos += 1;
        Some(byte)
    }
}

/// Convert a multibyte byte sequence to a wide character (for CharSet matching).
/// Returns None if the conversion fails or the sequence is invalid.
#[cfg(unix)]
pub fn mb_to_wchar(bytes: &[u8]) -> Option<char> {
    if bytes.is_empty() {
        return None;
    }

    // Single ASCII byte - fast path
    if bytes.len() == 1 && bytes[0] < 128 {
        return Some(bytes[0] as char);
    }

    // If not in MB locale, treat each byte as a character
    if !is_multibyte_locale() {
        if bytes.len() == 1 {
            return Some(bytes[0] as char);
        }
        return None;
    }

    // Use mbrtowc to convert
    let mut state = ffi::MbStateT::default();
    let mut wc: libc::wchar_t = 0;

    let result = unsafe { ffi::mbrtowc(&mut wc, bytes.as_ptr(), bytes.len(), &mut state) };

    if result > 0 && result != -1 && result != -2 {
        // Valid conversion - convert wchar_t to char if it's a valid Unicode codepoint
        // wchar_t is i32 on most platforms, cast to u32 for range check
        let wc_u32 = wc as u32;
        if wc_u32 <= 0x10FFFF {
            char::from_u32(wc_u32)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(not(unix))]
pub fn mb_to_wchar(bytes: &[u8]) -> Option<char> {
    if bytes.len() == 1 {
        Some(bytes[0] as char)
    } else {
        None
    }
}

/// Represents a multibyte character - a slice of bytes that form one logical character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbChar<'a> {
    /// The raw bytes of this character
    pub bytes: &'a [u8],
}

impl<'a> MbChar<'a> {
    /// Create a new MbChar from a byte slice
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// Get the byte length of this character
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Check if this is an empty character
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Check if this is a single ASCII byte
    pub fn is_ascii(&self) -> bool {
        self.bytes.len() == 1 && self.bytes[0] < 128
    }

    /// Try to convert to a Rust char (only works for valid UTF-8)
    pub fn to_char(&self) -> Option<char> {
        std::str::from_utf8(self.bytes).ok()?.chars().next()
    }

    /// Check if this character matches a byte
    pub fn matches_byte(&self, byte: u8) -> bool {
        self.bytes.len() == 1 && self.bytes[0] == byte
    }

    /// Convert to wide character (wchar_t) for CharSet matching
    /// Returns the character as a Rust char if conversion succeeds
    pub fn to_wchar(&self) -> Option<char> {
        mb_to_wchar(self.bytes)
    }

    /// Check if this character equals another MbChar (byte-level comparison)
    pub fn equals(&self, other: &MbChar) -> bool {
        self.bytes == other.bytes
    }

    /// Check if this character matches a Rust char (single character comparison)
    pub fn matches_char(&self, ch: char) -> bool {
        // Fast path for ASCII
        if ch.is_ascii() && self.bytes.len() == 1 {
            return self.bytes[0] == ch as u8;
        }
        // Convert to wide char and compare
        self.to_wchar() == Some(ch)
    }

    /// Check if this character matches a Rust char (case-insensitive)
    pub fn matches_char_ignore_case(&self, ch: char) -> bool {
        if let Some(wc) = self.to_wchar() {
            wc.to_lowercase().eq(ch.to_lowercase())
        } else {
            false
        }
    }

    /// Check if this character is a word character (alphanumeric or underscore)
    /// Used for word boundary matching (\b, \B, \<, \>)
    pub fn is_word_char(&self) -> bool {
        self.to_wchar()
            .map(|wc| wc.is_alphanumeric() || wc == '_')
            .unwrap_or(false)
    }
}

/// A text representation that tracks character boundaries for MBCS locales.
/// This allows efficient iteration and position conversion between byte and char indices.
#[derive(Debug, Clone)]
pub struct MbText<'a> {
    /// The raw bytes
    pub raw: &'a [u8],
    /// Character boundaries: char_boundaries[i] is the byte offset where character i starts
    /// The length is num_chars + 1, with the last entry being raw.len()
    pub char_boundaries: Vec<usize>,
    /// Validity of each character: true if valid multibyte char, false if invalid/incomplete
    pub char_validity: Vec<bool>,
}

impl<'a> MbText<'a> {
    /// Create a new MbText from raw bytes, computing character boundaries
    pub fn new(raw: &'a [u8]) -> Self {
        let (char_boundaries, char_validity) = compute_char_boundaries(raw);
        Self {
            raw,
            char_boundaries,
            char_validity,
        }
    }

    /// Create MbText from a string (uses UTF-8 char boundaries)
    pub fn from_str(s: &'a str) -> Self {
        if is_multibyte_locale() {
            // In MB locale, use locale-based character boundaries
            Self::new(s.as_bytes())
        } else {
            // In single-byte locale, use UTF-8 characters
            let raw = s.as_bytes();
            // Fix: only keep starting positions
            let mut fixed = Vec::with_capacity(s.chars().count() + 1);
            fixed.push(0);
            let mut pos = 0;
            for ch in s.chars() {
                pos += ch.len_utf8();
                fixed.push(pos);
            }
            let validity = vec![true; s.chars().count()]; // All UTF-8 chars are valid
            Self {
                raw,
                char_boundaries: fixed,
                char_validity: validity,
            }
        }
    }

    /// Get the number of characters
    pub fn char_count(&self) -> usize {
        if self.char_boundaries.is_empty() {
            0
        } else {
            self.char_boundaries.len() - 1
        }
    }

    /// Get the byte length
    pub fn byte_len(&self) -> usize {
        self.raw.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Get character at index (0-based character index)
    pub fn char_at(&self, char_idx: usize) -> Option<MbChar<'a>> {
        if char_idx >= self.char_count() {
            return None;
        }
        let start = self.char_boundaries[char_idx];
        let end = self.char_boundaries[char_idx + 1];
        Some(MbChar::new(&self.raw[start..end]))
    }

    /// Check if character at index is valid (not an incomplete/invalid MB sequence)
    pub fn is_valid_char(&self, char_idx: usize) -> bool {
        if char_idx >= self.char_validity.len() {
            false
        } else {
            self.char_validity[char_idx]
        }
    }

    /// Convert character index to byte offset
    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        if char_idx >= self.char_boundaries.len() {
            self.raw.len()
        } else {
            self.char_boundaries[char_idx]
        }
    }

    /// Convert byte offset to character index
    /// Returns the character index that contains this byte, or char_count() if at end
    pub fn byte_to_char(&self, byte_offset: usize) -> usize {
        if byte_offset >= self.raw.len() {
            return self.char_count();
        }
        // Binary search for the character containing this byte
        match self.char_boundaries.binary_search(&byte_offset) {
            Ok(idx) => idx,
            Err(idx) => {
                if idx > 0 {
                    idx - 1
                } else {
                    0
                }
            }
        }
    }

    /// Get a slice of characters from start to end (character indices)
    pub fn slice_chars(&self, start: usize, end: usize) -> &'a [u8] {
        let byte_start = self.char_to_byte(start);
        let byte_end = self.char_to_byte(end);
        &self.raw[byte_start..byte_end]
    }

    /// Get characters as an iterator
    pub fn chars(&self) -> MbTextCharIter<'a, '_> {
        MbTextCharIter { text: self, pos: 0 }
    }

    /// Get a substring from character positions as a String
    pub fn substring(&self, start: usize, end: usize) -> String {
        let bytes = self.slice_chars(start, end);
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// Get word boundary context at a position.
    ///
    /// Returns (prev_is_word, next_is_word) for use in word boundary matching:
    /// - `\b` (WordBoundary): `prev_is_word != next_is_word`
    /// - `\B` (NonWordBoundary): `prev_is_word == next_is_word`
    /// - `\<` (StartWord): `!prev_is_word && next_is_word`
    /// - `\>` (EndWord): `prev_is_word && !next_is_word`
    pub fn word_boundary_context(&self, pos: usize) -> (bool, bool) {
        let prev_is_word = if pos > 0 {
            self.char_at(pos - 1)
                .map(|ch| ch.is_word_char())
                .unwrap_or(false)
        } else {
            false
        };
        let next_is_word = self
            .char_at(pos)
            .map(|ch| ch.is_word_char())
            .unwrap_or(false);
        (prev_is_word, next_is_word)
    }
}

/// Iterator over characters in MbText
pub struct MbTextCharIter<'a, 'b> {
    text: &'b MbText<'a>,
    pos: usize,
}

impl<'a, 'b> Iterator for MbTextCharIter<'a, 'b> {
    type Item = MbChar<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.text.char_count() {
            return None;
        }
        let ch = self.text.char_at(self.pos)?;
        self.pos += 1;
        Some(ch)
    }
}

/// Compute character boundaries for a byte slice using locale's MB encoding
#[cfg(unix)]
fn compute_char_boundaries(bytes: &[u8]) -> (Vec<usize>, Vec<bool>) {
    if bytes.is_empty() {
        return (vec![0], vec![]);
    }

    let is_mb = is_multibyte_locale();

    if !is_mb {
        // Single-byte locale: each byte is a character (all valid)
        return ((0..=bytes.len()).collect(), vec![true; bytes.len()]);
    }

    let mut boundaries = Vec::with_capacity(bytes.len() + 1);
    let mut validity = Vec::with_capacity(bytes.len());
    boundaries.push(0);
    let mut state = ffi::MbStateT::default();
    let mut i = 0;

    while i < bytes.len() {
        let remaining = bytes.len() - i;
        let result = unsafe { ffi::mbrlen(bytes[i..].as_ptr(), remaining, &mut state) };

        let (char_len, valid) = match result {
            -2 | -1 => {
                // Invalid or incomplete sequence - treat as single byte, but mark invalid
                state = ffi::MbStateT::default();
                (1, false)
            }
            0 => (1, true), // NUL character is valid
            n => (n as usize, true),
        };

        validity.push(valid);
        i += char_len;
        boundaries.push(i);
    }

    (boundaries, validity)
}

#[cfg(not(unix))]
fn compute_char_boundaries(bytes: &[u8]) -> (Vec<usize>, Vec<bool>) {
    // Non-Unix: each byte is a character (all valid)
    ((0..=bytes.len()).collect(), vec![true; bytes.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mb_cur_max() {
        let max = mb_cur_max();
        assert!(max >= 1);
    }

    #[test]
    fn test_count_ascii() {
        assert_eq!(count_mb_chars(b"hello"), 5);
        assert_eq!(count_mb_chars(b""), 0);
    }

    #[test]
    fn test_mb_state() {
        let mut state = MbState::new();
        assert!(state.is_initial());
        let is_mb = state.is_mb_char(b'a');
        assert!(!is_mb || !state.is_initial());
    }

    #[test]
    fn test_mb_text_ascii() {
        let text = MbText::new(b"hello");
        assert_eq!(text.char_count(), 5);
        assert_eq!(text.byte_len(), 5);

        // Check character access
        let ch0 = text.char_at(0).unwrap();
        assert_eq!(ch0.bytes, b"h");
        assert!(ch0.is_ascii());

        // Check position conversion
        assert_eq!(text.char_to_byte(0), 0);
        assert_eq!(text.char_to_byte(2), 2);
        assert_eq!(text.byte_to_char(2), 2);
    }

    #[test]
    fn test_mb_text_slice() {
        let text = MbText::new(b"hello");
        assert_eq!(text.slice_chars(1, 4), b"ell");
        assert_eq!(text.substring(1, 4), "ell");
    }

    #[test]
    fn test_mb_text_iter() {
        let text = MbText::new(b"abc");
        let chars: Vec<_> = text.chars().collect();
        assert_eq!(chars.len(), 3);
        assert_eq!(chars[0].bytes, b"a");
        assert_eq!(chars[1].bytes, b"b");
        assert_eq!(chars[2].bytes, b"c");
    }

    #[test]
    fn test_mb_char_matches() {
        let ch = MbChar::new(b"a");
        assert!(ch.matches_byte(b'a'));
        assert!(!ch.matches_byte(b'b'));
    }
}
