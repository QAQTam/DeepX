// Copyright (c) 2026 Red Authors
// License: MIT
//

/// Token types for command parsing
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Addresses
    LineNumber(usize),         // 4, 20
    Dollar,                    // $
    RegexAddr(String, String), // /pattern/[modifiers] (pattern, modifiers)
    PlusOffset(usize),         // +N
    TildeStep(usize),          // ~N

    // Separators and operators
    Comma,     // ,
    Semicolon, // ;
    Newline,
    Bang,       // !
    OpenBrace,  // {
    CloseBrace, // }

    // Commands
    Command(char),           // s, p, d, etc
    SubstitutionDelim(char), // delimiter for s/../../
    SubstitutionFlag(char),  // g, p, w, I, i
    // Raw body of s-command starting with delimiter and including flags
    // Tuple: (body_text, optional raw_bytes for replacement preservation)
    SubstitutionBody(String, Option<Vec<u8>>),
    // Raw body of y-command: delimiter + from + delimiter + to + delimiter
    // Tuple: (body_text, optional raw_bytes for from/to preservation)
    TranslateBody(String, Option<Vec<u8>>),

    // Bodies for a/i/c commands (capture following text line)
    // Option: None = unterminated (no text), Some = explicit text (even if empty)
    AppendBody(Option<String>),
    InsertBody(Option<String>),
    ChangeBody(Option<String>),

    // Command to execute for e command (can be empty)
    ExecuteBody(String),

    // Literals and text
    String(String),
    Filename(String),
    Comment(String), // #comment

    Eof,
}

/// Address types for sed commands
#[derive(Debug, Clone, PartialEq)]
pub enum Address {
    Line(usize),                   // 4
    Dollar,                        // $
    Regex(String),                 // /pattern/
    Relative(Box<Address>, isize), // addr+N, addr-N
    Step(Box<Address>, usize),     // addr~N
}

/// Address range (addr1,addr2)
/// Each range has a unique ID for tracking state in AddressEvaluator
#[derive(Debug, Clone)]
pub struct AddressRange {
    pub start: Option<Address>,
    pub end: Option<Address>,
    /// Unique identifier for this range instance (used for state tracking)
    pub id: u64,
}

impl AddressRange {
    /// Create a new AddressRange with a unique ID
    pub fn new(start: Option<Address>, end: Option<Address>) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            start,
            end,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }
}

// PartialEq ignores id - two ranges are equal if they have the same addresses
impl PartialEq for AddressRange {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start && self.end == other.end
    }
}

/// Print/Execute timing for substitution command
///
/// When both 'p' and 'e' flags are present, their order matters:
/// - 'pe': print the substituted line, then execute it as a shell command
/// - 'ep': execute the substituted line as a shell command, then print it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintTiming {
    /// Neither print nor execute flags present, or only one of them
    None,
    /// 'pe' - print then execute
    PrintThenExecute,
    /// 'ep' - execute then print
    ExecuteThenPrint,
}

impl Default for PrintTiming {
    fn default() -> Self {
        PrintTiming::None
    }
}

/// Flags for substitution command
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SubstitutionFlags {
    pub global: bool,               // g
    pub print: bool,                // p
    pub write_file: Option<String>, // w filename
    pub ignore_case: bool,          // I/i (BSD extension)
    pub occurrence: Option<usize>,  // 1-9
    pub multiline: bool,            // m - multiline mode (^ and $ match line boundaries)
    pub multiline_dotall: bool,     // M - multiline + dotall (. matches \n)
    pub execute: bool,              // e - execute replacement as shell command
    pub print_timing: PrintTiming,  // Timing for 'p' and 'e' flags (replaces print_before_execute)
}

