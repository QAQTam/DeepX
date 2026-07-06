// Copyright (c) 2026 Red Authors
// License: MIT
//

// DFA (Deterministic Finite Automaton) compilation and execution
// Uses subset construction algorithm to convert NFA → DFA
// Provides fast O(n) matching without backtracking

use super::ast::RegexNode;
use super::nfa::{Nfa, StateId as NfaStateId, Transition};
use std::collections::{HashMap, HashSet};

/// DFA state ID
pub type DfaStateId = usize;

/// DFA transition table: (from_state, char) → to_state
pub type TransitionTable = HashMap<(DfaStateId, char), DfaStateId>;

/// Character class transition: (from_state, char_class) → to_state
/// Used for optimizing character class transitions
#[derive(Debug, Clone)]
pub struct CharClassTransition {
    pub from: DfaStateId,
    pub to: DfaStateId,
    pub chars: HashSet<char>,
}

/// DFA (Deterministic Finite Automaton)
#[derive(Debug, Clone)]
pub struct Dfa {
    /// Transition table for character transitions
    pub transitions: TransitionTable,

    /// Special transitions for Any (.) - maps state to target
    pub any_transitions: HashMap<DfaStateId, DfaStateId>,

    /// Character class transitions (optimized storage)
    pub char_class_transitions: Vec<CharClassTransition>,

    /// Start state
    pub start: DfaStateId,

    /// Accepting states
    pub accept_states: HashSet<DfaStateId>,

    /// Number of states
    pub num_states: usize,

    /// Case insensitive matching
    pub ignore_case: bool,
}

