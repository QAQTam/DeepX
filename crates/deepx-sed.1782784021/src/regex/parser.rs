// Copyright (c) 2026 Red Authors
// License: MIT
//

// Parser for BRE (Basic Regular Expression) and ERE (Extended Regular Expression)

use super::ast::*;
use crate::errors::{Result, SedError};

/// Parser for BRE/ERE patterns
pub struct Parser {
    chars: Vec<char>,
    pos: usize,
    is_ere: bool,
    posix_mode: bool,
    group_count: usize,
}

impl Parser {
    /// Create new parser for given pattern
    pub fn new(pattern: &str, is_ere: bool, posix_mode: bool) -> Self {
        Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            is_ere,
            posix_mode,
            group_count: 0,
        }
    }

    /// Parse the entire pattern
    pub fn parse(&mut self) -> Result<CompiledRegex> {
        let ast = self.parse_alternation()?;

        Ok(CompiledRegex::new(
            ast,
            self.group_count,
            self.is_ere,
            false, // ignore_case set later
            false, // dotall set later
        ))
    }

    /// Check if we're at end of pattern
    fn is_eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    /// Peek current character without consuming
    fn peek(&self) -> Option<char> {
        if self.is_eof() {
            None
        } else {
            Some(self.chars[self.pos])
        }
    }

    /// Peek next character (pos + 1) without consuming
    fn peek_next(&self) -> Option<char> {
        if self.pos + 1 >= self.chars.len() {
            None
        } else {
            Some(self.chars[self.pos + 1])
        }
    }

    /// Consume and return current character
    fn consume(&mut self) -> Option<char> {
        if self.is_eof() {
            None
        } else {
            let ch = self.chars[self.pos];
            self.pos += 1;
            Some(ch)
        }
    }

    /// Parse alternation (a\|b in BRE, a|b in ERE)
    fn parse_alternation(&mut self) -> Result<RegexNode> {
        let mut alternatives = vec![self.parse_sequence()?];

        loop {
            let alt_marker = if self.is_ere {
                // In ERE: | is alternation
                self.peek() == Some('|')
            } else {
                // In BRE: \| is alternation
                self.peek() == Some('\\') && self.peek_next() == Some('|')
            };

            if !alt_marker {
                break;
            }

            // Consume alternation marker
            if self.is_ere {
                self.consume(); // |
            } else {
                self.consume(); // \
                self.consume(); // |
            }

            alternatives.push(self.parse_sequence()?);
        }

        if alternatives.len() == 1 {
            // SAFETY: len() == 1 guarantees pop() returns Some
            Ok(alternatives.pop().expect("vec has exactly 1 element"))
        } else {
            Ok(RegexNode::Alternation(alternatives))
        }
    }

    /// Parse sequence of atoms (concatenation)
    fn parse_sequence(&mut self) -> Result<RegexNode> {
        let mut nodes = Vec::new();

        while !self.is_eof() {
            // Stop at alternation or closing paren
            if self.is_ere {
                if self.peek() == Some('|') || self.peek() == Some(')') {
                    break;
                }
            } else {
                if (self.peek() == Some('\\') && self.peek_next() == Some('|'))
                    || (self.peek() == Some('\\') && self.peek_next() == Some(')'))
                {
                    break;
                }
            }

            // Handle ^ as anchor only at the start of the sequence
            // Per POSIX: ^ is special only at the beginning of the RE or subexpression
            if self.peek() == Some('^') && nodes.is_empty() {
                self.consume();
                nodes.push(RegexNode::StartAnchor);
                continue;
            }

            // Handle $ as anchor only at the end of the sequence
            // Per POSIX: $ is special only at the end of the RE or subexpression
            if self.peek() == Some('$') {
                // Check if this $ would be at the end (nothing meaningful after it)
                let is_at_end = if self.pos + 1 >= self.chars.len() {
                    true // EOF after $
                } else {
                    let next = self.chars[self.pos + 1];
                    if self.is_ere {
                        // In ERE: end at | or ) or EOF
                        next == '|' || next == ')'
                    } else {
                        // In BRE: end at \| or \) or EOF
                        next == '\\'
                            && self.pos + 2 < self.chars.len()
                            && (self.chars[self.pos + 2] == '|' || self.chars[self.pos + 2] == ')')
                    }
                };

                if is_at_end {
                    self.consume();
                    nodes.push(RegexNode::EndAnchor);
                    continue;
                }
                // Otherwise fall through to parse_quantified which will treat $ as literal
            }

            nodes.push(self.parse_quantified()?);
        }

        if nodes.is_empty() {
            // Empty sequence - matches empty string
            Ok(RegexNode::Sequence(vec![]))
        } else if nodes.len() == 1 {
            // SAFETY: len() == 1 guarantees pop() returns Some
            Ok(nodes.pop().expect("vec has exactly 1 element"))
        } else {
            Ok(RegexNode::Sequence(nodes))
        }
    }

    /// Parse atom with optional quantifier
    fn parse_quantified(&mut self) -> Result<RegexNode> {
        let atom = self.parse_atom()?;

        // Check for quantifier
        match self.peek() {
            Some('*') => {
                self.consume();
                Ok(RegexNode::ZeroOrMore(Box::new(atom)))
            }
            Some('+') if self.is_ere => {
                self.consume();
                Ok(RegexNode::OneOrMore(Box::new(atom)))
            }
            Some('?') if self.is_ere => {
                self.consume();
                Ok(RegexNode::ZeroOrOne(Box::new(atom)))
            }
            Some('{') if self.is_ere => self.parse_bounded_repeat(atom),
            Some('\\') if !self.is_ere => {
                match self.peek_next() {
                    Some('+') => {
                        self.consume(); // \
                        self.consume(); // +
                        Ok(RegexNode::OneOrMore(Box::new(atom)))
                    }
                    Some('?') => {
                        self.consume(); // \
                        self.consume(); // ?
                        Ok(RegexNode::ZeroOrOne(Box::new(atom)))
                    }
                    Some('{') => {
                        self.consume(); // \
                        self.parse_bounded_repeat(atom)
                    }
                    _ => Ok(atom),
                }
            }
            _ => Ok(atom),
        }
    }

    /// Parse bounded repetition {m,n}
    fn parse_bounded_repeat(&mut self, atom: RegexNode) -> Result<RegexNode> {
        // Consume opening {
        self.consume();

        // Parse min
        let min = self.parse_number()?;

        let max = if self.peek() == Some(',') {
            self.consume(); // ,
                            // In BRE mode, check for \} to recognize {m,} case
                            // In ERE mode, check for } directly
            let is_unbounded = if self.is_ere {
                self.peek() == Some('}')
            } else {
                self.peek() == Some('\\') && self.peek_next() == Some('}')
            };
            if is_unbounded {
                None // {m,} = m or more
            } else {
                Some(self.parse_number()?) // {m,n}
            }
        } else {
            Some(min) // {m} = exactly m
        };

        // Consume closing } (in BRE it's \}, in ERE it's just })
        if !self.is_ere {
            // In BRE, expect \}
            if self.peek() != Some('\\') {
                return Err(SedError::parse("missing closing \\} in bounded repetition"));
            }
            self.consume(); // consume \
            if self.peek() != Some('}') {
                return Err(SedError::parse("missing closing \\} in bounded repetition"));
            }
            self.consume(); // consume }
        } else {
            // In ERE, expect just }
            if self.peek() != Some('}') {
                return Err(SedError::parse("missing closing } in bounded repetition"));
            }
            self.consume();
        }

        Ok(RegexNode::Repeat {
            node: Box::new(atom),
            min,
            max,
        })
    }

    /// Parse a number
    fn parse_number(&mut self) -> Result<usize> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.consume();
            } else {
                break;
            }
        }

        if start == self.pos {
            return Err(SedError::parse("expected number in repetition"));
        }

        let num_str: String = self.chars[start..self.pos].iter().collect();
        num_str
            .parse()
            .map_err(|_| SedError::parse("invalid number in repetition"))
    }

    /// Parse single atom (character, class, group, etc.)
    fn parse_atom(&mut self) -> Result<RegexNode> {
        match self.peek() {
            Some('.') => {
                self.consume();
                Ok(RegexNode::Any)
            }
            // Note: ^ is handled in parse_sequence - at start it's anchor, elsewhere it's literal
            // and goes through the normal literal path below since it's not in is_special_char
            // Note: $ is handled in parse_sequence - at end it's anchor, elsewhere it's literal
            // and goes through the normal literal path below since it's not in is_special_char
            Some('[') => self.parse_char_class(),
            Some('(') if self.is_ere => {
                self.consume(); // Consume opening '('
                self.parse_group()
            }
            Some('\\') => self.parse_escape(),
            Some(ch) if !self.is_special_char(ch) => {
                self.consume();
                Ok(RegexNode::Literal(ch))
            }
            Some(_ch) => Err(SedError::parse(format!(
                "unexpected special character at position {}",
                self.pos
            ))),
            None => Err(SedError::parse("unexpected end of pattern")),
        }
    }

    /// Check if character is special (needs escaping)
    /// Note: ']' is NOT special outside of character classes - it's only meaningful inside [...]
    /// Note: '^' is handled specially in parse_sequence (anchor at start, literal elsewhere)
    /// Note: '$' is handled specially in parse_sequence (anchor at end, literal elsewhere)
    fn is_special_char(&self, ch: char) -> bool {
        if self.is_ere {
            matches!(
                ch,
                '.' | '*' | '+' | '?' | '[' | '(' | ')' | '{' | '}' | '|' | '\\'
            )
        } else {
            matches!(ch, '.' | '*' | '[' | '\\')
        }
    }

    /// Parse escape sequence (\something)
    fn parse_escape(&mut self) -> Result<RegexNode> {
        self.consume(); // \

        match self.peek() {
            // Backreferences
            Some('1'..='9') => {
                // SAFETY: peek() returned Some, so consume() will too
                let digit = self.consume().expect("char exists after peek") as u8 - b'0';
                Ok(RegexNode::Backref(digit as usize))
            }

            // Groups in BRE: \( \)
            Some('(') if !self.is_ere => {
                self.consume();
                self.parse_group()
            }

            // Word boundaries (GNU extensions)
            Some('b') => {
                self.consume();
                Ok(RegexNode::WordBoundary)
            }
            Some('B') => {
                self.consume();
                Ok(RegexNode::NonWordBoundary)
            }
            Some('<') => {
                self.consume();
                Ok(RegexNode::StartWord)
            }
            Some('>') => {
                self.consume();
                Ok(RegexNode::EndWord)
            }

            // Word character class \w (GNU extension)
            Some('w') => {
                self.consume();
                // \w matches [[:alnum:]_] (alphanumeric + underscore)
                let mut charset = CharSet::new();
                charset.add_posix_class(PosixClass::Alnum);
                charset.add_char('_');
                Ok(RegexNode::CharClass(charset))
            }

            // Escape sequences
            Some('n') => {
                self.consume();
                Ok(RegexNode::Literal('\n'))
            }
            Some('t') => {
                self.consume();
                Ok(RegexNode::Literal('\t'))
            }
            Some('r') => {
                self.consume();
                Ok(RegexNode::Literal('\r'))
            }

            // Literal escape
            Some(_) => {
                // SAFETY: peek() returned Some, so consume() will too
                let ch = self.consume().expect("char exists after peek");
                Ok(RegexNode::Literal(ch))
            }

            None => Err(SedError::parse("trailing backslash at end of pattern")),
        }
    }

    /// Parse capturing group
    /// Note: opening paren should already be consumed (either '(' in ERE or '\(' in BRE)
    fn parse_group(&mut self) -> Result<RegexNode> {
        self.group_count += 1;
        let group_id = self.group_count;

        let content = self.parse_alternation()?;

        // Consume closing paren
        if self.is_ere {
            if self.peek() != Some(')') {
                return Err(SedError::parse("unclosed group: missing ')'"));
            }
            self.consume();
        } else {
            if self.peek() != Some('\\') || self.peek_next() != Some(')') {
                return Err(SedError::parse("unclosed group: missing '\\)'"));
            }
            self.consume(); // \
            self.consume(); // )
        }

        Ok(RegexNode::Group {
            id: group_id,
            node: Box::new(content),
        })
    }

    /// Parse character class [...]
    fn parse_char_class(&mut self) -> Result<RegexNode> {
        self.consume(); // [

        // Check for negation
        let negated = if self.peek() == Some('^') {
            self.consume();
            true
        } else {
            false
        };

        let mut set = CharSet::new();

        while let Some(ch) = self.peek() {
            if ch == ']' && (!set.ranges.is_empty() || !set.posix_classes.is_empty()) {
                // Closing ] - only if we've seen at least one element (range or POSIX class)
                self.consume();
                break;
            }

            if ch == '[' {
                match self.peek_next() {
                    Some(':') => {
                        // POSIX character class [:name:]
                        self.parse_posix_class(&mut set)?;
                        continue;
                    }
                    Some('.') => {
                        // POSIX collating symbol [.X.]
                        self.parse_collating_symbol(&mut set)?;
                        continue;
                    }
                    Some('=') => {
                        // POSIX equivalence class [=X=]
                        self.parse_equivalence_class(&mut set)?;
                        continue;
                    }
                    _ => {}
                }
            }

            {
                // Regular character(s) or range
                let chars = self.consume_class_chars()?;

                // Check if this is a range (a-z)
                // Ranges only work with single characters, not multi-char escapes
                if chars.len() == 1 && self.peek() == Some('-') && self.peek_next() != Some(']') {
                    // Range a-z
                    self.consume(); // -
                    let end_chars = self.consume_class_chars()?;
                    if end_chars.len() == 1 {
                        set.add_range(chars[0], end_chars[0]);
                    } else {
                        // Multi-char escape can't be end of range, treat as literals
                        set.add_char(chars[0]);
                        set.add_char('-');
                        for c in end_chars {
                            set.add_char(c);
                        }
                    }
                } else {
                    // Single character(s) - add all from escape
                    for c in chars {
                        set.add_char(c);
                    }
                }
            }
        }

        if negated {
            Ok(RegexNode::NegatedCharClass(set))
        } else {
            Ok(RegexNode::CharClass(set))
        }
    }

    /// Consume single character(s) from character class
    /// Returns a vector because some escapes like \" expand to both \ and "
    /// In POSIX mode, escape sequences (\t, \n, etc.) are NOT interpreted inside character classes
    fn consume_class_chars(&mut self) -> Result<Vec<char>> {
        match self.peek() {
            Some('\\') => {
                self.consume();
                match self.peek() {
                    Some('n') if !self.posix_mode => {
                        self.consume();
                        Ok(vec!['\n'])
                    }
                    Some('t') if !self.posix_mode => {
                        self.consume();
                        Ok(vec!['\t'])
                    }
                    Some('r') if !self.posix_mode => {
                        self.consume();
                        Ok(vec!['\r'])
                    }
                    Some('b') if !self.posix_mode => {
                        // \b inside character class is backspace (0x08), not word boundary
                        self.consume();
                        Ok(vec!['\x08'])
                    }
                    Some(ch) => {
                        // In POSIX mode, ALL escapes (including \t, \n) are treated as literal backslash + char
                        // In GNU mode, non-special escapes like \" include both backslash and the character
                        self.consume();
                        Ok(vec!['\\', ch])
                    }
                    None => Err(SedError::parse("unexpected end in character class")),
                }
            }
            Some(ch) => {
                self.consume();
                Ok(vec![ch])
            }
            None => Err(SedError::parse("unclosed character class")),
        }
    }

    /// Parse POSIX character class [:name:]
    fn parse_posix_class(&mut self, set: &mut CharSet) -> Result<()> {
        self.consume(); // [
        self.consume(); // :

        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == ':' {
                break;
            }
            self.consume();
        }

        let name: String = self.chars[start..self.pos].iter().collect();

        if self.peek() != Some(':') || self.peek_next() != Some(']') {
            return Err(SedError::parse(format!(
                "invalid POSIX character class: [:{}",
                name
            )));
        }

        self.consume(); // :
        self.consume(); // ]

        let class = PosixClass::from_name(&name).ok_or_else(|| {
            SedError::parse(format!("unknown POSIX character class: [:{}:]", name))
        })?;

        set.add_posix_class(class);
        Ok(())
    }

    /// Parse POSIX collating symbol [.X.]
    /// In C/POSIX locale, collating symbols are just literal characters
    /// Multi-character collating elements (like [.ch.] in Spanish) are
    /// treated as individual characters in C locale
    fn parse_collating_symbol(&mut self, set: &mut CharSet) -> Result<()> {
        self.consume(); // [
        self.consume(); // .

        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == '.' {
                break;
            }
            self.consume();
        }

        let symbol: String = self.chars[start..self.pos].iter().collect();

        if self.peek() != Some('.') || self.peek_next() != Some(']') {
            return Err(SedError::parse(format!(
                "invalid collating symbol: [.{}",
                symbol
            )));
        }

        self.consume(); // .
        self.consume(); // ]

        // In C locale, treat collating symbol as literal character(s)
        // For single-char symbols, this is simply that character
        // For multi-char symbols like "ch", add each character individually
        for ch in symbol.chars() {
            set.add_char(ch);
        }
        Ok(())
    }

    /// Parse POSIX equivalence class [=X=]
    /// In C/POSIX locale, equivalence classes just match the literal character
    /// (In other locales, they would match all characters with same primary collation weight)
    fn parse_equivalence_class(&mut self, set: &mut CharSet) -> Result<()> {
        self.consume(); // [
        self.consume(); // =

        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == '=' {
                break;
            }
            self.consume();
        }

        let equiv: String = self.chars[start..self.pos].iter().collect();

        if self.peek() != Some('=') || self.peek_next() != Some(']') {
            return Err(SedError::parse(format!(
                "invalid equivalence class: [={}",
                equiv
            )));
        }

        self.consume(); // =
        self.consume(); // ]

        // In C locale, equivalence class is just the literal character(s)
        for ch in equiv.chars() {
            set.add_char(ch);
        }
        Ok(())
    }
}

