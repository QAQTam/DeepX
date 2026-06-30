// Copyright (c) 2026 Red Authors
// License: MIT
//

// NFA (Non-deterministic Finite Automaton) construction using Thompson's algorithm
// Converts AST → NFA for subsequent DFA compilation

use super::ast::{CharSet, RegexNode};
use std::collections::HashSet;

/// NFA state ID
pub type StateId = usize;

/// NFA transition
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Transition on specific character
    Char(char),
    /// Transition on any character (.)
    Any,
    /// Transition on character in set
    CharSet(CharSet),
    /// Transition on character NOT in set
    NegatedCharSet(CharSet),
    /// Epsilon transition (no input consumed)
    Epsilon,
    /// Start anchor (^) - matches at beginning of text
    StartAnchor,
    /// End anchor ($) - matches at end of text
    EndAnchor,
    /// Word boundary (\b) - matches at word/non-word transition
    WordBoundary,
    /// Non-word boundary (\B) - matches where \b doesn't match
    NonWordBoundary,
    /// Start of word (\<) - matches at start of word
    StartWord,
    /// End of word (\>) - matches at end of word
    EndWord,
    /// Start of capture group - records position
    GroupStart(usize),
    /// End of capture group - records position
    GroupEnd(usize),
    /// Backreference (\1-\9) - matches same text as captured group
    Backref(usize),
}

/// NFA state
#[derive(Debug, Clone)]
pub struct State {
    pub id: StateId,
    /// Outgoing transitions: (Transition, target_state_id)
    pub transitions: Vec<(Transition, StateId)>,
    /// Is this an accepting state?
    pub is_accept: bool,
}

impl State {
    fn new(id: StateId) -> Self {
        State {
            id,
            transitions: Vec::new(),
            is_accept: false,
        }
    }

    fn add_transition(&mut self, transition: Transition, target: StateId) {
        self.transitions.push((transition, target));
    }
}

/// NFA (Non-deterministic Finite Automaton)
#[derive(Debug, Clone)]
pub struct Nfa {
    pub states: Vec<State>,
    pub start: StateId,
    pub accept: StateId,
}

impl Nfa {
    /// Create new NFA with start and accept states
    pub fn new() -> Self {
        let start_state = State::new(0);
        let accept_state = State {
            id: 1,
            transitions: Vec::new(),
            is_accept: true,
        };

        Nfa {
            states: vec![start_state, accept_state],
            start: 0,
            accept: 1,
        }
    }

    /// Add new state and return its ID
    fn add_state(&mut self) -> StateId {
        let id = self.states.len();
        self.states.push(State::new(id));
        id
    }

    /// Add transition between states
    fn add_transition(&mut self, from: StateId, transition: Transition, to: StateId) {
        self.states[from].add_transition(transition, to);
    }

    /// Build NFA from AST using Thompson's construction
    pub fn from_ast(ast: &RegexNode) -> Self {
        let mut nfa = Nfa::new();
        let fragment = nfa.build_fragment(ast);

        // Connect start to fragment start via epsilon
        nfa.add_transition(nfa.start, Transition::Epsilon, fragment.start);

        // Connect fragment end to accept via epsilon
        nfa.add_transition(fragment.end, Transition::Epsilon, nfa.accept);

        nfa
    }

    /// Build NFA fragment from AST node
    /// Returns (start_state, end_state) of the fragment
    fn build_fragment(&mut self, node: &RegexNode) -> Fragment {
        match node {
            // Literal character: create simple path
            RegexNode::Literal(ch) => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::Char(*ch), end);
                Fragment { start, end }
            }