impl Dfa {
    /// Build DFA from NFA using subset construction
    pub fn from_nfa(nfa: &Nfa, ignore_case: bool) -> Self {
        let mut dfa = Dfa {
            transitions: HashMap::new(),
            any_transitions: HashMap::new(),
            char_class_transitions: Vec::new(),
            start: 0,
            accept_states: HashSet::new(),
            num_states: 0,
            ignore_case,
        };

        // Map from NFA state sets to DFA state IDs
        let mut nfa_set_to_dfa_id: HashMap<Vec<NfaStateId>, DfaStateId> = HashMap::new();

        // Work queue: DFA states to process
        let mut work_queue: Vec<(DfaStateId, HashSet<NfaStateId>)> = Vec::new();

        // Start with epsilon closure of NFA start state
        let mut start_set = HashSet::new();
        start_set.insert(nfa.start);
        let start_closure = nfa.epsilon_closure(&start_set);

        // Create DFA start state
        let start_id = dfa.add_state(&start_closure, nfa);
        let mut start_vec: Vec<_> = start_closure.iter().copied().collect();
        start_vec.sort();
        nfa_set_to_dfa_id.insert(start_vec.clone(), start_id);
        work_queue.push((start_id, start_closure));

        // Process work queue
        while let Some((dfa_state, nfa_states)) = work_queue.pop() {
            // Collect all possible transitions from this set of NFA states
            let mut char_transitions: HashMap<char, HashSet<NfaStateId>> = HashMap::new();
            let mut has_any_transition = false;
            let mut any_targets = HashSet::new();

            for &nfa_state_id in &nfa_states {
                for (transition, target) in &nfa.states[nfa_state_id].transitions {
                    match transition {
                        Transition::Char(ch) => {
                            let ch_normalized = if ignore_case {
                                // For case-insensitive, track both cases
                                ch.to_lowercase().next().unwrap_or(*ch)
                            } else {
                                *ch
                            };
                            char_transitions
                                .entry(ch_normalized)
                                .or_insert_with(HashSet::new)
                                .insert(*target);

                            // Also add uppercase version for case-insensitive
                            if ignore_case {
                                let ch_upper = ch.to_uppercase().next().unwrap_or(*ch);
                                if ch_upper != ch_normalized {
                                    char_transitions
                                        .entry(ch_upper)
                                        .or_insert_with(HashSet::new)
                                        .insert(*target);
                                }
                            }
                        }
                        Transition::Any => {
                            has_any_transition = true;
                            any_targets.insert(*target);
                        }
                        Transition::CharSet(set) => {
                            // Expand character set into individual characters
                            // This is simplified - in production, we'd optimize this
                            for range in &set.ranges {
                                for ch_code in range.0 as u32..=range.1 as u32 {
                                    if let Some(ch) = char::from_u32(ch_code) {
                                        let ch_normalized = if ignore_case {
                                            ch.to_lowercase().next().unwrap_or(ch)
                                        } else {
                                            ch
                                        };
                                        char_transitions
                                            .entry(ch_normalized)
                                            .or_insert_with(HashSet::new)
                                            .insert(*target);

                                        if ignore_case {
                                            let ch_upper = ch.to_uppercase().next().unwrap_or(ch);
                                            if ch_upper != ch_normalized {
                                                char_transitions
                                                    .entry(ch_upper)
                                                    .or_insert_with(HashSet::new)
                                                    .insert(*target);
                                            }
                                        }
                                    }
                                }
                            }
                            // POSIX character classes are handled but without DFA optimization
                        }
                        Transition::NegatedCharSet(_set) => {
                            // Should not reach here - can_use_dfa rejects negated char sets
                            unreachable!("NegatedCharSet should be handled by NFA, not DFA");
                        }
                        Transition::Epsilon => {
                            // Already handled by epsilon closure
                        }
                        Transition::StartAnchor | Transition::EndAnchor => {
                            // Anchors are handled by epsilon closure
                            // DFA can't properly handle anchors because it doesn't track position
                            // Patterns with anchors should use NFA matcher instead
                        }
                        Transition::WordBoundary
                        | Transition::NonWordBoundary
                        | Transition::StartWord
                        | Transition::EndWord => {
                            // Word boundaries require position tracking - use NFA
                        }
                        Transition::GroupStart(_) | Transition::GroupEnd(_) => {
                            // Capture groups need tracking - for DFA just treat as epsilon
                            // But patterns with backreferences must use NFA
                        }
                        Transition::Backref(_) => {
                            // Backreferences require captured text - use NFA
                        }
                    }
                }
            }

            // Process character transitions
            for (ch, targets) in char_transitions {
                let closure = nfa.epsilon_closure(&targets);
                let mut closure_vec: Vec<_> = closure.iter().copied().collect();
                closure_vec.sort();

                // Get or create DFA state for this closure
                let target_dfa_id = if let Some(&existing_id) = nfa_set_to_dfa_id.get(&closure_vec)
                {
                    existing_id
                } else {
                    let new_id = dfa.add_state(&closure, nfa);
                    nfa_set_to_dfa_id.insert(closure_vec.clone(), new_id);
                    work_queue.push((new_id, closure.clone()));
                    new_id
                };

                dfa.transitions.insert((dfa_state, ch), target_dfa_id);
            }

            // Process Any transitions
            if has_any_transition {
                let closure = nfa.epsilon_closure(&any_targets);
                let mut closure_vec: Vec<_> = closure.iter().copied().collect();
                closure_vec.sort();

                let target_dfa_id = if let Some(&existing_id) = nfa_set_to_dfa_id.get(&closure_vec)
                {
                    existing_id
                } else {
                    let new_id = dfa.add_state(&closure, nfa);
                    nfa_set_to_dfa_id.insert(closure_vec, new_id);
                    work_queue.push((new_id, closure));
                    new_id
                };

                dfa.any_transitions.insert(dfa_state, target_dfa_id);
            }
        }

        dfa
    }

