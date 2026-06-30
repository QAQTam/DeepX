// Copyright (c) 2026 Red Authors
// License: MIT
//

use crate::parser::{AddressRange, PrintTiming};

/// Regex matcher using custom engine with 3-level optimization (Literal/DFA/NFA)
#[derive(Debug, Clone)]
pub struct SedRegex {
    pub matcher: crate::regex::Matcher,
}

/// Compiled substitution data - result of compiling a Substitution command
#[derive(Debug, Clone)]
pub struct CompiledSubstitution {
    pub range: Option<AddressRange>,
    pub negated: bool,
    pub pattern: SedRegex,
    pub replacement: ReplacementTemplate,
    pub global: bool,
    pub print: bool,
    pub write_file: Option<String>,
    pub occurrence: Option<usize>,
    pub use_last: bool,
    pub execute: bool,
    pub print_timing: PrintTiming,
    pub literal_pattern: Option<String>,
    pub literal_replacement: Option<String>,
    pub literal_pattern_bytes: Option<Vec<u8>>,
    pub literal_replacement_bytes: Option<Vec<u8>>,
}

impl SedRegex {
    /// Create new SedRegex from custom matcher
    pub fn new(matcher: crate::regex::Matcher) -> Self {
        SedRegex { matcher }
    }

    /// Check if the pattern matches the text
    pub fn is_match(&self, text: &str) -> bool {
        self.matcher.is_match(text)
    }
}

#[derive(Debug, Clone)]
pub enum ReplacementToken {
    Literal(String),
    /// Raw bytes literal (for preserving invalid UTF-8 in replacement)
    LiteralBytes(Vec<u8>),
    WholeMatch,
    Group(u8),
    UppercaseNext, // \u - uppercase next character
    LowercaseNext, // \l - lowercase next character
    UppercaseAll,  // \U - uppercase all following characters
    LowercaseAll,  // \L - lowercase all following characters
    EndCase,       // \E - end case conversion
}

#[derive(Debug, Clone)]
pub struct ReplacementTemplate {
    pub tokens: Vec<ReplacementToken>,
}

#[derive(Debug, Clone)]
pub enum Command {
    /// Substitution command - uses pre-compiled CompiledSubstitution
    Substitution(CompiledSubstitution),
    List {
        range: Option<AddressRange>,
        negated: bool,
        line_length: Option<usize>, // Optional line wrap length (e.g., l70)
    },
    Translate {
        range: Option<AddressRange>,
        negated: bool,
        from: String,
        to: String,
        /// Raw bytes for 'from' string (for preserving invalid UTF-8)
        from_bytes: Option<Vec<u8>>,
        /// Raw bytes for 'to' string (for preserving invalid UTF-8)
        to_bytes: Option<Vec<u8>>,
    },
    Print {
        range: Option<AddressRange>,
        negated: bool,
    },
    PrintFirstLine {
        range: Option<AddressRange>,
        negated: bool,
    },
    Delete {
        range: Option<AddressRange>,
        negated: bool,
    },
    Quit {
        range: Option<AddressRange>,
        negated: bool,
        exit_code: Option<i32>,
    },
    QuitSilent {
        range: Option<AddressRange>,
        negated: bool,
        exit_code: Option<i32>,
    },
    Append {
        range: Option<AddressRange>,
        negated: bool,
        text: Option<String>, // None = unterminated, Some = explicit (even if empty)
    },
    Insert {
        range: Option<AddressRange>,
        negated: bool,
        text: Option<String>, // None = unterminated, Some = explicit (even if empty)
    },
    Change {
        range: Option<AddressRange>,
        negated: bool,
        text: Option<String>, // None = unterminated, Some = explicit (even if empty)
    },
    N {
        range: Option<AddressRange>,
        negated: bool,
    },
    BigD {
        range: Option<AddressRange>,
        negated: bool,
    },
    HoldCopy {
        range: Option<AddressRange>,
        negated: bool,
    },
    HoldAppend {
        range: Option<AddressRange>,
        negated: bool,
    },
    GetCopy {
        range: Option<AddressRange>,
        negated: bool,
    },
    GetAppend {
        range: Option<AddressRange>,
        negated: bool,
    },
    Exchange {
        range: Option<AddressRange>,
        negated: bool,
    },
    Label {
        name: String,
    },
    Branch {
        range: Option<AddressRange>,
        negated: bool,
        label: String,
        /// Pre-resolved target index (set after all commands are parsed)
        target_index: Option<usize>,
    },
    Test {
        range: Option<AddressRange>,
        negated: bool,
        label: String,
        /// Pre-resolved target index (set after all commands are parsed)
        target_index: Option<usize>,
    },
    TestNeg {
        range: Option<AddressRange>,
        negated: bool,
        label: String,
        /// Pre-resolved target index (set after all commands are parsed)
        target_index: Option<usize>,
    },
    Execute {
        range: Option<AddressRange>,
        negated: bool,
        command: Option<String>, // None = execute pattern space, Some = execute this command
    },
    Clear {
        range: Option<AddressRange>,
        negated: bool,
    },
    PrintFilename {
        range: Option<AddressRange>,
        negated: bool,
    },
    Next,
    Write {
        range: Option<AddressRange>,
        negated: bool,
        path: String,
    },
    WriteFirstLine {
        range: Option<AddressRange>,
        negated: bool,
        path: String,
    },
    Read {
        range: Option<AddressRange>,
        negated: bool,
        path: String,
    },
    ReadLine {
        range: Option<AddressRange>,
        negated: bool,
        path: String,
    },
    LineNumber {
        range: Option<AddressRange>,
        negated: bool,
    },
}

#[derive(Debug)]
pub enum CommandResult {
    /// Continue processing, output this content at end of cycle
    /// Contains (text_content, raw_bytes) - use raw_bytes for output if available
    Continue(String, Option<Vec<u8>>),
    /// Explicit print command (p) - output this content immediately
    /// Contains (text_content, raw_bytes) - use raw_bytes for output if available
    Print(String, Option<Vec<u8>>),
    /// Delete pattern space and start next cycle
    Delete,
    /// Print content and continue processing (for P command)
    PrintAndContinue(String),
    /// Quit with optional exit code
    Quit(Option<i32>),
    /// Restart cycle with pattern space already modified in place
    Restart,
    /// Restart cycle with new pattern space content
    RestartWith(String),
    /// Restart cycle with raw bytes (preserves invalid UTF-8)
    RestartWithBytes(Vec<u8>),
    /// Append next line and resume at given program counter
    AppendNextAndResume {
        resume_pc: usize,
        pattern_space: String,
    },
    /// Read next line and resume at given program counter
    NextLineAndResume { resume_pc: usize },
    /// Suppress auto-print but don't end cycle (used by e command)
    SuppressAutoprint,
}