/// AST node for commands
#[derive(Debug, Clone)]
pub enum Command {
    Substitution {
        range: Option<AddressRange>,
        negated: bool,
        pattern: String,
        /// Raw bytes for pattern string (for non-UTF-8 byte matching)
        pattern_raw_bytes: Option<Vec<u8>>,
        replacement: String,
        /// Raw bytes for replacement string (for preserving invalid UTF-8)
        replacement_raw_bytes: Option<Vec<u8>>,
        flags: SubstitutionFlags,
        delimiter: char,
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
        delimiter: char,
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
    // Pattern/Hold space related (subset): N (append next line), D (delete first line and restart)
    N {
        range: Option<AddressRange>,
        negated: bool,
    },
    BigD {
        range: Option<AddressRange>,
        negated: bool,
    },
    HoldCopy {
        // h
        range: Option<AddressRange>,
        negated: bool,
    },
    HoldAppend {
        // H
        range: Option<AddressRange>,
        negated: bool,
    },
    GetCopy {
        // g
        range: Option<AddressRange>,
        negated: bool,
    },
    GetAppend {
        // G
        range: Option<AddressRange>,
        negated: bool,
    },
    Exchange {
        // x
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
    },
    Test {
        range: Option<AddressRange>,
        negated: bool,
        label: String,
    },
    TestNeg {
        range: Option<AddressRange>,
        negated: bool,
        label: String,
    },
    Execute {
        range: Option<AddressRange>,
        negated: bool,
        command: Option<String>, // None = execute pattern space, Some = execute this command
    },
    Version {
        version: String, // Empty means "4.0"
    },
    Clear {
        range: Option<AddressRange>,
        negated: bool,
    },
    PrintFilename {
        range: Option<AddressRange>,
        negated: bool,
    },
    Next, // n
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
    Group {
        range: Option<AddressRange>,
        negated: bool,
        commands: Vec<Command>,
    },
    LineNumber {
        range: Option<AddressRange>,
        negated: bool,
    },
    List {
        range: Option<AddressRange>,
        negated: bool,
        line_length: Option<usize>, // Optional line wrap length (e.g., l70)
    },
    Comment(String), // # comment
}

/// Trait for commands that have an address range
///
/// Most sed commands can be restricted to specific line ranges.
/// This trait provides uniform access to the range field.
pub trait HasAddressRange {
    /// Get reference to address range (if any)
    fn address_range(&self) -> Option<&AddressRange>;
}

impl HasAddressRange for Command {
    fn address_range(&self) -> Option<&AddressRange> {
        match self {
            Self::Substitution { range, .. }
            | Self::Translate { range, .. }
            | Self::Append { range, .. }
            | Self::Insert { range, .. }
            | Self::Change { range, .. }
            | Self::N { range, .. }
            | Self::BigD { range, .. }
            | Self::HoldCopy { range, .. }
            | Self::HoldAppend { range, .. }
            | Self::GetCopy { range, .. }
            | Self::GetAppend { range, .. }
            | Self::Exchange { range, .. }
            | Self::Branch { range, .. }
            | Self::Test { range, .. }
            | Self::TestNeg { range, .. }
            | Self::Execute { range, .. }
            | Self::Clear { range, .. }
            | Self::PrintFilename { range, .. }
            | Self::Write { range, .. }
            | Self::WriteFirstLine { range, .. }
            | Self::Read { range, .. }
            | Self::ReadLine { range, .. }
            | Self::Print { range, .. }
            | Self::PrintFirstLine { range, .. }
            | Self::Delete { range, .. }
            | Self::Quit { range, .. }
            | Self::QuitSilent { range, .. }
            | Self::Group { range, .. }
            | Self::LineNumber { range, .. }
            | Self::List { range, .. } => range.as_ref(),

            // Commands without address range
            Self::Label { .. } | Self::Version { .. } | Self::Next | Self::Comment(_) => None,
        }
    }
}

/// Display implementation for user-friendly error messages
impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Command(ch) => write!(f, "'{}'", ch),
            Token::Semicolon => write!(f, "';'"),
            Token::CloseBrace => write!(f, "'}}'"),
            Token::OpenBrace => write!(f, "'{{'"),
            Token::Newline => write!(f, "newline"),
            Token::Eof => write!(f, "end of script"),
            Token::Filename(name) => write!(f, "'{}'", name),
            Token::Bang => write!(f, "'!'"),
            Token::Comma => write!(f, "','"),
            Token::Dollar => write!(f, "'$'"),
            Token::LineNumber(n) => write!(f, "{}", n),
            Token::RegexAddr(pattern, modifiers) => {
                if modifiers.is_empty() {
                    write!(f, "/{}/", pattern)
                } else {
                    write!(f, "/{}/{}", pattern, modifiers)
                }
            }
            Token::PlusOffset(n) => write!(f, "+{}", n),
            Token::TildeStep(n) => write!(f, "~{}", n),
            Token::SubstitutionDelim(ch) => write!(f, "'{}'", ch),
            Token::SubstitutionFlag(ch) => write!(f, "'{}'", ch),
            Token::SubstitutionBody(s, _) => write!(f, "substitution '{}'", s),
            Token::TranslateBody(s, _) => write!(f, "translate '{}'", s),
            Token::AppendBody(Some(s)) => write!(f, "append text '{}'", s),
            Token::AppendBody(None) => write!(f, "append (unterminated)"),
            Token::InsertBody(Some(s)) => write!(f, "insert text '{}'", s),
            Token::InsertBody(None) => write!(f, "insert (unterminated)"),
            Token::ChangeBody(Some(s)) => write!(f, "change text '{}'", s),
            Token::ChangeBody(None) => write!(f, "change (unterminated)"),
            Token::ExecuteBody(s) => write!(f, "execute '{}'", s),
            Token::String(s) => write!(f, "\"{}\"", s),
            Token::Comment(s) => write!(f, "comment '{}'", s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_display_commands() {
        assert_eq!(format!("{}", Token::Command('s')), "'s'");
        assert_eq!(format!("{}", Token::Command('p')), "'p'");
    }

    #[test]
    fn test_token_display_separators() {
        assert_eq!(format!("{}", Token::Semicolon), "';'");
        assert_eq!(format!("{}", Token::CloseBrace), "'}'");
        assert_eq!(format!("{}", Token::OpenBrace), "'{'");
        assert_eq!(format!("{}", Token::Newline), "newline");
        assert_eq!(format!("{}", Token::Eof), "end of script");
        assert_eq!(format!("{}", Token::Bang), "'!'");
        assert_eq!(format!("{}", Token::Comma), "','");
    }

    #[test]
    fn test_token_display_addresses() {
        assert_eq!(format!("{}", Token::Dollar), "'$'");
        assert_eq!(format!("{}", Token::LineNumber(42)), "42");
        assert_eq!(format!("{}", Token::PlusOffset(5)), "+5");
        assert_eq!(format!("{}", Token::TildeStep(3)), "~3");
    }

    #[test]
    fn test_token_display_regex_addr() {
        assert_eq!(
            format!("{}", Token::RegexAddr("foo".to_string(), "".to_string())),
            "/foo/"
        );
        assert_eq!(
            format!("{}", Token::RegexAddr("bar".to_string(), "I".to_string())),
            "/bar/I"
        );
    }

    #[test]
    fn test_token_display_substitution() {
        assert_eq!(format!("{}", Token::SubstitutionDelim('/')), "'/'");
        assert_eq!(format!("{}", Token::SubstitutionFlag('g')), "'g'");
        assert_eq!(
            format!("{}", Token::SubstitutionBody("/a/b/".to_string(), None)),
            "substitution '/a/b/'"
        );
    }

    #[test]
    fn test_token_display_translate() {
        assert_eq!(
            format!("{}", Token::TranslateBody("/abc/xyz/".to_string(), None)),
            "translate '/abc/xyz/'"
        );
    }

    #[test]
    fn test_token_display_text_bodies() {
        assert_eq!(
            format!("{}", Token::AppendBody(Some("hello".to_string()))),
            "append text 'hello'"
        );
        assert_eq!(
            format!("{}", Token::AppendBody(None)),
            "append (unterminated)"
        );

        assert_eq!(
            format!("{}", Token::InsertBody(Some("world".to_string()))),
            "insert text 'world'"
        );
        assert_eq!(
            format!("{}", Token::InsertBody(None)),
            "insert (unterminated)"
        );

        assert_eq!(
            format!("{}", Token::ChangeBody(Some("new".to_string()))),
            "change text 'new'"
        );
        assert_eq!(
            format!("{}", Token::ChangeBody(None)),
            "change (unterminated)"
        );

        assert_eq!(
            format!("{}", Token::ExecuteBody("ls -la".to_string())),
            "execute 'ls -la'"
        );
    }

    #[test]
    fn test_token_display_strings() {
        assert_eq!(
            format!("{}", Token::Filename("/tmp/file".to_string())),
            "'/tmp/file'"
        );
        assert_eq!(format!("{}", Token::String("text".to_string())), "\"text\"");
        assert_eq!(
            format!("{}", Token::Comment("note".to_string())),
            "comment 'note'"
        );
    }

    #[test]
    fn test_address_range_new_unique_ids() {
        let range1 = AddressRange::new(None, None);
        let range2 = AddressRange::new(None, None);
        // Each range should have a unique ID
        assert_ne!(range1.id, range2.id);
    }

    #[test]
    fn test_address_range_eq_ignores_id() {
        let range1 = AddressRange::new(Some(Address::Line(1)), Some(Address::Line(5)));
        let range2 = AddressRange::new(Some(Address::Line(1)), Some(Address::Line(5)));
        // IDs are different but ranges should be equal
        assert_ne!(range1.id, range2.id);
        assert_eq!(range1, range2);
    }

    #[test]
    fn test_print_timing_default() {
        assert_eq!(PrintTiming::default(), PrintTiming::None);
    }

    #[test]
    fn test_has_address_range_substitution() {
        let cmd = Command::Substitution {
            range: Some(AddressRange::new(Some(Address::Line(1)), None)),
            negated: false,
            pattern: "foo".to_string(),
            pattern_raw_bytes: None,
            replacement: "bar".to_string(),
            replacement_raw_bytes: None,
            flags: SubstitutionFlags::default(),
            delimiter: '/',
        };
        assert!(cmd.address_range().is_some());
    }

    #[test]
    fn test_has_address_range_label() {
        let cmd = Command::Label {
            name: "loop".to_string(),
        };
        assert!(cmd.address_range().is_none());
    }

    #[test]
    fn test_has_address_range_version() {
        let cmd = Command::Version {
            version: "4.0".to_string(),
        };
        assert!(cmd.address_range().is_none());
    }

    #[test]
    fn test_has_address_range_next() {
        let cmd = Command::Next;
        assert!(cmd.address_range().is_none());
    }

    #[test]
    fn test_has_address_range_comment() {
        let cmd = Command::Comment("test".to_string());
        assert!(cmd.address_range().is_none());
    }
}