    /// Add new DFA state corresponding to a set of NFA states
    fn add_state(&mut self, nfa_states: &HashSet<NfaStateId>, nfa: &Nfa) -> DfaStateId {
        let id = self.num_states;
        self.num_states += 1;

        // Check if any NFA state in this set is accepting
        for &nfa_state_id in nfa_states {
            if nfa.states[nfa_state_id].is_accept {
                self.accept_states.insert(id);
                break;
            }
        }

        id
    }

    /// Check if text matches the pattern
    pub fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// Find first match in text, returns (start, end) indices
    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        let chars: Vec<char> = text.chars().collect();

        // Try matching from each position
        for start in 0..=chars.len() {
            if let Some(end) = self.match_from(&chars, start) {
                return Some((start, end));
            }
        }

        None
    }

    /// Try to match from a specific start position
    /// Returns the end position if match succeeds
    /// Uses greedy matching - finds longest possible match
    fn match_from(&self, chars: &[char], start: usize) -> Option<usize> {
        let mut state = self.start;
        let mut pos = start;
        let mut last_accept = None;

        // Special case: if start state is accepting, we can match empty string
        if self.accept_states.contains(&state) {
            last_accept = Some(start);
        }

        while pos < chars.len() {
            let ch = chars[pos];
            let mut found_transition = false;

            // Try character transition first
            if let Some(&next_state) = self.transitions.get(&(state, ch)) {
                state = next_state;
                pos += 1;
                found_transition = true;

                // Update last accept position if in accepting state
                if self.accept_states.contains(&state) {
                    last_accept = Some(pos);
                }
            } else if let Some(&next_state) = self.any_transitions.get(&state) {
                // Try Any transition
                state = next_state;
                pos += 1;
                found_transition = true;

                if self.accept_states.contains(&state) {
                    last_accept = Some(pos);
                }
            }

            // No transition possible - stop
            if !found_transition {
                break;
            }
        }

        // Return last accepting position (greedy matching)
        last_accept
    }

    /// Find all matches in text (for global substitution)
    pub fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
        let mut matches = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut pos = 0;

        while pos < chars.len() {
            if let Some(end) = self.match_from(&chars, pos) {
                if end > pos {
                    matches.push((pos, end));
                    pos = end;
                } else {
                    // Empty match, advance by one to avoid infinite loop
                    pos += 1;
                }
            } else {
                pos += 1;
            }
        }

        matches
    }

    /// Replace first match
    pub fn replace_first(&self, text: &str, replacement: &str) -> String {
        if let Some((start, end)) = self.find(text) {
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
        let matches = self.find_all(text);
        if matches.is_empty() {
            return text.to_string();
        }

        let chars: Vec<char> = text.chars().collect();
        let mut result = String::new();
        let mut last_end = 0;

        for (start, end) in matches {
            // Add text between matches
            for &ch in &chars[last_end..start] {
                result.push(ch);
            }

            // Add replacement
            result.push_str(replacement);
            last_end = end;
        }

        // Add remaining text
        for &ch in &chars[last_end..] {
            result.push(ch);
        }

        result
    }
}

/// DFA matcher - wraps DFA with high-level interface
#[derive(Debug, Clone)]
pub struct DfaMatcher {
    dfa: Dfa,
}

impl DfaMatcher {
    /// Compile pattern into DFA
    pub fn compile(
        pattern: &str,
        is_ere: bool,
        ignore_case: bool,
        posix_mode: bool,
    ) -> crate::errors::Result<Self> {
        // Parse pattern to AST
        let compiled = if is_ere {
            super::parser::parse_ere(pattern, posix_mode)?
        } else {
            super::parser::parse_bre(pattern, posix_mode)?
        };

        // Check if pattern can be handled by DFA
        // DFA cannot handle: backreferences, anchors (for now)
        if !can_use_dfa(&compiled.ast) {
            return Err(crate::errors::SedError::parse(
                "Pattern requires NFA (backreferences not supported in DFA)".to_string(),
            ));
        }

        // Build NFA from AST
        let nfa = Nfa::from_ast(&compiled.ast);

        // Compile NFA to DFA
        let dfa = Dfa::from_nfa(&nfa, ignore_case);

        Ok(DfaMatcher { dfa })
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.dfa.is_match(text)
    }

    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        self.dfa.find(text)
    }

    pub fn replace_first(&self, text: &str, replacement: &str) -> String {
        self.dfa.replace_first(text, replacement)
    }

    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        self.dfa.replace_all(text, replacement)
    }
}

