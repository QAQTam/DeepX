// Copyright (c) 2026 Red Authors
// License: MIT
//

// NFA executor with backtracking for backreferences
// Implements full BRE/ERE matching with capture groups

use super::nfa::{Nfa, StateId, Transition};
use crate::constants::MAX_REGEX_BACKTRACK_ITERATIONS;
use crate::mbcs::{is_multibyte_locale, MbText};
use rustc_hash::FxHashSet;
use std::collections::HashMap;
use std::rc::Rc;

/// Check if character is a word character (alphanumeric or underscore)
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Capture group - stores matched text and raw bytes
#[derive(Debug, Clone)]
pub struct Capture {
    pub start: usize,
    pub end: usize,
    pub text: String,
    /// Raw bytes of captured text (for MBCS support)
    pub raw_bytes: Option<Vec<u8>>,
}

/// Backtracking state for NFA execution
#[derive(Debug, Clone)]
struct BacktrackState {
    /// Current NFA state
    state_id: StateId,
    /// Current position in text
    text_pos: usize,
    /// Captured groups so far (completed captures) - Rc to avoid cloning
    captures: Rc<HashMap<usize, Capture>>,
    /// Pending group starts (group_id -> start_position) - Rc to avoid cloning
    group_starts: Rc<HashMap<usize, usize>>,
}

/// NFA matcher with backtracking support
#[derive(Debug, Clone)]
pub struct NfaMatcher {
    nfa: Nfa,
    ignore_case: bool,
    has_backreferences: bool, // Cache whether NFA contains backreferences
    multiline: bool,          // Whether ^ and $ should match at line boundaries
}

impl NfaMatcher {
    /// Compile pattern into NFA matcher
    pub fn compile(pattern: &str, is_ere: bool, ignore_case: bool) -> crate::errors::Result<Self> {
        Self::compile_with_flags(pattern, is_ere, ignore_case, false, false)
    }

    /// Compile pattern into NFA matcher with multiline flag
    pub fn compile_with_flags(
        pattern: &str,
        is_ere: bool,
        ignore_case: bool,
        multiline: bool,
        posix_mode: bool,
    ) -> crate::errors::Result<Self> {
        // Parse pattern to AST
        let compiled = if is_ere {
            super::parser::parse_ere(pattern, posix_mode)?
        } else {
            super::parser::parse_bre(pattern, posix_mode)?
        };

        // Build NFA from AST
        let nfa = Nfa::from_ast(&compiled.ast);

        // Check if NFA contains backreferences (cache for performance)
        let has_backreferences = nfa.states.iter().any(|s| {
            s.transitions
                .iter()
                .any(|(t, _)| matches!(t, Transition::Backref(_)))
        });

        Ok(NfaMatcher {
            nfa,
            ignore_case,
            has_backreferences,
            multiline,
        })
    }