/// Parse BRE pattern
pub fn parse_bre(pattern: &str, posix_mode: bool) -> Result<CompiledRegex> {
    Parser::new(pattern, false, posix_mode).parse()
}

/// Parse ERE pattern
pub fn parse_ere(pattern: &str, posix_mode: bool) -> Result<CompiledRegex> {
    Parser::new(pattern, true, posix_mode).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_literal() {
        let regex = parse_bre("abc", false).unwrap();
        match regex.ast {
            RegexNode::Sequence(nodes) => {
                assert_eq!(nodes.len(), 3);
                assert_eq!(nodes[0], RegexNode::Literal('a'));
                assert_eq!(nodes[1], RegexNode::Literal('b'));
                assert_eq!(nodes[2], RegexNode::Literal('c'));
            }
            _ => panic!("expected Sequence"),
        }
    }

    #[test]
    fn test_parse_any() {
        let regex = parse_bre("a.b", false).unwrap();
        match regex.ast {
            RegexNode::Sequence(nodes) => {
                assert_eq!(nodes.len(), 3);
                assert_eq!(nodes[0], RegexNode::Literal('a'));
                assert_eq!(nodes[1], RegexNode::Any);
                assert_eq!(nodes[2], RegexNode::Literal('b'));
            }
            _ => panic!("expected Sequence"),
        }
    }

    #[test]
    fn test_parse_star() {
        let regex = parse_bre("a*", false).unwrap();
        match regex.ast {
            RegexNode::ZeroOrMore(node) => {
                assert_eq!(*node, RegexNode::Literal('a'));
            }
            _ => panic!("expected ZeroOrMore"),
        }
    }

    #[test]
    fn test_parse_anchors() {
        let regex = parse_bre("^a$", false).unwrap();
        match regex.ast {
            RegexNode::Sequence(nodes) => {
                assert_eq!(nodes.len(), 3);
                assert_eq!(nodes[0], RegexNode::StartAnchor);
                assert_eq!(nodes[1], RegexNode::Literal('a'));
                assert_eq!(nodes[2], RegexNode::EndAnchor);
            }
            _ => panic!("expected Sequence"),
        }
    }

    #[test]
    fn test_parse_group_bre() {
        let regex = parse_bre("\\(ab\\)", false).unwrap();
        match regex.ast {
            RegexNode::Group { id, node } => {
                assert_eq!(id, 1);
                match *node {
                    RegexNode::Sequence(nodes) => {
                        assert_eq!(nodes.len(), 2);
                    }
                    _ => panic!("expected Sequence in group"),
                }
            }
            _ => panic!("expected Group"),
        }
    }

    #[test]
    fn test_parse_backref() {
        let regex = parse_bre("\\(a\\)\\1", false).unwrap();
        match regex.ast {
            RegexNode::Sequence(nodes) => {
                assert_eq!(nodes.len(), 2);
                // First node is group
                matches!(nodes[0], RegexNode::Group { .. });
                // Second node is backref
                assert_eq!(nodes[1], RegexNode::Backref(1));
            }
            _ => panic!("expected Sequence"),
        }
    }

    #[test]
    fn test_parse_char_class() {
        let regex = parse_bre("[abc]", false).unwrap();
        match regex.ast {
            RegexNode::CharClass(set) => {
                assert!(set.matches('a'));
                assert!(set.matches('b'));
                assert!(set.matches('c'));
                assert!(!set.matches('d'));
            }
            _ => panic!("expected CharClass"),
        }
    }

    #[test]
    fn test_parse_char_class_range() {
        let regex = parse_bre("[a-z]", false).unwrap();
        match regex.ast {
            RegexNode::CharClass(set) => {
                assert!(set.matches('a'));
                assert!(set.matches('m'));
                assert!(set.matches('z'));
                assert!(!set.matches('A'));
                assert!(!set.matches('0'));
            }
            _ => panic!("expected CharClass"),
        }
    }

    #[test]
    fn test_parse_negated_class() {
        let regex = parse_bre("[^abc]", false).unwrap();
        match regex.ast {
            RegexNode::NegatedCharClass(set) => {
                // set contains [abc], so:
                // set.matches('a') == true (a is in set)
                // set.matches('d') == false (d is NOT in set)
                // But NegatedCharClass will invert this during matching
                assert!(set.matches('a')); // a is in set [abc]
                assert!(set.matches('b')); // b is in set [abc]
                assert!(!set.matches('d')); // d is NOT in set [abc]
                assert!(!set.matches('x')); // x is NOT in set [abc]
            }
            _ => panic!("expected NegatedCharClass"),
        }
    }

    #[test]
    fn test_parse_alternation_bre() {
        let regex = parse_bre("a\\|b", false).unwrap();
        match regex.ast {
            RegexNode::Alternation(alts) => {
                assert_eq!(alts.len(), 2);
                assert_eq!(alts[0], RegexNode::Literal('a'));
                assert_eq!(alts[1], RegexNode::Literal('b'));
            }
            _ => panic!("expected Alternation"),
        }
    }

    #[test]
    fn test_parse_ere_plus() {
        let regex = parse_ere("a+", false).unwrap();
        match regex.ast {
            RegexNode::OneOrMore(node) => {
                assert_eq!(*node, RegexNode::Literal('a'));
            }
            _ => panic!("expected OneOrMore"),
        }
    }

    #[test]
    fn test_parse_ere_question() {
        let regex = parse_ere("a?", false).unwrap();
        match regex.ast {
            RegexNode::ZeroOrOne(node) => {
                assert_eq!(*node, RegexNode::Literal('a'));
            }
            _ => panic!("expected ZeroOrOne"),
        }
    }

    #[test]
    fn test_parse_ere_group() {
        let regex = parse_ere("(ab)", false).unwrap();
        match regex.ast {
            RegexNode::Group { id, node } => {
                assert_eq!(id, 1);
                match *node {
                    RegexNode::Sequence(_) => {}
                    _ => panic!("expected Sequence in group"),
                }
            }
            _ => panic!("expected Group"),
        }
    }
}
