// Copyright (c) 2026 Red Authors
// License: MIT
//

use crate::errors::{Result, SedError};

use super::ast::Token;

/// Check if the current locale is UTF-8 based on environment variables
pub fn is_utf8_locale() -> bool {
    // Check LC_ALL first (highest priority), then LC_CTYPE, then LANG
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_CTYPE"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();

    let locale_lower = locale.to_lowercase();

    // On Windows, if no locale environment variables are set, default to UTF-8
    // Modern Windows uses UTF-8 by default for console and most applications
    #[cfg(windows)]
    if locale_lower.is_empty() {
        return true;
    }

    locale_lower.contains("utf-8") || locale_lower.contains("utf8")
}

/// Build a mapping from char positions in the converted string to byte positions in raw bytes
///
/// This is needed because `from_utf8_lossy()` converts invalid bytes to U+FFFD, changing
/// the byte positions. We need to know where each character came from in the raw bytes
/// to properly detect UTF-8 lead bytes vs continuation bytes for delimiter validation.
pub fn build_char_to_byte_mapping(input: &str, raw_bytes: &[u8]) -> Vec<usize> {
    let mut mapping = Vec::with_capacity(input.chars().count());
    let mut byte_pos = 0;

    for ch in input.chars() {
        // Record the byte position for this char
        mapping.push(byte_pos);

        if ch == '\u{FFFD}' {
            // Replacement character - came from invalid byte(s)
            // In from_utf8_lossy, each invalid byte becomes one U+FFFD
            // BUT: an incomplete sequence at the end may consume multiple bytes
            // For simplicity, assume single invalid byte per U+FFFD (most common case)
            byte_pos += 1;
        } else {
            // Valid UTF-8 character - advance by its UTF-8 length in the source
            byte_pos += ch.len_utf8();
        }

        // Safety: don't go past raw bytes
        if byte_pos > raw_bytes.len() {
            byte_pos = raw_bytes.len();
        }
    }

    mapping
}

/// State machine for the lexer
///
/// This enum represents the lexer's parsing state, which determines
/// how the next token should be interpreted.
///
/// ## State Transitions
///
/// ```text
/// Normal
///   ├─ 's' → ExpectingSubstitution
///   ├─ 'a'/'i'/'c' (with body) → ExpectingText('a'|'i'|'c')
///   ├─ 'e' → ExpectingExecute
///   ├─ 'r'/'R'/'w'/'W' → ExpectingFilename('r'|'R'|'w'|'W')
///   └─ 'b'/'t'/'T'/':' → EnterLabelContext
///
/// EnterLabelContext → InLabelContext (on next token)
///
/// InLabelContext
///   ├─ newline/';'/'}' → Normal
///   └─ other chars → stay in InLabelContext
///
/// ExpectingSubstitution → Normal (after parsing body)
/// ExpectingText → Normal (after parsing text)
/// ExpectingExecute → Normal (after parsing execute body)
/// ExpectingFilename → Normal (after parsing filename)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerState {
    /// Default state - parsing regular commands and addresses
    Normal,

    /// Transition state - will enter InLabelContext on next token
    /// Set when 'b', 't', 'T', or ':' is encountered
    EnterLabelContext,

    /// Parsing label name after 'b', 't', 'T', or ':'
    /// In this state, commands like 's', 'a', 'y' are treated as label characters
    /// instead of triggering special parsing
    InLabelContext,

    /// Expecting substitution body after 's' command
    /// Next token will be parsed as SubstitutionBody
    ExpectingSubstitution,

    /// Expecting text body after 'a', 'i', or 'c' command
    /// The char indicates which command: 'a' (append), 'i' (insert), 'c' (change)
    ExpectingText(char),

    /// Expecting execute body after 'e' command
    /// Next token will be parsed as ExecuteBody
    ExpectingExecute,

    /// Expecting filename after 'r', 'R', 'w', or 'W' command
    /// The char indicates which command
    ExpectingFilename(char),
}