    /// Check if text matches pattern
    pub fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// Find first match in text
    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        self.find_with_captures(text)
            .map(|(start, end, _)| (start, end))
    }

    /// Find first match and return captures (public interface)
    pub fn find_with_captures_pub(
        &self,
        text: &str,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        self.find_with_captures(text)
    }

    /// Find first match and return captures
    fn find_with_captures(&self, text: &str) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        self.find_with_captures_from(text, 0)
    }

    /// Find first match starting from a specific position
    pub fn find_with_captures_from(
        &self,
        text: &str,
        start_from: usize,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        // Use MBCS-aware matching in multibyte locales
        if is_multibyte_locale() {
            return self.find_with_captures_from_mb(text.as_bytes(), start_from);
        }

        let chars: Vec<char> = text.chars().collect();

        // Try matching from each position starting from start_from
        for start in start_from..=chars.len() {
            if let Some((end, captures)) = self.match_from(&chars, start) {
                return Some((start, end, captures));
            }
        }

        None
    }

    /// MBCS-aware find with captures - works with raw bytes and locale encoding
    fn find_with_captures_from_mb(
        &self,
        bytes: &[u8],
        start_from: usize,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        let mb_text = MbText::new(bytes);

        // Try matching from each character position
        for start in start_from..=mb_text.char_count() {
            if let Some((end, captures)) = self.match_from_mb(&mb_text, start) {
                return Some((start, end, captures));
            }
        }

        None
    }

    /// Find first match starting from a specific position (without captures)
    pub fn find_from(&self, text: &str, start_from: usize) -> Option<(usize, usize)> {
        self.find_with_captures_from(text, start_from)
            .map(|(start, end, _)| (start, end))
    }

    /// Find match in raw bytes (for MBCS locales)
    /// Returns (start_byte, end_byte, captures) where start/end are byte offsets
    pub fn find_with_captures_bytes(
        &self,
        bytes: &[u8],
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        self.find_with_captures_bytes_from(bytes, 0)
    }

    /// Find match in raw bytes starting from a byte offset
    pub fn find_with_captures_bytes_from(
        &self,
        bytes: &[u8],
        start_byte: usize,
    ) -> Option<(usize, usize, HashMap<usize, Capture>)> {
        let mb_text = MbText::new(bytes);

        // Convert start byte offset to character position
        let start_char = mb_text.byte_to_char(start_byte);

        // Try matching from each character position
        for start in start_char..=mb_text.char_count() {
            if let Some((end_char, captures)) = self.match_from_mb(&mb_text, start) {
                // Convert character positions back to byte offsets
                let start_byte = mb_text.char_to_byte(start);
                let end_byte = mb_text.char_to_byte(end_char);
                return Some((start_byte, end_byte, captures));
            }
        }

        None
    }

    /// Try to match from a specific position using backtracking
    /// Uses greedy matching - finds longest match
    fn match_from(&self, chars: &[char], start: usize) -> Option<(usize, HashMap<usize, Capture>)> {
        // Use optimized path for patterns without backreferences
        if !self.has_backreferences {
            return self.match_from_simple(chars, start);
        }

        // Initialize backtracking stack with start state
        let mut stack = vec![BacktrackState {
            state_id: self.nfa.start,
            text_pos: start,
            captures: Rc::new(HashMap::new()),
            group_starts: Rc::new(HashMap::new()),
        }];

        // Track visited states - include captures for backreference patterns
        let mut visited: FxHashSet<(StateId, usize, Vec<(usize, usize, usize)>)> =
            FxHashSet::default();

        // Track best match found so far (greedy)
        let mut best_match: Option<(usize, HashMap<usize, Capture>)> = None;

        // Safety limit to prevent infinite loops
        let mut iterations = 0;

        while let Some(state) = stack.pop() {
            iterations += 1;
            if iterations > MAX_REGEX_BACKTRACK_ITERATIONS {
                return best_match;
            }

            // Include capture positions in key for backreference patterns
            let captures_key: Vec<(usize, usize, usize)> = state
                .captures
                .iter()
                .map(|(id, cap)| (*id, cap.start, cap.end))
                .collect();

            let key = (state.state_id, state.text_pos, captures_key);
            if !visited.insert(key) {
                continue;
            }

            // Check if current state is accepting
            if self.nfa.states[state.state_id].is_accept {
                // Update best match if this is longer (greedy)
                if best_match.is_none() || best_match.as_ref().unwrap().0 < state.text_pos {
                    // Clone the inner HashMap when storing best match
                    best_match = Some((state.text_pos, (*state.captures).clone()));
                }
                // Continue searching for longer matches
            }

            // Try all transitions from current state
            for (transition, target_id) in &self.nfa.states[state.state_id].transitions {
                match transition {
                    Transition::Epsilon => {
                        // Epsilon transition - no input consumed
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: state.group_starts.clone(),
                        });
                    }

                    Transition::Char(ch) => {
                        // Check if we have a character to match
                        if state.text_pos < chars.len() {
                            let text_ch = chars[state.text_pos];
                            let matches = if self.ignore_case {
                                text_ch.to_lowercase().eq(ch.to_lowercase())
                            } else {
                                text_ch == *ch
                            };

                            if matches {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }

                    Transition::Any => {
                        // Match any character
                        if state.text_pos < chars.len() {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos + 1,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::CharSet(set) => {
                        if state.text_pos < chars.len() {
                            let text_ch = chars[state.text_pos];
                            let ch_to_match = if self.ignore_case {
                                text_ch.to_lowercase().next().unwrap_or(text_ch)
                            } else {
                                text_ch
                            };

                            if set.matches(ch_to_match) {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }

                    Transition::NegatedCharSet(set) => {
                        if state.text_pos < chars.len() {
                            let text_ch = chars[state.text_pos];
                            let ch_to_match = if self.ignore_case {
                                text_ch.to_lowercase().next().unwrap_or(text_ch)
                            } else {
                                text_ch
                            };

                            if !set.matches(ch_to_match) {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }

                    Transition::StartAnchor => {
                        // ^ behavior depends on multiline flag:
                        // - Without multiline: matches ONLY at start of text (position 0)
                        // - With multiline (m/M flag): matches at start of text AND after newlines
                        let matches = if self.multiline {
                            // Multiline mode: match at start or after \n
                            state.text_pos == 0
                                || (state.text_pos > 0 && chars[state.text_pos - 1] == '\n')
                        } else {
                            // Normal mode: match only at start
                            state.text_pos == 0
                        };

                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::EndAnchor => {
                        // $ behavior depends on multiline flag:
                        // - Without multiline: matches ONLY at end of text
                        // - With multiline (m/M flag): matches at end of text AND before newlines
                        let matches = if self.multiline {
                            // Multiline mode: match at end or before \n
                            state.text_pos == chars.len()
                                || (state.text_pos < chars.len() && chars[state.text_pos] == '\n')
                        } else {
                            // Normal mode: match only at end
                            state.text_pos == chars.len()
                        };
                        let at_end = matches;

                        if at_end {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::WordBoundary => {
                        // \b matches at word/non-word boundary
                        let prev_is_word = if state.text_pos > 0 {
                            is_word_char(chars[state.text_pos - 1])
                        } else {
                            false
                        };

                        let next_is_word = if state.text_pos < chars.len() {
                            is_word_char(chars[state.text_pos])
                        } else {
                            false
                        };

                        // Boundary exists if exactly one side is a word character
                        let at_boundary = prev_is_word != next_is_word;

                        if at_boundary {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::NonWordBoundary => {
                        // \B matches where \b doesn't match
                        let prev_is_word = if state.text_pos > 0 {
                            is_word_char(chars[state.text_pos - 1])
                        } else {
                            false
                        };

                        let next_is_word = if state.text_pos < chars.len() {
                            is_word_char(chars[state.text_pos])
                        } else {
                            false
                        };

                        // Non-boundary: both sides are word or both are non-word
                        let not_at_boundary = prev_is_word == next_is_word;

                        if not_at_boundary {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::StartWord => {
                        // \< matches at start of word
                        let prev_is_word = if state.text_pos > 0 {
                            is_word_char(chars[state.text_pos - 1])
                        } else {
                            false
                        };

                        let next_is_word = if state.text_pos < chars.len() {
                            is_word_char(chars[state.text_pos])
                        } else {
                            false
                        };

                        // Start of word: previous is non-word, next is word
                        let at_word_start = !prev_is_word && next_is_word;

                        if at_word_start {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::EndWord => {
                        // \> matches at end of word
                        let prev_is_word = if state.text_pos > 0 {
                            is_word_char(chars[state.text_pos - 1])
                        } else {
                            false
                        };

                        let next_is_word = if state.text_pos < chars.len() {
                            is_word_char(chars[state.text_pos])
                        } else {
                            false
                        };

                        // End of word: previous is word, next is non-word
                        let at_word_end = prev_is_word && !next_is_word;

                        if at_word_end {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::GroupStart(group_id) => {
                        // Mark start of capture group - clone inner HashMap and wrap in new Rc
                        let mut new_group_starts = (*state.group_starts).clone();
                        new_group_starts.insert(*group_id, state.text_pos);

                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: Rc::new(new_group_starts),
                        });
                    }

                    Transition::GroupEnd(group_id) => {
                        // Complete capture group
                        if let Some(&start_pos) = state.group_starts.get(group_id) {
                            let captured_text: String =
                                chars[start_pos..state.text_pos].iter().collect();

                            // Clone inner HashMap and wrap in new Rc
                            let mut new_captures = (*state.captures).clone();
                            new_captures.insert(
                                *group_id,
                                Capture {
                                    start: start_pos,
                                    end: state.text_pos,
                                    text: captured_text,
                                    raw_bytes: None, // Non-MBCS path
                                },
                            );

                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: Rc::new(new_captures),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::Backref(group_id) => {
                        // Match same text as captured group
                        if let Some(capture) = state.captures.get(group_id) {
                            let ref_chars: Vec<char> = capture.text.chars().collect();

                            // Check if we have enough characters
                            if state.text_pos + ref_chars.len() <= chars.len() {
                                // Compare character by character
                                let matches = ref_chars.iter().enumerate().all(|(i, &ch)| {
                                    let text_ch = chars[state.text_pos + i];
                                    if self.ignore_case {
                                        text_ch.to_lowercase().eq(ch.to_lowercase())
                                    } else {
                                        text_ch == ch
                                    }
                                });

                                if matches {
                                    stack.push(BacktrackState {
                                        state_id: *target_id,
                                        text_pos: state.text_pos + ref_chars.len(),
                                        captures: state.captures.clone(),
                                        group_starts: state.group_starts.clone(),
                                    });
                                }
                            }
                        }
                        // If group not captured yet, backreference fails (no push to stack)
                    }
                }
            }
        }

        best_match
    }

    /// Optimized match_from for patterns WITHOUT backreferences
    /// Uses simpler visited set for better performance
    fn match_from_simple(
        &self,
        chars: &[char],
        start: usize,
    ) -> Option<(usize, HashMap<usize, Capture>)> {
        let mut stack = vec![BacktrackState {
            state_id: self.nfa.start,
            text_pos: start,
            captures: Rc::new(HashMap::new()),
            group_starts: Rc::new(HashMap::new()),
        }];

        // Simple visited set - no captures in key since no backreferences
        let mut visited: FxHashSet<(StateId, usize)> = FxHashSet::default();
        let mut best_match: Option<(usize, HashMap<usize, Capture>)> = None;
        let mut iterations = 0;

        while let Some(state) = stack.pop() {
            iterations += 1;
            if iterations > MAX_REGEX_BACKTRACK_ITERATIONS {
                return best_match;
            }

            // Simple key - just state and position
            if !visited.insert((state.state_id, state.text_pos)) {
                continue;
            }

            if self.nfa.states[state.state_id].is_accept {
                if best_match.is_none() || best_match.as_ref().unwrap().0 < state.text_pos {
                    best_match = Some((state.text_pos, (*state.captures).clone()));
                }
            }

            for (transition, target_id) in &self.nfa.states[state.state_id].transitions {
                match transition {
                    Transition::Epsilon => {
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: state.group_starts.clone(),
                        });
                    }
                    Transition::Char(ch) => {
                        if state.text_pos < chars.len() {
                            let text_ch = chars[state.text_pos];
                            let matches = if self.ignore_case {
                                text_ch.to_lowercase().eq(ch.to_lowercase())
                            } else {
                                text_ch == *ch
                            };
                            if matches {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }
                    Transition::Any => {
                        if state.text_pos < chars.len() {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos + 1,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::CharSet(set) => {
                        if state.text_pos < chars.len() {
                            let text_ch = chars[state.text_pos];
                            let ch_to_match = if self.ignore_case {
                                text_ch.to_lowercase().next().unwrap_or(text_ch)
                            } else {
                                text_ch
                            };
                            if set.matches(ch_to_match) {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }
                    Transition::NegatedCharSet(set) => {
                        if state.text_pos < chars.len() {
                            let text_ch = chars[state.text_pos];
                            let ch_to_match = if self.ignore_case {
                                text_ch.to_lowercase().next().unwrap_or(text_ch)
                            } else {
                                text_ch
                            };
                            if !set.matches(ch_to_match) {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }
                    Transition::StartAnchor => {
                        let matches = if self.multiline {
                            state.text_pos == 0
                                || (state.text_pos > 0 && chars[state.text_pos - 1] == '\n')
                        } else {
                            state.text_pos == 0
                        };
                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::EndAnchor => {
                        let matches = if self.multiline {
                            state.text_pos == chars.len()
                                || (state.text_pos < chars.len() && chars[state.text_pos] == '\n')
                        } else {
                            state.text_pos == chars.len()
                        };
                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::WordBoundary => {
                        let prev_is_word =
                            state.text_pos > 0 && is_word_char(chars[state.text_pos - 1]);
                        let next_is_word =
                            state.text_pos < chars.len() && is_word_char(chars[state.text_pos]);
                        if prev_is_word != next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::NonWordBoundary => {
                        let prev_is_word =
                            state.text_pos > 0 && is_word_char(chars[state.text_pos - 1]);
                        let next_is_word =
                            state.text_pos < chars.len() && is_word_char(chars[state.text_pos]);
                        if prev_is_word == next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::StartWord => {
                        let prev_is_word =
                            state.text_pos > 0 && is_word_char(chars[state.text_pos - 1]);
                        let next_is_word =
                            state.text_pos < chars.len() && is_word_char(chars[state.text_pos]);
                        if !prev_is_word && next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::EndWord => {
                        let prev_is_word =
                            state.text_pos > 0 && is_word_char(chars[state.text_pos - 1]);
                        let next_is_word =
                            state.text_pos < chars.len() && is_word_char(chars[state.text_pos]);
                        if prev_is_word && !next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::GroupStart(group_id) => {
                        let mut new_group_starts = (*state.group_starts).clone();
                        new_group_starts.insert(*group_id, state.text_pos);
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: Rc::new(new_group_starts),
                        });
                    }
                    Transition::GroupEnd(group_id) => {
                        if let Some(&start_pos) = state.group_starts.get(group_id) {
                            let captured_text: String =
                                chars[start_pos..state.text_pos].iter().collect();
                            let mut new_captures = (*state.captures).clone();
                            new_captures.insert(
                                *group_id,
                                Capture {
                                    start: start_pos,
                                    end: state.text_pos,
                                    text: captured_text,
                                    raw_bytes: None, // Non-MBCS path
                                },
                            );
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: Rc::new(new_captures),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::Backref(_) => {
                        // Should never be reached in simple path
                    }
                }
            }
        }
        best_match
    }

    /// MBCS-aware matching from a specific character position
    /// Uses MbText for proper character boundary handling in non-UTF-8 locales
    fn match_from_mb(
        &self,
        mb_text: &MbText,
        start: usize,
    ) -> Option<(usize, HashMap<usize, Capture>)> {
        // Use simplified path for patterns without backreferences
        if !self.has_backreferences {
            return self.match_from_mb_simple(mb_text, start);
        }

        let mut stack = vec![BacktrackState {
            state_id: self.nfa.start,
            text_pos: start,
            captures: Rc::new(HashMap::new()),
            group_starts: Rc::new(HashMap::new()),
        }];

        let mut visited: FxHashSet<(StateId, usize, Vec<(usize, usize, usize)>)> =
            FxHashSet::default();
        let mut best_match: Option<(usize, HashMap<usize, Capture>)> = None;
        let mut iterations = 0;

        while let Some(state) = stack.pop() {
            iterations += 1;
            if iterations > MAX_REGEX_BACKTRACK_ITERATIONS {
                return best_match;
            }

            let captures_key: Vec<(usize, usize, usize)> = state
                .captures
                .iter()
                .map(|(id, cap)| (*id, cap.start, cap.end))
                .collect();

            let key = (state.state_id, state.text_pos, captures_key);
            if !visited.insert(key) {
                continue;
            }

            if self.nfa.states[state.state_id].is_accept {
                if best_match.is_none() || best_match.as_ref().unwrap().0 < state.text_pos {
                    best_match = Some((state.text_pos, (*state.captures).clone()));
                }
            }

            for (transition, target_id) in &self.nfa.states[state.state_id].transitions {
                match transition {
                    Transition::Epsilon => {
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: state.group_starts.clone(),
                        });
                    }

                    Transition::Char(ch) => {
                        if let Some(mb_ch) = mb_text.char_at(state.text_pos) {
                            let matches = if self.ignore_case {
                                mb_ch.matches_char_ignore_case(*ch)
                            } else {
                                mb_ch.matches_char(*ch)
                            };
                            if matches {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }

                    Transition::Any => {
                        // Only match valid characters (skip invalid/incomplete MB sequences)
                        if state.text_pos < mb_text.char_count()
                            && mb_text.is_valid_char(state.text_pos)
                        {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos + 1,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::CharSet(set) => {
                        if let Some(mb_ch) = mb_text.char_at(state.text_pos) {
                            if let Some(wc) = mb_ch.to_wchar() {
                                let ch_to_match = if self.ignore_case {
                                    wc.to_lowercase().next().unwrap_or(wc)
                                } else {
                                    wc
                                };
                                if set.matches(ch_to_match) {
                                    stack.push(BacktrackState {
                                        state_id: *target_id,
                                        text_pos: state.text_pos + 1,
                                        captures: state.captures.clone(),
                                        group_starts: state.group_starts.clone(),
                                    });
                                }
                            }
                        }
                    }

                    Transition::NegatedCharSet(set) => {
                        if let Some(mb_ch) = mb_text.char_at(state.text_pos) {
                            if let Some(wc) = mb_ch.to_wchar() {
                                let ch_to_match = if self.ignore_case {
                                    wc.to_lowercase().next().unwrap_or(wc)
                                } else {
                                    wc
                                };
                                if !set.matches(ch_to_match) {
                                    stack.push(BacktrackState {
                                        state_id: *target_id,
                                        text_pos: state.text_pos + 1,
                                        captures: state.captures.clone(),
                                        group_starts: state.group_starts.clone(),
                                    });
                                }
                            } else {
                                // Invalid MB sequence - treat as non-matching any char
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }

                    Transition::StartAnchor => {
                        let matches = if self.multiline {
                            state.text_pos == 0 || {
                                if let Some(prev_ch) =
                                    mb_text.char_at(state.text_pos.saturating_sub(1))
                                {
                                    prev_ch.matches_byte(b'\n')
                                } else {
                                    false
                                }
                            }
                        } else {
                            state.text_pos == 0
                        };
                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::EndAnchor => {
                        let matches = if self.multiline {
                            state.text_pos == mb_text.char_count() || {
                                if let Some(curr_ch) = mb_text.char_at(state.text_pos) {
                                    curr_ch.matches_byte(b'\n')
                                } else {
                                    false
                                }
                            }
                        } else {
                            state.text_pos == mb_text.char_count()
                        };
                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::WordBoundary => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if prev_is_word != next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::NonWordBoundary => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if prev_is_word == next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::StartWord => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if !prev_is_word && next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::EndWord => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if prev_is_word && !next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::GroupStart(group_id) => {
                        let mut new_group_starts = (*state.group_starts).clone();
                        new_group_starts.insert(*group_id, state.text_pos);
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: Rc::new(new_group_starts),
                        });
                    }

                    Transition::GroupEnd(group_id) => {
                        if let Some(&start_pos) = state.group_starts.get(group_id) {
                            // Store raw bytes for MBCS mode
                            let raw_bytes = mb_text.slice_chars(start_pos, state.text_pos).to_vec();
                            let captured_text = String::from_utf8_lossy(&raw_bytes).into_owned();
                            let mut new_captures = (*state.captures).clone();
                            new_captures.insert(
                                *group_id,
                                Capture {
                                    start: start_pos,
                                    end: state.text_pos,
                                    text: captured_text,
                                    raw_bytes: Some(raw_bytes),
                                },
                            );
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: Rc::new(new_captures),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }

                    Transition::Backref(group_id) => {
                        if let Some(capture) = state.captures.get(group_id) {
                            // Get the captured bytes (prefer raw_bytes if available)
                            let ref_bytes = capture
                                .raw_bytes
                                .as_ref()
                                .map(|b| b.as_slice())
                                .unwrap_or_else(|| capture.text.as_bytes());
                            let ref_mb = MbText::new(ref_bytes);
                            let ref_len = ref_mb.char_count();

                            if state.text_pos + ref_len <= mb_text.char_count() {
                                // Compare character by character
                                let mut matches = true;
                                for i in 0..ref_len {
                                    if let (Some(ref_ch), Some(text_ch)) =
                                        (ref_mb.char_at(i), mb_text.char_at(state.text_pos + i))
                                    {
                                        let ch_matches = if self.ignore_case {
                                            if let (Some(wc1), Some(wc2)) =
                                                (ref_ch.to_wchar(), text_ch.to_wchar())
                                            {
                                                wc1.to_lowercase().eq(wc2.to_lowercase())
                                            } else {
                                                ref_ch.bytes == text_ch.bytes
                                            }
                                        } else {
                                            ref_ch.bytes == text_ch.bytes
                                        };
                                        if !ch_matches {
                                            matches = false;
                                            break;
                                        }
                                    } else {
                                        matches = false;
                                        break;
                                    }
                                }

                                if matches {
                                    stack.push(BacktrackState {
                                        state_id: *target_id,
                                        text_pos: state.text_pos + ref_len,
                                        captures: state.captures.clone(),
                                        group_starts: state.group_starts.clone(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        best_match
    }

    /// Simplified MBCS-aware matching (no backreferences)
    fn match_from_mb_simple(
        &self,
        mb_text: &MbText,
        start: usize,
    ) -> Option<(usize, HashMap<usize, Capture>)> {
        let mut stack = vec![BacktrackState {
            state_id: self.nfa.start,
            text_pos: start,
            captures: Rc::new(HashMap::new()),
            group_starts: Rc::new(HashMap::new()),
        }];

        let mut visited: FxHashSet<(StateId, usize)> = FxHashSet::default();
        let mut best_match: Option<(usize, HashMap<usize, Capture>)> = None;
        let mut iterations = 0;

        while let Some(state) = stack.pop() {
            iterations += 1;
            if iterations > MAX_REGEX_BACKTRACK_ITERATIONS {
                return best_match;
            }

            if !visited.insert((state.state_id, state.text_pos)) {
                continue;
            }

            if self.nfa.states[state.state_id].is_accept {
                if best_match.is_none() || best_match.as_ref().unwrap().0 < state.text_pos {
                    best_match = Some((state.text_pos, (*state.captures).clone()));
                }
            }

            for (transition, target_id) in &self.nfa.states[state.state_id].transitions {
                match transition {
                    Transition::Epsilon => {
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: state.group_starts.clone(),
                        });
                    }
                    Transition::Char(ch) => {
                        if let Some(mb_ch) = mb_text.char_at(state.text_pos) {
                            let matches = if self.ignore_case {
                                mb_ch.matches_char_ignore_case(*ch)
                            } else {
                                mb_ch.matches_char(*ch)
                            };
                            if matches {
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }
                    Transition::Any => {
                        // Only match valid characters (skip invalid/incomplete MB sequences)
                        if state.text_pos < mb_text.char_count()
                            && mb_text.is_valid_char(state.text_pos)
                        {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos + 1,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::CharSet(set) => {
                        if let Some(mb_ch) = mb_text.char_at(state.text_pos) {
                            if let Some(wc) = mb_ch.to_wchar() {
                                let ch_to_match = if self.ignore_case {
                                    wc.to_lowercase().next().unwrap_or(wc)
                                } else {
                                    wc
                                };
                                if set.matches(ch_to_match) {
                                    stack.push(BacktrackState {
                                        state_id: *target_id,
                                        text_pos: state.text_pos + 1,
                                        captures: state.captures.clone(),
                                        group_starts: state.group_starts.clone(),
                                    });
                                }
                            }
                        }
                    }
                    Transition::NegatedCharSet(set) => {
                        if let Some(mb_ch) = mb_text.char_at(state.text_pos) {
                            if let Some(wc) = mb_ch.to_wchar() {
                                let ch_to_match = if self.ignore_case {
                                    wc.to_lowercase().next().unwrap_or(wc)
                                } else {
                                    wc
                                };
                                if !set.matches(ch_to_match) {
                                    stack.push(BacktrackState {
                                        state_id: *target_id,
                                        text_pos: state.text_pos + 1,
                                        captures: state.captures.clone(),
                                        group_starts: state.group_starts.clone(),
                                    });
                                }
                            } else {
                                // Invalid MB - treat as matching negated set
                                stack.push(BacktrackState {
                                    state_id: *target_id,
                                    text_pos: state.text_pos + 1,
                                    captures: state.captures.clone(),
                                    group_starts: state.group_starts.clone(),
                                });
                            }
                        }
                    }
                    Transition::StartAnchor => {
                        let matches = if self.multiline {
                            state.text_pos == 0 || {
                                mb_text
                                    .char_at(state.text_pos.saturating_sub(1))
                                    .map(|ch| ch.matches_byte(b'\n'))
                                    .unwrap_or(false)
                            }
                        } else {
                            state.text_pos == 0
                        };
                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::EndAnchor => {
                        let matches = if self.multiline {
                            state.text_pos == mb_text.char_count() || {
                                mb_text
                                    .char_at(state.text_pos)
                                    .map(|ch| ch.matches_byte(b'\n'))
                                    .unwrap_or(false)
                            }
                        } else {
                            state.text_pos == mb_text.char_count()
                        };
                        if matches {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::WordBoundary => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if prev_is_word != next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::NonWordBoundary => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if prev_is_word == next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::StartWord => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if !prev_is_word && next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::EndWord => {
                        let (prev_is_word, next_is_word) =
                            mb_text.word_boundary_context(state.text_pos);
                        if prev_is_word && !next_is_word {
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: state.captures.clone(),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::GroupStart(group_id) => {
                        let mut new_group_starts = (*state.group_starts).clone();
                        new_group_starts.insert(*group_id, state.text_pos);
                        stack.push(BacktrackState {
                            state_id: *target_id,
                            text_pos: state.text_pos,
                            captures: state.captures.clone(),
                            group_starts: Rc::new(new_group_starts),
                        });
                    }
                    Transition::GroupEnd(group_id) => {
                        if let Some(&start_pos) = state.group_starts.get(group_id) {
                            // Store raw bytes for MBCS mode
                            let raw_bytes = mb_text.slice_chars(start_pos, state.text_pos).to_vec();
                            let captured_text = String::from_utf8_lossy(&raw_bytes).into_owned();
                            let mut new_captures = (*state.captures).clone();
                            new_captures.insert(
                                *group_id,
                                Capture {
                                    start: start_pos,
                                    end: state.text_pos,
                                    text: captured_text,
                                    raw_bytes: Some(raw_bytes),
                                },
                            );
                            stack.push(BacktrackState {
                                state_id: *target_id,
                                text_pos: state.text_pos,
                                captures: Rc::new(new_captures),
                                group_starts: state.group_starts.clone(),
                            });
                        }
                    }
                    Transition::Backref(_) => {
                        // Should never be reached in simple path
                    }
                }
            }
        }
        best_match
    }

    /// Replace first match
    pub fn replace_first(&self, text: &str, replacement: &str) -> String {
        if let Some((start, end, _captures)) = self.find_with_captures(text) {
            let chars: Vec<char> = text.chars().collect();
            let mut result = String::new();

            // Add text before match
            for &ch in &chars[..start] {
                result.push(ch);
            }

            // Add replacement
            result.push_str(replacement);

            // Add text after match
            for &ch in &chars[end..] {
                result.push(ch);
            }

            result
        } else {
            text.to_string()
        }
    }

    /// Replace all matches
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut result = String::new();
        let mut pos = 0;

        while pos < chars.len() {
            if let Some((end, _captures)) = self.match_from(&chars, pos) {
                if end > pos {
                    // Found a match
                    result.push_str(replacement);
                    pos = end;
                } else {
                    // Empty match - advance by one
                    result.push(chars[pos]);
                    pos += 1;
                }
            } else {
                // No match - copy character
                result.push(chars[pos]);
                pos += 1;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfa_literal() {
        let matcher = NfaMatcher::compile("abc", false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(matcher.is_match("xabcy"));
        assert!(!matcher.is_match("ab"));
    }

    #[test]
    fn test_nfa_star() {
        let matcher = NfaMatcher::compile("a*b", false, false).unwrap();
        assert!(matcher.is_match("b"));
        assert!(matcher.is_match("ab"));
        assert!(matcher.is_match("aaab"));
    }

    #[test]
    fn test_nfa_alternation() {
        let matcher = NfaMatcher::compile("a\\|b", false, false).unwrap();
        assert!(matcher.is_match("a"));
        assert!(matcher.is_match("b"));
        assert!(!matcher.is_match("c"));
    }

    #[test]
    fn test_nfa_replace() {
        let matcher = NfaMatcher::compile("a\\+", false, false).unwrap();
        assert_eq!(matcher.replace_first("aaa", "X"), "X");
        assert_eq!(matcher.replace_all("aaa aaa", "X"), "X X");
    }

    #[test]
    fn test_nfa_case_insensitive() {
        let matcher = NfaMatcher::compile("abc", false, true).unwrap();
        assert!(matcher.is_match("ABC"));
        assert!(matcher.is_match("aBc"));
        assert!(matcher.is_match("abc"));
    }

    #[test]
    fn test_nfa_start_anchor() {
        let matcher = NfaMatcher::compile("^abc", false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(matcher.is_match("abcdef"));
        assert!(!matcher.is_match("xabc"));

        // In BRE/ERE, ^ matches ONLY at start of text (position 0), NOT after embedded newlines
        // Note: sed's line-oriented processing makes it SEEM like ^ matches after newlines,
        // but that's because sed processes each line separately.
        assert!(!matcher.is_match("x\nabc")); // Does NOT match - "abc" is not at start
        assert!(!matcher.is_match("\nabc")); // Does NOT match - "abc" is after newline, not at position 0
    }

    #[test]
    fn test_nfa_end_anchor() {
        let matcher = NfaMatcher::compile("abc$", false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(matcher.is_match("xabc"));
        assert!(!matcher.is_match("abcx"));

        // In BRE/ERE, $ matches ONLY at end of text, NOT before embedded newlines
        // sed appears to match before newlines because it strips trailing newlines during line processing
        assert!(!matcher.is_match("abc\n")); // Does NOT match - "abc" is not at end (newline is at end)
    }

    #[test]
    fn test_nfa_both_anchors() {
        let matcher = NfaMatcher::compile("^abc$", false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(!matcher.is_match("xabc"));
        assert!(!matcher.is_match("abcx"));
        assert!(!matcher.is_match("xabcx"));
    }

    #[test]
    fn test_nfa_anchor_replace() {
        let matcher = NfaMatcher::compile("^abc", false, false).unwrap();
        assert_eq!(matcher.replace_first("abc", "X"), "X");
        assert_eq!(matcher.replace_first("xabc", "X"), "xabc"); // No match

        let matcher2 = NfaMatcher::compile("abc$", false, false).unwrap();
        assert_eq!(matcher2.replace_first("abc", "X"), "X");
        assert_eq!(matcher2.replace_first("abcx", "X"), "abcx"); // No match
    }

    #[test]
    fn test_nfa_word_boundary() {
        // \b matches at word boundaries
        let matcher = NfaMatcher::compile("\\bword\\b", false, false).unwrap();
        assert!(matcher.is_match("word"));
        assert!(matcher.is_match("a word b"));
        assert!(matcher.is_match("word!"));
        assert!(!matcher.is_match("sword"));
        assert!(!matcher.is_match("wording"));
    }

    #[test]
    fn test_nfa_start_word() {
        // \< matches at start of word
        let matcher = NfaMatcher::compile("\\<abc", false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(matcher.is_match("abc def"));
        assert!(matcher.is_match(" abc"));
        assert!(!matcher.is_match("xabc"));
        assert!(!matcher.is_match("123abc"));
    }

    #[test]
    fn test_nfa_end_word() {
        // \> matches at end of word
        let matcher = NfaMatcher::compile("abc\\>", false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(matcher.is_match("abc "));
        assert!(matcher.is_match("abc!"));
        assert!(!matcher.is_match("abcd"));
        assert!(!matcher.is_match("abc123"));
    }

    #[test]
    fn test_nfa_word_boundary_replace() {
        let matcher = NfaMatcher::compile("\\bcat\\b", false, false).unwrap();
        assert_eq!(matcher.replace_first("cat", "dog"), "dog");
        assert_eq!(matcher.replace_first("the cat sat", "dog"), "the dog sat");
        assert_eq!(matcher.replace_first("category", "dog"), "category"); // No match
    }

    #[test]
    fn test_nfa_posix_char_class_greedy() {
        // Test POSIX character classes with greedy quantifiers
        let matcher = NfaMatcher::compile("[[:alpha:]]*", false, false).unwrap();
        assert!(matcher.is_match("abc"), "Should match 'abc'");

        // Find should return longest match
        if let Some((start, end)) = matcher.find("abc") {
            assert_eq!(start, 0, "Match should start at 0");
            assert_eq!(end, 3, "Match should end at 3 (greedy - all chars)");
        } else {
            panic!("Should find a match");
        }

        // Replace should replace all matched chars
        assert_eq!(
            matcher.replace_first("abc", "X"),
            "X",
            "Should replace all alpha chars"
        );
        assert_eq!(
            matcher.replace_first("abc123", "X"),
            "X123",
            "Should replace only alpha prefix"
        );
    }
}