/// Check if AST can be handled by DFA (no backreferences, etc.)
fn can_use_dfa(ast: &RegexNode) -> bool {
    match ast {
        // Backreferences require NFA with backtracking
        RegexNode::Backref(_) => false,

        // Groups require NFA for capture support
        // DFA can match patterns with groups but can't capture them
        RegexNode::Group { .. } => false,

        // Anchors require position tracking - use NFA
        RegexNode::StartAnchor | RegexNode::EndAnchor => false,

        // Word boundaries also require position tracking - use NFA
        RegexNode::WordBoundary | RegexNode::NonWordBoundary => false,
        RegexNode::StartWord | RegexNode::EndWord => false,

        // Recursive checks
        RegexNode::Sequence(nodes) => nodes.iter().all(can_use_dfa),
        RegexNode::Alternation(branches) => branches.iter().all(can_use_dfa),
        RegexNode::ZeroOrMore(node) => can_use_dfa(node),
        RegexNode::OneOrMore(node) => can_use_dfa(node),
        RegexNode::ZeroOrOne(node) => can_use_dfa(node),
        RegexNode::Repeat { node, .. } => can_use_dfa(node),

        // Simple nodes are always OK
        RegexNode::Literal(_) => true,
        RegexNode::Any => true,
        RegexNode::CharClass(set) => {
            // DFA doesn't support POSIX character classes - fall back to NFA
            set.posix_classes.is_empty()
        }
        RegexNode::NegatedCharClass(_set) => {
            // DFA can't efficiently handle negated character classes
            // The complement set can be very large (all Unicode except a few chars)
            // Fall back to NFA which handles negation properly
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dfa_literal() {
        let matcher = DfaMatcher::compile("abc", false, false, false).unwrap();
        assert!(matcher.is_match("abc"));
        assert!(matcher.is_match("xxabcxx"));
        assert!(!matcher.is_match("ab"));
        assert!(!matcher.is_match("xyz"));
    }

    #[test]
    fn test_dfa_alternation() {
        let matcher = DfaMatcher::compile("a\\|b", false, false, false).unwrap();
        assert!(matcher.is_match("a"));
        assert!(matcher.is_match("b"));
        assert!(!matcher.is_match("c"));
    }

    #[test]
    fn test_dfa_star() {
        let matcher = DfaMatcher::compile("a*", false, false, false).unwrap();
        assert!(matcher.is_match(""));
        assert!(matcher.is_match("a"));
        assert!(matcher.is_match("aaa"));
        assert!(matcher.is_match("bbb")); // a* matches empty string at start
    }

    #[test]
    fn test_dfa_plus() {
        let matcher = DfaMatcher::compile("a\\+", false, false, false).unwrap();
        assert!(!matcher.is_match(""));
        assert!(matcher.is_match("a"));
        assert!(matcher.is_match("aaa"));
    }

    #[test]
    fn test_dfa_replace() {
        let matcher = DfaMatcher::compile("foo", false, false, false).unwrap();
        assert_eq!(matcher.replace_first("foo bar", "baz"), "baz bar");
        assert_eq!(matcher.replace_all("foo foo", "bar"), "bar bar");
    }

    #[test]
    fn test_negated_charset_falls_back_to_nfa() {
        // Negated character sets should fail to compile as DFA
        // and fall back to NFA which handles them correctly
        let result = DfaMatcher::compile("[^a]", false, false, false);
        assert!(result.is_err(), "DFA should reject negated character sets");
    }
}