/// Lexer for tokenizing command scripts
pub struct Lexer<'a> {
    pub(crate) input: &'a str,
    pub(crate) position: usize,
    pub(crate) current_char: Option<char>,
    /// Current state of the lexer state machine
    pub(crate) state: LexerState,
    /// POSIX mode flag (configuration, not state)
    pub(crate) posix: bool,
    /// UTF-8 locale flag for multibyte delimiter detection
    pub(crate) utf8_locale: bool,
    /// Original raw bytes (before lossy UTF-8 conversion)
    /// Used to detect UTF-8 lead bytes vs continuation bytes for delimiter validation
    pub(crate) raw_input: Option<&'a [u8]>,
    /// Mapping from char position to byte position in raw_input
    /// Built during construction when raw_input is provided
    pub(crate) char_to_byte: Vec<usize>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self::new_with_posix(input, false)
    }

    pub fn new_with_posix(input: &'a str, posix: bool) -> Self {
        let mut lexer = Self {
            input,
            position: 0,
            current_char: None,
            state: LexerState::Normal,
            posix,
            utf8_locale: is_utf8_locale(),
            raw_input: None,
            char_to_byte: Vec::new(),
        };
        lexer.current_char = input.chars().next();
        lexer
    }

    /// Create a new lexer with raw bytes for accurate multibyte delimiter detection
    ///
    /// The `raw_bytes` should be the original bytes before lossy UTF-8 conversion.
    /// This allows detecting UTF-8 lead bytes (0xC0-0xFF) vs continuation bytes (0x80-0xBF)
    /// which is needed for proper multibyte delimiter error messages.
    pub fn new_with_raw_bytes(input: &'a str, raw_bytes: &'a [u8], posix: bool) -> Self {
        // Build char-to-byte mapping by scanning both strings in parallel
        let char_to_byte = build_char_to_byte_mapping(input, raw_bytes);

        let mut lexer = Self {
            input,
            position: 0,
            current_char: None,
            state: LexerState::Normal,
            posix,
            utf8_locale: is_utf8_locale(),
            raw_input: Some(raw_bytes),
            char_to_byte,
        };
        lexer.current_char = input.chars().next();
        lexer
    }

    /// Get the original raw byte at the current char position
    /// Returns None if raw_input is not available or position is out of bounds
    fn current_raw_byte(&self) -> Option<u8> {
        let raw = self.raw_input?;
        let byte_pos = self.char_to_byte.get(self.position).copied()?;
        raw.get(byte_pos).copied()
    }

    /// Check if a delimiter byte is a UTF-8 lead byte (starts multibyte sequence)
    /// UTF-8 lead bytes are 0xC0-0xFF (binary 11xxxxxx)
    fn is_utf8_lead_byte(byte: u8) -> bool {
        byte >= 0xC0
    }

    /// Get raw bytes for a range of char positions (start inclusive, end exclusive)
    /// Returns None if raw_input is not available
    fn get_raw_bytes_range(&self, start_char: usize, end_char: usize) -> Option<Vec<u8>> {
        let raw = self.raw_input?;
        let start_byte = self.char_to_byte.get(start_char).copied()?;
        // For end, if end_char is at or past the mapping length, use raw.len()
        let end_byte = if end_char >= self.char_to_byte.len() {
            raw.len()
        } else {
            self.char_to_byte.get(end_char).copied()?
        };
        Some(raw[start_byte..end_byte].to_vec())
    }

    /// Check if the current byte position is inside a multibyte sequence
    /// This is used to determine if a `]` or other special character should be
    /// treated as part of a multibyte character rather than its ASCII meaning.
    ///
    /// Uses mbcs module to properly handle stateful encodings like Shift-JIS.
    fn is_inside_mb_sequence(&self, mb_state: &mut crate::mbcs::MbState) -> bool {
        if !crate::mbcs::is_multibyte_locale() {
            return false;
        }
        if let Some(raw_byte) = self.current_raw_byte() {
            mb_state.is_mb_char(raw_byte)
        } else {
            false
        }
    }

    pub fn next_token(&mut self) -> Result<Token> {
        // Main loop to handle backslash-newline continuation without recursion
        loop {
            // State machine dispatch: handle states that expect specific token types
            match self.state {
                LexerState::EnterLabelContext => {
                    // Transition to label context
                    self.state = LexerState::InLabelContext;
                    // Continue to parse the token in label context
                }
                LexerState::ExpectingSubstitution => {
                    self.state = LexerState::Normal;
                    return self.parse_substitution_body();
                }
                LexerState::ExpectingText(kind) => {
                    self.state = LexerState::Normal;
                    return self.parse_aic_body(kind);
                }
                LexerState::ExpectingExecute => {
                    self.state = LexerState::Normal;
                    return self.parse_e_body();
                }
                LexerState::ExpectingFilename(cmd) => {
                    // In POSIX mode, 'R' and 'W' are GNU extensions
                    // Don't try to parse filename, let parser handle the error
                    if self.posix && (cmd == 'R' || cmd == 'W') {
                        self.state = LexerState::Normal;
                        // Continue to parse next token normally
                    } else {
                        self.state = LexerState::Normal;
                        return self.parse_filename_literal();
                    }
                }
                LexerState::Normal | LexerState::InLabelContext => {
                    // Continue to token parsing below
                }
            }

            self.skip_whitespace();

            // Handle backslash-newline continuation in loop instead of recursion
            if self.current_char == Some('\\') && self.peek() == Some('\n') {
                self.advance();
                self.advance();
                self.skip_whitespace();
                continue; // Loop back to start instead of recursive call
            }

            // Break out of loop and process the token
            break;
        }

        match self.current_char {
            None => Ok(Token::Eof),
            Some('\n') => {
                // Exit label context on newline
                if self.state == LexerState::InLabelContext {
                    self.state = LexerState::Normal;
                }
                self.advance();
                Ok(Token::Newline)
            }
            Some('#') => self.parse_comment(),
            Some('$') => {
                self.advance();
                Ok(Token::Dollar)
            }
            Some(',') => {
                self.advance();
                Ok(Token::Comma)
            }
            Some(';') => {
                // Exit label context on semicolon
                if self.state == LexerState::InLabelContext {
                    self.state = LexerState::Normal;
                }
                self.advance();
                Ok(Token::Semicolon)
            }
            Some('!') => {
                self.advance();
                Ok(Token::Bang)
            }
            Some('{') => {
                self.advance();
                Ok(Token::OpenBrace)
            }
            Some('}') => {
                // Exit label context on close brace
                if self.state == LexerState::InLabelContext {
                    self.state = LexerState::Normal;
                }
                self.advance();
                Ok(Token::CloseBrace)
            }
            Some('+') => self.parse_plus_offset(),
            Some('~') => self.parse_tilde_step(),
            Some('/') => self.parse_regex_addr(),
            Some('\\') => {
                // Check if next char is a valid regex delimiter
                // In GNU sed, \c can be used where c is any non-alphanumeric character
                // BUT exclude BRE metacharacters: ( ) { } + ? which have special meaning when escaped
                // Note: | is allowed as alternative delimiter (\|pattern|) despite being a BRE alternation operator
                if let Some(next) = self.peek() {
                    if !next.is_ascii_alphanumeric()
                        && next != '\\'
                        && next != '('
                        && next != ')'
                        && next != '{'
                        && next != '}'
                        && next != '+'
                        && next != '?'
                    {
                        return self.parse_alt_regex_addr();
                    }
                }
                // Otherwise fall through to other backslash handling
                self.advance();
                Ok(Token::Command('\\'))
            }
            Some(c) if c.is_ascii_digit() => self.parse_number(),
            Some('=') => {
                self.advance();
                Ok(Token::Command('='))
            }
            Some('s') => {
                self.advance();
                // In label context, 's' is part of a label name, not a substitution command
                if self.state != LexerState::InLabelContext {
                    // Set state to expect substitution if there's a next character
                    // The substitution body parser will validate if it's a valid delimiter
                    if self.current_char.is_some() {
                        self.state = LexerState::ExpectingSubstitution;
                    }
                }
                Ok(Token::Command('s'))
            }
            Some('y') => {
                // In label context, 'y' is part of a label name, not a transliterate command
                if self.state == LexerState::InLabelContext {
                    self.advance();
                    Ok(Token::Command('y'))
                } else {
                    // Check if next char is a valid delimiter (any non-whitespace char)
                    // GNU sed allows alphanumeric delimiters for y command
                    self.advance();
                    if let Some(c) = self.current_char {
                        if c != ' ' && c != '\t' && c != '\r' && c != '\n' {
                            // Parse translate body immediately
                            return self.parse_translate_body();
                        }
                    }
                    // No delimiter found or EOF - this is an error
                    Err(SedError::parse_at(
                        "unterminated 'y' command",
                        self.position,
                    ))
                }
            }
            Some('a') => {
                self.advance();
                // Check if this command has a text body (unless in label context)
                if self.state != LexerState::InLabelContext && self.check_text_command_has_body() {
                    self.state = LexerState::ExpectingText('a');
                }
                Ok(Token::Command('a'))
            }
            Some('i') => {
                self.advance();
                // Check if this command has a text body (unless in label context)
                if self.state != LexerState::InLabelContext && self.check_text_command_has_body() {
                    self.state = LexerState::ExpectingText('i');
                }
                Ok(Token::Command('i'))
            }
            Some('c') => {
                self.advance();
                // Check if this command has a text body (unless in label context)
                if self.state != LexerState::InLabelContext && self.check_text_command_has_body() {
                    self.state = LexerState::ExpectingText('c');
                }
                Ok(Token::Command('c'))
            }
            Some('r') => {
                self.advance();
                if self.state != LexerState::InLabelContext {
                    self.state = LexerState::ExpectingFilename('r');
                }
                Ok(Token::Command('r'))
            }
            Some('R') => {
                self.advance();
                if self.state != LexerState::InLabelContext {
                    self.state = LexerState::ExpectingFilename('R');
                }
                Ok(Token::Command('R'))
            }
            Some('w') => {
                self.advance();
                if self.state != LexerState::InLabelContext {
                    self.state = LexerState::ExpectingFilename('w');
                }
                Ok(Token::Command('w'))
            }
            Some('W') => {
                self.advance();
                if self.state != LexerState::InLabelContext {
                    self.state = LexerState::ExpectingFilename('W');
                }
                Ok(Token::Command('W'))
            }
            Some('e') => {
                self.advance();
                // Check if 'e' is standalone command or part of word/label
                // Only treat as execute command if NOT in label context
                if self.state != LexerState::InLabelContext {
                    match self.current_char {
                        None | Some('\n') | Some(' ') | Some('\t') => {
                            // Standalone 'e' - this is a command
                            self.state = LexerState::ExpectingExecute;
                        }
                        Some(';') | Some('}') => {
                            // 'e;' or 'e}' - could be label, don't set state
                        }
                        _ => {
                            // 'e' followed by other chars like 'etrue'
                            self.state = LexerState::ExpectingExecute;
                        }
                    }
                }
                Ok(Token::Command('e'))
            }
            Some(c) if c.is_alphabetic() => {
                self.advance();
                // Set label context for b/t/T commands
                match c {
                    'b' | 't' | 'T' => {
                        self.state = LexerState::EnterLabelContext;
                    }
                    _ => {}
                }
                Ok(Token::Command(c))
            }
            Some(':') => {
                self.advance();
                // Enter label context after ':' (label definition)
                self.state = LexerState::EnterLabelContext;
                Ok(Token::SubstitutionDelim(':'))
            }
            Some(c) => {
                self.advance();
                Ok(Token::SubstitutionDelim(c))
            }
        }
    }

    /// Check if a text command (a/i/c) has a body following it
    ///
    /// Looks ahead from current position to determine if non-whitespace,
    /// non-delimiter characters follow, indicating a text body.
    fn check_text_command_has_body(&self) -> bool {
        let mut iter = self.input.chars();
        let mut j = 0usize;
        // Skip to current position
        while j < self.position {
            iter.next();
            j += 1;
        }
        // Check if next non-whitespace char indicates a body
        while let Some(ch) = iter.next() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                continue; // Skip whitespace
            }
            // Any non-whitespace character that's not a delimiter indicates a body
            return ch != '\n' && ch != ';' && ch != '}';
        }
        false
    }

    fn advance(&mut self) {
        self.position += 1;
        self.current_char = self.input.chars().nth(self.position);
    }
    fn peek(&self) -> Option<char> {
        self.input.chars().nth(self.position + 1)
    }
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current_char {
            if c == ' ' || c == '\t' || c == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }
    fn parse_comment(&mut self) -> Result<Token> {
        let mut comment = String::new();
        self.advance();
        while let Some(c) = self.current_char {
            if c == '\n' {
                break;
            }
            comment.push(c);
            self.advance();
        }
        Ok(Token::Comment(comment))
    }
    fn parse_regex_addr(&mut self) -> Result<Token> {
        let mut pattern = String::new();
        let mut in_bracket = false; // Track if we're inside [...]
        let mut mb_state = crate::mbcs::MbState::new(); // Track multibyte sequence state
        self.advance();

        // Check for empty regex pattern // (reuses last regex)
        if let Some('/') = self.current_char {
            self.advance();
            // Parse optional modifiers (I, M, i, m) after the closing /
            // BUT: if followed by backslash, it's likely a command (like 'i\'), not a modifier
            let mut modifiers = String::new();
            while let Some(m) = self.current_char {
                match m {
                    'I' | 'M' | 'i' | 'm' => {
                        // Check if this could be a command instead of a modifier
                        // If next char is '\', it's probably 'i\' command, not 'i' modifier
                        if self.peek() == Some('\\') {
                            break;
                        }
                        modifiers.push(m);
                        self.advance();
                    }
                    _ => break,
                }
            }
            return Ok(Token::RegexAddr(pattern, modifiers));
        }

        while let Some(c) = self.current_char {
            if c == '/' && !in_bracket {
                self.advance();
                // Parse optional modifiers (I, M, i, m) after the closing /
                // BUT: if followed by backslash, it's likely a command (like 'i\'), not a modifier
                let mut modifiers = String::new();
                while let Some(m) = self.current_char {
                    match m {
                        'I' | 'M' | 'i' | 'm' => {
                            // Check if this could be a command instead of a modifier
                            // If next char is '\', it's probably 'i\' command, not 'i' modifier
                            if self.peek() == Some('\\') {
                                break;
                            }
                            modifiers.push(m);
                            self.advance();
                        }
                        _ => break,
                    }
                }
                return Ok(Token::RegexAddr(pattern, modifiers));
            }
            if c == '\\' {
                self.advance();
                if let Some(escaped) = self.current_char {
                    match escaped {
                        '/' => pattern.push('/'),
                        '\\' => {
                            // Preserve both backslashes for the regex engine
                            // Input \\ should produce \\ in pattern to match literal backslash
                            pattern.push('\\');
                            pattern.push('\\');
                        }
                        other => {
                            pattern.push('\\');
                            pattern.push(other);
                        }
                    }
                    self.advance();
                }
            } else {
                // Track bracket state for character classes
                if c == '[' {
                    pattern.push(c);
                    self.advance();

                    if in_bracket {
                        // Check for POSIX bracket expressions [:class:], [.collating.], [=equiv=]
                        if let Some(next) = self.current_char {
                            if next == ':' || next == '.' || next == '=' {
                                let close_char = next;
                                pattern.push(next);
                                self.advance();

                                // Look for closing sequence
                                while let Some(ch) = self.current_char {
                                    pattern.push(ch);
                                    self.advance();
                                    if ch == close_char {
                                        if let Some(']') = self.current_char {
                                            pattern.push(']');
                                            self.advance();
                                            break;
                                        }
                                    }
                                    if self.current_char.is_none() {
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        in_bracket = true;
                    }
                } else if c == ']' && in_bracket {
                    // Check if this ']' is part of a multibyte sequence
                    // In Shift-JIS, 0x5D (']') can be the second byte of a multibyte char
                    if self.is_inside_mb_sequence(&mut mb_state) {
                        // Part of multibyte sequence - don't close bracket
                        pattern.push(c);
                        self.advance();
                    } else {
                        // Not part of multibyte sequence - close bracket
                        in_bracket = false;
                        pattern.push(c);
                        self.advance();
                    }
                } else {
                    // Update mb_state for all other characters
                    self.is_inside_mb_sequence(&mut mb_state);
                    pattern.push(c);
                    self.advance();
                }
            }
        }
        Err(SedError::parse_at(
            "unterminated address regex",
            self.position,
        ))
    }
    fn parse_alt_regex_addr(&mut self) -> Result<Token> {
        self.advance();
        let delimiter = self
            .current_char
            .ok_or_else(|| SedError::parse_at("expected delimiter character", self.position))?;

        // In UTF-8 locale, reject multibyte delimiters
        if self.utf8_locale {
            if let Some(raw_byte) = self.current_raw_byte() {
                // Have raw bytes - check if it's a UTF-8 lead byte
                if Self::is_utf8_lead_byte(raw_byte) {
                    return Err(SedError::parse_at(
                        "delimiter character is not a single-byte character",
                        self.position,
                    ));
                }
            } else if delimiter.len_utf8() > 1 && delimiter != '\u{FFFD}' {
                // No raw bytes - fall back to checking char length
                return Err(SedError::parse_at(
                    "delimiter character is not a single-byte character",
                    self.position,
                ));
            }
        }

        self.advance();
        let mut pattern = String::new();
        let mut in_bracket = false; // Track if we're inside [...]
        let mut mb_state = crate::mbcs::MbState::new(); // Track multibyte sequence state

        // Check for empty regex pattern (reuses last regex)
        if let Some(c) = self.current_char {
            if c == delimiter {
                self.advance();
                // Parse optional modifiers (I, M, i, m) after the closing delimiter
                let mut modifiers = String::new();
                while let Some(m) = self.current_char {
                    match m {
                        'I' | 'M' | 'i' | 'm' => {
                            modifiers.push(m);
                            self.advance();
                        }
                        _ => break,
                    }
                }
                return Ok(Token::RegexAddr(pattern, modifiers));
            }
        }

        while let Some(c) = self.current_char {
            if c == delimiter && !in_bracket {
                self.advance();
                // Parse optional modifiers (I, M, i, m) after the closing delimiter
                let mut modifiers = String::new();
                while let Some(m) = self.current_char {
                    match m {
                        'I' | 'M' | 'i' | 'm' => {
                            modifiers.push(m);
                            self.advance();
                        }
                        _ => break,
                    }
                }
                return Ok(Token::RegexAddr(pattern, modifiers));
            }
            if c == '\\' {
                pattern.push(c);
                self.advance();
                if let Some(escaped) = self.current_char {
                    pattern.push(escaped);
                    self.advance();
                }
            } else {
                // Track bracket state for character classes
                if c == '[' {
                    pattern.push(c);
                    self.advance();

                    if in_bracket {
                        // Check for POSIX bracket expressions [:class:], [.collating.], [=equiv=]
                        if let Some(next) = self.current_char {
                            if next == ':' || next == '.' || next == '=' {
                                let close_char = next;
                                pattern.push(next);
                                self.advance();

                                // Look for closing sequence
                                while let Some(ch) = self.current_char {
                                    pattern.push(ch);
                                    self.advance();
                                    if ch == close_char {
                                        if let Some(']') = self.current_char {
                                            pattern.push(']');
                                            self.advance();
                                            break;
                                        }
                                    }
                                    if self.current_char.is_none() {
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        in_bracket = true;
                    }
                } else if c == ']' && in_bracket {
                    // Check if this ']' is part of a multibyte sequence
                    // In Shift-JIS, 0x5D (']') can be the second byte of a multibyte char
                    if self.is_inside_mb_sequence(&mut mb_state) {
                        // Part of multibyte sequence - don't close bracket
                        pattern.push(c);
                        self.advance();
                    } else {
                        // Not part of multibyte sequence - close bracket
                        in_bracket = false;
                        pattern.push(c);
                        self.advance();
                    }
                } else {
                    // Update mb_state for all other characters
                    self.is_inside_mb_sequence(&mut mb_state);
                    pattern.push(c);
                    self.advance();
                }
            }
        }
        Err(SedError::parse_at(
            "unterminated address regex",
            self.position,
        ))
    }
    fn parse_substitution_body(&mut self) -> Result<Token> {
        let mut body = String::new();
        // Track start position for raw bytes extraction
        let start_pos = self.position;

        let delim = match self.current_char {
            Some(d) => d,
            None => {
                return Err(SedError::parse_at(
                    "unterminated 's' command",
                    self.position,
                ));
            }
        };

        // In UTF-8 locale, reject multibyte delimiters
        // Check raw bytes if available (more accurate), otherwise use char length
        if self.utf8_locale {
            if let Some(raw_byte) = self.current_raw_byte() {
                // Have raw bytes - check if it's a UTF-8 lead byte (starts multibyte sequence)
                if Self::is_utf8_lead_byte(raw_byte) {
                    return Err(SedError::parse_at(
                        "delimiter character is not a single-byte character",
                        self.position,
                    ));
                }
            } else if delim.len_utf8() > 1 && delim != '\u{FFFD}' {
                // No raw bytes - fall back to checking char length
                // U+FFFD is allowed because it may represent a single invalid byte
                return Err(SedError::parse_at(
                    "delimiter character is not a single-byte character",
                    self.position,
                ));
            }
        }

        body.push(delim);
        self.advance();
        let mut in_class = false;
        let track_class = delim != '[';
        loop {
            match self.current_char {
                None => {
                    return Err(SedError::parse_at(
                        "unterminated 's' command",
                        self.position,
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.current_char {
                        Some('\n') => {
                            // In sed, \<newline> in replacement string inserts a literal newline
                            // Preserve the backslash-newline sequence for parse_replacement to handle
                            body.push('\\');
                            body.push('\n');
                            self.advance();
                        }
                        Some(c) => {
                            body.push('\\');
                            body.push(c);
                            self.advance();
                        }
                        None => {
                            body.push('\\');
                        }
                    }
                }
                Some('[') if track_class => {
                    body.push('[');
                    self.advance();

                    if in_class {
                        // Inside bracket expression, check for POSIX bracket expressions
                        // [:class:], [.collating.], [=equiv=]
                        if let Some(next) = self.current_char {
                            if next == ':' || next == '.' || next == '=' {
                                let close_char = next;
                                body.push(next);
                                self.advance();

                                // Look for closing sequence (e.g., :] or .] or =])
                                let mut found_close = false;
                                while let Some(c) = self.current_char {
                                    body.push(c);
                                    self.advance();

                                    if c == close_char {
                                        if let Some(']') = self.current_char {
                                            body.push(']');
                                            self.advance();
                                            found_close = true;
                                            break;
                                        }
                                    }

                                    // If we hit None, break and let outer loop handle error
                                    if self.current_char.is_none() {
                                        break;
                                    }
                                }

                                // If we didn't find the closing sequence, the bracket is unterminated
                                if !found_close && self.current_char.is_none() {
                                    return Err(SedError::parse_at(
                                        "unterminated 's' command",
                                        self.position,
                                    ));
                                }
                            } else {
                                in_class = true; // Nested bracket?
                            }
                        }
                    } else {
                        in_class = true;
                    }
                }
                Some(']') if track_class && in_class => {
                    in_class = false;
                    body.push(']');
                    self.advance();
                }
                Some(c) if c == delim && !in_class => {
                    body.push(c);
                    self.advance();
                    break;
                }
                Some(c) => {
                    body.push(c);
                    self.advance();
                }
            }
        }
        loop {
            match self.current_char {
                None => {
                    return Err(SedError::parse_at(
                        "unterminated 's' command",
                        self.position,
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.current_char {
                        Some('\n') => {
                            // In sed, \<newline> in replacement string inserts a literal newline
                            // Preserve the backslash-newline sequence for parse_replacement to handle
                            body.push('\\');
                            body.push('\n');
                            self.advance();
                        }
                        Some(c) => {
                            body.push('\\');
                            body.push(c);
                            self.advance();
                        }
                        None => {
                            body.push('\\');
                        }
                    }
                }
                Some(c) if c == delim => {
                    body.push(c);
                    self.advance();
                    break;
                }
                Some(c) => {
                    body.push(c);
                    self.advance();
                }
            }
        }
        loop {
            match self.current_char {
                None => break,
                Some('\n') | Some('}') | Some(';') => break,
                // Comment after flags (GNU sed allows # or space+# after flags)
                Some('#') => break,
                Some(' ') | Some('\t') => {
                    // Skip whitespace and check for comment
                    self.advance();
                    if matches!(self.current_char, Some('#')) {
                        break;
                    }
                    // Whitespace not followed by comment - continue parsing flags
                    continue;
                }
                Some('\\') => {
                    self.advance();
                    match self.current_char {
                        Some('\n') => {
                            self.advance();
                        }
                        Some(c) => {
                            body.push('\\');
                            body.push(c);
                            self.advance();
                        }
                        None => {
                            body.push('\\');
                        }
                    }
                }
                Some('\r') => {
                    // Carriage return - check if followed by newline
                    self.advance();
                    if matches!(self.current_char, Some('\n')) {
                        // \r\n is acceptable line ending, break here (don't include \r in body)
                        break;
                    } else {
                        // \r alone is an error
                        return Err(SedError::parse_at("unknown option to 's'", self.position));
                    }
                }
                Some(c) => {
                    body.push(c);
                    self.advance();
                }
            }
        }

        // Extract raw bytes for the body if available
        let raw_bytes = self.get_raw_bytes_range(start_pos, self.position);

        Ok(Token::SubstitutionBody(body, raw_bytes))
    }
    fn parse_translate_body(&mut self) -> Result<Token> {
        let mut body = String::new();
        // Track start position for raw bytes extraction
        let start_pos = self.position;

        let delim = match self.current_char {
            Some(d) => d,
            None => {
                return Err(SedError::parse_at(
                    "unterminated 'y' command",
                    self.position,
                ));
            }
        };

        // In UTF-8 locale, reject multibyte delimiters (same logic as s command)
        if self.utf8_locale {
            if let Some(raw_byte) = self.current_raw_byte() {
                // Have raw bytes - check if it's a UTF-8 lead byte
                if Self::is_utf8_lead_byte(raw_byte) {
                    return Err(SedError::parse_at(
                        "delimiter character is not a single-byte character",
                        self.position,
                    ));
                }
            } else if delim.len_utf8() > 1 && delim != '\u{FFFD}' {
                // No raw bytes - fall back to checking char length
                return Err(SedError::parse_at(
                    "delimiter character is not a single-byte character",
                    self.position,
                ));
            }
        }

        body.push(delim);
        self.advance();
        loop {
            match self.current_char {
                None => {
                    return Err(SedError::parse_at(
                        "unterminated 'y' command",
                        self.position,
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.current_char {
                        Some(c) => {
                            body.push('\\');
                            body.push(c);
                            self.advance();
                        }
                        None => {
                            body.push('\\');
                        }
                    }
                }
                Some(c) if c == delim => {
                    body.push(c);
                    self.advance();
                    break;
                }
                Some(c) => {
                    body.push(c);
                    self.advance();
                }
            }
        }
        loop {
            match self.current_char {
                None => {
                    return Err(SedError::parse_at(
                        "unterminated 'y' command",
                        self.position,
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.current_char {
                        Some(c) => {
                            body.push('\\');
                            body.push(c);
                            self.advance();
                        }
                        None => {
                            body.push('\\');
                        }
                    }
                }
                Some(c) if c == delim => {
                    body.push(c);
                    self.advance();
                    break;
                }
                Some(c) => {
                    body.push(c);
                    self.advance();
                }
            }
        }

        // Extract raw bytes for the body if available
        let raw_bytes = self.get_raw_bytes_range(start_pos, self.position);

        Ok(Token::TranslateBody(body, raw_bytes))
    }
    fn parse_aic_body(&mut self, kind: char) -> Result<Token> {
        // Skip leading whitespace
        while let Some(c) = self.current_char {
            if c == ' ' || c == '\t' {
                self.advance();
            } else {
                break;
            }
        }

        // Two formats supported:
        // 1. GNU format: c\ (backslash-newline) followed by text
        // 2. BSD format: cText (text immediately after command)
        let mut text = String::new();

        match self.current_char {
            Some('\\') => {
                // Check for backslash-newline (GNU format)
                self.advance();
                if self.current_char == Some('\n') {
                    // GNU format: read text from next line(s)
                    // Support multiline text: lines ending with \ continue to next line
                    self.advance();
                    loop {
                        // Read characters until end of line
                        let mut line = String::new();
                        while let Some(c) = self.current_char {
                            if c == '\n' {
                                break;
                            }
                            line.push(c);
                            self.advance();
                        }

                        // Check if line ends with backslash (continuation)
                        let continues = line.ends_with('\\');
                        if continues {
                            // Remove trailing backslash and add the line
                            line.pop();
                            text.push_str(&line);
                            text.push('\n'); // Add literal newline between continued lines

                            // Move past the newline for next iteration
                            if self.current_char == Some('\n') {
                                self.advance();
                            } else {
                                // End of input after backslash
                                break;
                            }
                        } else {
                            // Last line (no continuation)
                            text.push_str(&line);
                            break;
                        }
                    }
                    // Explicit text (even empty) - return Some
                    return Ok(match kind {
                        'a' => Token::AppendBody(Some(text)),
                        'i' => Token::InsertBody(Some(text)),
                        _ => Token::ChangeBody(Some(text)),
                    });
                } else if self.current_char.is_none() {
                    // Backslash followed by EOF - unterminated command
                    // In POSIX mode, this is an error (incomplete command)
                    // In GNU mode, treat as unterminated (no text to add)
                    if self.posix {
                        return Err(SedError::parse_at("incomplete command", self.position));
                    }
                    // GNU mode: return None to indicate unterminated
                    return Ok(match kind {
                        'a' => Token::AppendBody(None),
                        'i' => Token::InsertBody(None),
                        _ => Token::ChangeBody(None),
                    });
                } else {
                    // Backslash followed by text (GNU extension: a\text)
                    // In POSIX mode, this is an error - backslash-newline required
                    if self.posix {
                        return Err(SedError::parse_at(
                            "expected \\ after 'a', 'c' or 'i'",
                            self.position,
                        ));
                    }
                    // GNU extension: text immediately after backslash
                    // Read until newline only (semicolons and braces are part of text)
                    while let Some(c) = self.current_char {
                        if c == '\n' {
                            break;
                        }
                        text.push(c);
                        self.advance();
                    }
                }
            }
            Some('\n') | Some(';') | Some('}') | None => {
                // Empty text body - valid for a/i/c commands
                // Don't consume the delimiter
            }
            _ => {
                // Text immediately after command (BSD/GNU extension format, like 'ifoo')
                // In POSIX mode, this is an error - backslash is required
                if self.posix {
                    return Err(SedError::parse_at(
                        "expected \\ after 'a', 'c' or 'i'",
                        self.position + 1,
                    ));
                }
                // GNU extension: text immediately after command
                while let Some(c) = self.current_char {
                    if c == '\n' || c == ';' || c == '}' {
                        break;
                    }
                    text.push(c);
                    self.advance();
                }
            }
        }

        Ok(match kind {
            'a' => Token::AppendBody(Some(text)),
            'i' => Token::InsertBody(Some(text)),
            _ => Token::ChangeBody(Some(text)),
        })
    }

    fn parse_e_body(&mut self) -> Result<Token> {
        // For 'e' command: read everything until newline/semicolon/close brace
        // Unlike a/i/c, there's NO backslash requirement
        // Reads ALL characters including spaces, semicolons become part of command!
        while let Some(c) = self.current_char {
            if c == ' ' || c == '\t' {
                self.advance();
            } else {
                break;
            }
        }
        let mut text = String::new();
        while let Some(c) = self.current_char {
            match c {
                '\n' | '}' => break, // Newline or close brace terminates
                ';' => break,        // Semicolon also terminates (unlike GNU sed doc claims!)
                _ => {
                    text.push(c);
                    self.advance();
                }
            }
        }
        // Return trimmed text (empty is OK - means execute pattern space)
        Ok(Token::ExecuteBody(text.trim().to_string()))
    }
    fn parse_filename_literal(&mut self) -> Result<Token> {
        while let Some(c) = self.current_char {
            if c == ' ' || c == '\t' {
                self.advance();
            } else {
                break;
            }
        }
        let mut name = String::new();
        while let Some(c) = self.current_char {
            match c {
                '\n' | '}' | ';' => break,
                _ => {
                    name.push(c);
                    self.advance();
                }
            }
        }
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            // Don't return error here - let parser handle it
            // Parser can check other conditions first (like address ranges in POSIX mode)
            // Return next token instead
            return self.next_token();
        }
        Ok(Token::Filename(trimmed))
    }
    fn parse_number(&mut self) -> Result<Token> {
        let mut num_str = String::new();
        while let Some(c) = self.current_char {
            if c.is_ascii_digit() {
                num_str.push(c);
                self.advance();
            } else {
                break;
            }
        }
        // Handle large numbers by capping at usize::MAX (GNU sed behavior)
        let num: usize = match num_str.parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                // Number too large or invalid - cap at usize::MAX
                // This matches GNU sed behavior for extremely large line numbers
                usize::MAX
            }
        };
        Ok(Token::LineNumber(num))
    }
    fn parse_plus_offset(&mut self) -> Result<Token> {
        self.advance();
        if let Some(c) = self.current_char {
            if c.is_ascii_digit() {
                let mut num_str = String::new();
                while let Some(c) = self.current_char {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                let num: usize = num_str.parse().unwrap_or(usize::MAX);
                Ok(Token::PlusOffset(num))
            } else {
                Ok(Token::SubstitutionDelim('+'))
            }
        } else {
            Ok(Token::SubstitutionDelim('+'))
        }
    }
    fn parse_tilde_step(&mut self) -> Result<Token> {
        self.advance();
        if let Some(c) = self.current_char {
            if c.is_ascii_digit() {
                let mut num_str = String::new();
                while let Some(c) = self.current_char {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                let num: usize = num_str.parse().unwrap_or(usize::MAX);
                Ok(Token::TildeStep(num))
            } else {
                Ok(Token::SubstitutionDelim('~'))
            }
        } else {
            Ok(Token::SubstitutionDelim('~'))
        }
    }
}