            // Any (dot): matches any character
            RegexNode::Any => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::Any, end);
                Fragment { start, end }
            }

            // Character class [abc]
            RegexNode::CharClass(set) => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::CharSet(set.clone()), end);
                Fragment { start, end }
            }

            // Negated character class [^abc]
            RegexNode::NegatedCharClass(set) => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::NegatedCharSet(set.clone()), end);
                Fragment { start, end }
            }

            // Sequence: concatenate fragments
            RegexNode::Sequence(nodes) => {
                if nodes.is_empty() {
                    // Empty sequence: epsilon transition
                    let start = self.add_state();
                    let end = self.add_state();
                    self.add_transition(start, Transition::Epsilon, end);
                    return Fragment { start, end };
                }

                // Build first fragment
                let mut current = self.build_fragment(&nodes[0]);

                // Concatenate remaining fragments
                for node in &nodes[1..] {
                    let next = self.build_fragment(node);
                    // Connect current end to next start via epsilon
                    self.add_transition(current.end, Transition::Epsilon, next.start);
                    current.end = next.end;
                }

                current
            }

            // Alternation: a|b
            RegexNode::Alternation(branches) => {
                let start = self.add_state();
                let end = self.add_state();

                for branch in branches {
                    let frag = self.build_fragment(branch);
                    // Connect start to each branch via epsilon
                    self.add_transition(start, Transition::Epsilon, frag.start);
                    // Connect each branch to end via epsilon
                    self.add_transition(frag.end, Transition::Epsilon, end);
                }

                Fragment { start, end }
            }

            // Zero or more: a*
            RegexNode::ZeroOrMore(inner) => {
                let start = self.add_state();
                let end = self.add_state();
                let frag = self.build_fragment(inner);

                // For greedy matching with stack-based backtracking:
                // Add transitions in reverse priority order (last added = first tried)

                // From start: skip to end first (lower priority for greedy)
                self.add_transition(start, Transition::Epsilon, end);
                // Then try to match (higher priority for greedy)
                self.add_transition(start, Transition::Epsilon, frag.start);

                // From fragment end: exit first (lower priority for greedy)
                self.add_transition(frag.end, Transition::Epsilon, end);
                // Then loop back for more matches (higher priority for greedy)
                self.add_transition(frag.end, Transition::Epsilon, frag.start);

                Fragment { start, end }
            }

            // One or more: a+
            RegexNode::OneOrMore(inner) => {
                let frag = self.build_fragment(inner);
                let start = frag.start;
                let end = self.add_state();

                // Must match at least once
                // For greedy matching with LIFO stack:
                // - Exit transition added FIRST (explored last = only if loop fails)
                // - Loop transition added LAST (explored first = greedy)
                self.add_transition(frag.end, Transition::Epsilon, end);
                self.add_transition(frag.end, Transition::Epsilon, frag.start);

                Fragment { start, end }
            }

            // Zero or one: a?
            RegexNode::ZeroOrOne(inner) => {
                let start = self.add_state();
                let end = self.add_state();
                let frag = self.build_fragment(inner);

                // For greedy matching with LIFO stack:
                // - Skip transition added FIRST (explored last = only if match fails)
                // - Match transition added LAST (explored first = greedy)
                self.add_transition(start, Transition::Epsilon, end);
                self.add_transition(start, Transition::Epsilon, frag.start);

                // Connect fragment end to overall end
                self.add_transition(frag.end, Transition::Epsilon, end);

                Fragment { start, end }
            }

            // Repeat {m,n}
            RegexNode::Repeat { node, min, max } => self.build_repeat_fragment(node, *min, *max),

            // Start anchor (^)
            RegexNode::StartAnchor => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::StartAnchor, end);
                Fragment { start, end }
            }

            // End anchor ($)
            RegexNode::EndAnchor => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::EndAnchor, end);
                Fragment { start, end }
            }

            // Word boundary (\b)
            RegexNode::WordBoundary => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::WordBoundary, end);
                Fragment { start, end }
            }

            // Non-word boundary (\B)
            RegexNode::NonWordBoundary => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::NonWordBoundary, end);
                Fragment { start, end }
            }

            // Start of word (\<)
            RegexNode::StartWord => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::StartWord, end);
                Fragment { start, end }
            }

            // End of word (\>)
            RegexNode::EndWord => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::EndWord, end);
                Fragment { start, end }
            }

            // Capture group \(...\) or (...)
            RegexNode::Group { node, id } => {
                // Special case: Quantifiers inside Group
                // For patterns like \([0-9]\{1,3\}\) or \([0-9]*\), we need special handling
                // to avoid creating multiple separate groups
                match &**node {
                    RegexNode::Repeat {
                        node: inner_node,
                        min,
                        max,
                    } => {
                        let start = self.add_state();
                        let group_start = self.add_state();

                        // Mark start of capture group
                        self.add_transition(start, Transition::GroupStart(*id), group_start);

                        // Build repetition fragment (without creating multiple groups)
                        let repeat_frag = self.build_repeat_fragment(inner_node, *min, *max);
                        self.add_transition(group_start, Transition::Epsilon, repeat_frag.start);

                        // Mark end of capture group
                        let group_end = self.add_state();
                        let end = self.add_state();
                        self.add_transition(repeat_frag.end, Transition::Epsilon, group_end);
                        self.add_transition(group_end, Transition::GroupEnd(*id), end);

                        return Fragment { start, end };
                    }
                    _ => {
                        // Normal case handled below
                    }
                }

                // Normal case: build inner fragment normally
                let start = self.add_state();
                let group_start = self.add_state();

                // Mark start of capture group
                self.add_transition(start, Transition::GroupStart(*id), group_start);

                // Build inner fragment
                let inner = self.build_fragment(node);
                self.add_transition(group_start, Transition::Epsilon, inner.start);

                // Mark end of capture group
                let group_end = self.add_state();
                let end = self.add_state();
                self.add_transition(inner.end, Transition::Epsilon, group_end);
                self.add_transition(group_end, Transition::GroupEnd(*id), end);

                Fragment { start, end }
            }

            // Backreference \1-\9
            RegexNode::Backref(id) => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::Backref(*id), end);
                Fragment { start, end }
            }
        }
    }

    /// Build fragment for {m,n} repetition
    fn build_repeat_fragment(
        &mut self,
        node: &RegexNode,
        min: usize,
        max: Option<usize>,
    ) -> Fragment {
        match (min, max) {
            // {0,0} - never matches (shouldn't happen but handle it)
            (0, Some(0)) => {
                let start = self.add_state();
                let end = self.add_state();
                self.add_transition(start, Transition::Epsilon, end);
                Fragment { start, end }
            }

            // {m,m} - exactly m times
            (m, Some(n)) if m == n => {
                // Build m copies concatenated
                let mut fragments = Vec::new();
                for _ in 0..m {
                    fragments.push(self.build_fragment(node));
                }

                if fragments.is_empty() {
                    let start = self.add_state();
                    let end = self.add_state();
                    self.add_transition(start, Transition::Epsilon, end);
                    return Fragment { start, end };
                }

                let start = fragments[0].start;
                let mut current_end = fragments[0].end;

                for frag in fragments.iter().skip(1) {
                    self.add_transition(current_end, Transition::Epsilon, frag.start);
                    current_end = frag.end;
                }

                Fragment {
                    start,
                    end: current_end,
                }
            }

            // {m,n} - between m and n times
            (m, Some(n)) => {
                // Create new start and end states
                let start = self.add_state();
                let end = self.add_state();

                // Build n fragments total (m required + (n-m) optional)
                let mut fragments: Vec<Fragment> = Vec::new();
                for _ in 0..n {
                    fragments.push(self.build_fragment(node));
                }

                if fragments.is_empty() {
                    // {0,0} edge case
                    self.add_transition(start, Transition::Epsilon, end);
                    return Fragment { start, end };
                }

                // For greedy matching with stack-based backtracker (LIFO):
                // - Transitions are iterated in order and pushed to stack
                // - Last pushed = first explored
                // - For greedy: "continue" should be explored FIRST = pushed LAST
                // - So: add "skip/exit" transitions FIRST, "continue" transitions LAST

                // If min is 0, we can skip everything - add skip FIRST (explored last)
                if m == 0 {
                    self.add_transition(start, Transition::Epsilon, end);
                }

                // Connect start to first fragment LAST (explored first = greedy)
                self.add_transition(start, Transition::Epsilon, fragments[0].start);

                // Connect fragments in sequence, with optional exits after minimum
                for i in 0..fragments.len() {
                    // After completing fragment i, we've matched (i+1) times
                    // If (i+1) >= m, we can optionally exit - add FIRST (explored last)
                    if i + 1 >= m {
                        self.add_transition(fragments[i].end, Transition::Epsilon, end);
                    }

                    if i < fragments.len() - 1 {
                        // Connect to next fragment - add LAST (explored first = greedy)
                        self.add_transition(
                            fragments[i].end,
                            Transition::Epsilon,
                            fragments[i + 1].start,
                        );
                    }
                }

                Fragment { start, end }
            }

            // {m,} - m or more times (unbounded)
            (m, None) => {
                // Build m required copies + one copy with loop
                if m == 0 {
                    // {0,} is same as *
                    return self.build_fragment(&RegexNode::ZeroOrMore(Box::new(node.clone())));
                }

                let mut fragments = Vec::new();
                for _ in 0..m {
                    fragments.push(self.build_fragment(node));
                }

                let loop_frag = self.build_fragment(node);

                let start = fragments[0].start;
                let end = self.add_state();

                // Connect required copies
                let mut current_end = fragments[0].end;
                for frag in fragments.iter().skip(1) {
                    self.add_transition(current_end, Transition::Epsilon, frag.start);
                    current_end = frag.end;
                }

                // After required copies, we can either:
                // 1. Exit immediately (already matched m times = minimum)
                // 2. Try to match more via loop_frag
                // For greedy matching with LIFO stack: exit FIRST (explored last), loop LAST (explored first)
                self.add_transition(current_end, Transition::Epsilon, end);
                self.add_transition(current_end, Transition::Epsilon, loop_frag.start);

                // Loop fragment can repeat or exit
                // For greedy: exit FIRST (explored last), repeat LAST (explored first)
                self.add_transition(loop_frag.end, Transition::Epsilon, end);
                self.add_transition(loop_frag.end, Transition::Epsilon, loop_frag.start);

                Fragment { start, end }
            }
        }
    }

    /// Compute epsilon closure of a set of states
    pub fn epsilon_closure(&self, states: &HashSet<StateId>) -> HashSet<StateId> {
        let mut closure = states.clone();
        let mut stack: Vec<StateId> = states.iter().copied().collect();

        while let Some(state_id) = stack.pop() {
            for (transition, target) in &self.states[state_id].transitions {
                if matches!(transition, Transition::Epsilon) {
                    if closure.insert(*target) {
                        stack.push(*target);
                    }
                }
            }
        }

        closure
    }
}

