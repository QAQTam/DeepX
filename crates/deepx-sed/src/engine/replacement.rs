// Copyright (c) 2026 Red Authors
// License: MIT
//

use fancy_regex::Captures;

use super::types::{ReplacementTemplate, ReplacementToken};

#[derive(Debug, Clone, Copy, PartialEq)]
enum CaseMode {
    None,
    UppercaseNext,
    LowercaseNext,
    UppercaseAll,
    LowercaseAll,
}

pub fn render_replacement(caps: &Captures, template: &ReplacementTemplate) -> String {
    let mut out = String::new();
    let mut base_mode = CaseMode::None; // Persistent mode (\U, \L, or None)
    let mut next_mode = CaseMode::None; // One-shot mode (\u, \l)

    for token in &template.tokens {
        match token {
            ReplacementToken::Literal(s) => {
                out.push_str(&apply_case_modes(s, &mut base_mode, &mut next_mode));
            }
            ReplacementToken::WholeMatch => {
                if let Some(m) = caps.get(0) {
                    out.push_str(&apply_case_modes(
                        m.as_str(),
                        &mut base_mode,
                        &mut next_mode,
                    ));
                }
            }
            ReplacementToken::Group(d) => {
                let idx = *d as usize;
                if let Some(m) = caps.get(idx) {
                    out.push_str(&apply_case_modes(
                        m.as_str(),
                        &mut base_mode,
                        &mut next_mode,
                    ));
                }
            }
            ReplacementToken::UppercaseNext => {
                next_mode = CaseMode::UppercaseNext;
            }
            ReplacementToken::LowercaseNext => {
                next_mode = CaseMode::LowercaseNext;
            }
            ReplacementToken::UppercaseAll => {
                base_mode = CaseMode::UppercaseAll;
            }
            ReplacementToken::LowercaseAll => {
                base_mode = CaseMode::LowercaseAll;
            }
            ReplacementToken::EndCase => {
                base_mode = CaseMode::None;
            }
        }
    }
    out
}

fn apply_case_modes(s: &str, base_mode: &mut CaseMode, next_mode: &mut CaseMode) -> String {
    if s.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    let mut chars = s.chars();

    // Handle first character with next_mode if set
    if *next_mode != CaseMode::None {
        if let Some(c) = chars.next() {
            match next_mode {
                CaseMode::UppercaseNext => result.extend(c.to_uppercase()),
                CaseMode::LowercaseNext => result.extend(c.to_lowercase()),
                _ => result.push(c),
            }
            *next_mode = CaseMode::None; // Reset one-shot mode
        }
    }

    // Apply base_mode to remaining characters
    for c in chars {
        match base_mode {
            CaseMode::None => result.push(c),
            CaseMode::UppercaseAll => result.extend(c.to_uppercase()),
            CaseMode::LowercaseAll => result.extend(c.to_lowercase()),
            _ => result.push(c), // Should not happen for base_mode
        }
    }

    result
}
