// Copyright (c) 2026 Red Authors
// License: MIT
//

// Abstract Syntax Tree for BRE/ERE patterns

/// AST node representing a regex pattern
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegexNode {
    // === Basic atoms ===
    /// Single literal character
    Literal(char),

    /// Any character (.) - matches everything except newline (unless dotall mode)
    Any,

    /// Start of line anchor (^)
    StartAnchor,

    /// End of line anchor ($)
    EndAnchor,

    // === Character classes ===
    /// Character class like [abc] or [a-z]
    CharClass(CharSet),

    /// Negated character class like [^abc]
    NegatedCharClass(CharSet),

    // === Quantifiers ===
    /// Zero or more repetitions (*)
    ZeroOrMore(Box<RegexNode>),

    /// One or more repetitions (\+ in BRE, + in ERE)
    OneOrMore(Box<RegexNode>),

    /// Zero or one repetition (\? in BRE, ? in ERE)
    ZeroOrOne(Box<RegexNode>),

    /// Bounded repetition \{m,n\} in BRE, {m,n} in ERE
    Repeat {
        node: Box<RegexNode>,
        min: usize,
        max: Option<usize>, // None means unbounded
    },

    // === Grouping and alternation ===
    /// Capturing group \(...\) in BRE, (...) in ERE
    Group {
        id: usize, // Group number (1-based for sed compatibility)
        node: Box<RegexNode>,
    },

    /// Alternation a\|b in BRE, a|b in ERE
    Alternation(Vec<RegexNode>),

    /// Sequence of nodes (concatenation)
    Sequence(Vec<RegexNode>),

    // === Backreferences ===
    /// Backreference \1, \2, etc.
    Backref(usize),

    // === Word boundaries (GNU extensions) ===
    /// Word boundary \b
    WordBoundary,

    /// Non-word boundary \B
    NonWordBoundary,

    /// Start of word \< (GNU extension)
    StartWord,

    /// End of word \> (GNU extension)
    EndWord,
}

/// Character set for character classes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharSet {
    /// List of character ranges (inclusive)
    /// e.g., [a-zA-Z] → vec![('a', 'z'), ('A', 'Z')]
    pub ranges: Vec<(char, char)>,

    /// POSIX character classes like [:alpha:], [:digit:]
    pub posix_classes: Vec<PosixClass>,
}

impl CharSet {
    pub fn new() -> Self {
        CharSet {
            ranges: Vec::new(),
            posix_classes: Vec::new(),
        }
    }

    pub fn add_range(&mut self, start: char, end: char) {
        self.ranges.push((start, end));
    }

    pub fn add_char(&mut self, ch: char) {
        self.ranges.push((ch, ch));
    }

    pub fn add_posix_class(&mut self, class: PosixClass) {
        self.posix_classes.push(class);
    }

    /// Check if character matches this set
    pub fn matches(&self, ch: char) -> bool {
        // Check ranges
        for (start, end) in &self.ranges {
            if ch >= *start && ch <= *end {
                return true;
            }
        }

        // Check POSIX classes
        for class in &self.posix_classes {
            if class.matches(ch) {
                return true;
            }
        }

        false
    }
}

/// POSIX character classes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosixClass {
    /// [:alpha:] - alphabetic characters
    Alpha,

    /// [:digit:] - decimal digits
    Digit,

    /// [:alnum:] - alphanumeric characters
    Alnum,

    /// [:space:] - whitespace characters
    Space,

    /// [:blank:] - space and tab
    Blank,

    /// [:upper:] - uppercase letters
    Upper,

    /// [:lower:] - lowercase letters
    Lower,

    /// [:punct:] - punctuation characters
    Punct,

    /// [:xdigit:] - hexadecimal digits
    Xdigit,

    /// [:cntrl:] - control characters
    Cntrl,

    /// [:graph:] - visible characters (no space)
    Graph,

    /// [:print:] - printable characters (includes space)
    Print,
}

impl PosixClass {
    /// Check if character matches this POSIX class
    pub fn matches(&self, ch: char) -> bool {
        match self {
            PosixClass::Alpha => ch.is_alphabetic(),
            PosixClass::Digit => ch.is_ascii_digit(),
            PosixClass::Alnum => ch.is_alphanumeric(),
            PosixClass::Space => ch.is_whitespace(),
            PosixClass::Blank => ch == ' ' || ch == '\t',
            PosixClass::Upper => ch.is_uppercase(),
            PosixClass::Lower => ch.is_lowercase(),
            PosixClass::Punct => ch.is_ascii_punctuation(),
            PosixClass::Xdigit => ch.is_ascii_hexdigit(),
            PosixClass::Cntrl => ch.is_control(),
            PosixClass::Graph => !ch.is_whitespace() && !ch.is_control(),
            PosixClass::Print => !ch.is_control(),
        }
    }

    /// Parse POSIX class name
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "alpha" => Some(PosixClass::Alpha),
            "digit" => Some(PosixClass::Digit),
            "alnum" => Some(PosixClass::Alnum),
            "space" => Some(PosixClass::Space),
            "blank" => Some(PosixClass::Blank),
            "upper" => Some(PosixClass::Upper),
            "lower" => Some(PosixClass::Lower),
            "punct" => Some(PosixClass::Punct),
            "xdigit" => Some(PosixClass::Xdigit),
            "cntrl" => Some(PosixClass::Cntrl),
            "graph" => Some(PosixClass::Graph),
            "print" => Some(PosixClass::Print),
            _ => None,
        }
    }
}

/// Compiled regex with metadata
#[derive(Debug, Clone)]
pub struct CompiledRegex {
    pub ast: RegexNode,
    pub num_groups: usize,
    pub is_ere: bool,
    pub ignore_case: bool,
    pub dotall: bool, // Whether . matches \n
}

impl CompiledRegex {
    pub fn new(
        ast: RegexNode,
        num_groups: usize,
        is_ere: bool,
        ignore_case: bool,
        dotall: bool,
    ) -> Self {
        CompiledRegex {
            ast,
            num_groups,
            is_ere,
            ignore_case,
            dotall,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_charset_matches() {
        let mut set = CharSet::new();
        set.add_range('a', 'z');
        set.add_range('A', 'Z');

        assert!(set.matches('a'));
        assert!(set.matches('z'));
        assert!(set.matches('A'));
        assert!(set.matches('Z'));
        assert!(!set.matches('0'));
        assert!(!set.matches(' '));
    }

    #[test]
    fn test_posix_digit() {
        assert!(PosixClass::Digit.matches('0'));
        assert!(PosixClass::Digit.matches('9'));
        assert!(!PosixClass::Digit.matches('a'));
    }

    #[test]
    fn test_posix_alpha() {
        assert!(PosixClass::Alpha.matches('a'));
        assert!(PosixClass::Alpha.matches('Z'));
        assert!(!PosixClass::Alpha.matches('0'));
    }
}