/// Fragment of NFA (start and end states)
#[derive(Debug, Clone, Copy)]
struct Fragment {
    start: StateId,
    end: StateId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regex::parser;

    #[test]
    fn test_nfa_literal() {
        let ast = parser::parse_bre("a", false).unwrap();
        let nfa = Nfa::from_ast(&ast.ast);

        // Should have at least start, accept, and one literal transition
        assert!(nfa.states.len() >= 4);
        assert_eq!(nfa.start, 0);
        assert_eq!(nfa.accept, 1);
    }

    #[test]
    fn test_nfa_sequence() {
        let ast = parser::parse_bre("abc", false).unwrap();
        let nfa = Nfa::from_ast(&ast.ast);

        // Should have states for a, b, c plus start/accept
        assert!(nfa.states.len() >= 6);
    }

    #[test]
    fn test_nfa_alternation() {
        let ast = parser::parse_bre("a\\|b", false).unwrap();
        let nfa = Nfa::from_ast(&ast.ast);

        // Should have branching structure
        assert!(nfa.states.len() >= 6);
    }

    #[test]
    fn test_nfa_star() {
        let ast = parser::parse_bre("a*", false).unwrap();
        let nfa = Nfa::from_ast(&ast.ast);

        // Should have loop structure
        assert!(nfa.states.len() >= 4);
    }

    #[test]
    fn test_epsilon_closure() {
        let mut nfa = Nfa::new();
        let s2 = nfa.add_state();
        let s3 = nfa.add_state();

        nfa.add_transition(0, Transition::Epsilon, s2);
        nfa.add_transition(s2, Transition::Epsilon, s3);

        let mut initial = HashSet::new();
        initial.insert(0);

        let closure = nfa.epsilon_closure(&initial);

        // Should include 0, s2, s3
        assert!(closure.contains(&0));
        assert!(closure.contains(&s2));
        assert!(closure.contains(&s3));
    }
}
