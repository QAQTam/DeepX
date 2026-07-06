// Copyright (c) 2026 Red Authors
// License: MIT
//

use crate::constants;
use crate::context::Context;
use crate::errors::{Result, SedError};
use crate::posix_rules;
use crate::util::version::compare_versions;

use std::cmp::Ordering;

mod ast;
mod escapes;
mod lexer;

pub use ast::{
    Address, AddressRange, Command, HasAddressRange, PrintTiming, SubstitutionFlags, Token,
};
pub use lexer::{build_char_to_byte_mapping, is_utf8_locale, Lexer};

// Lexer moved to module `lexer`

/// Parser for building AST from tokens
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
    char_position: usize, // Approximate character position for error messages
    last_regex: Option<String>, // Track last regex pattern for empty regex support
    posix: bool,          // POSIX mode flag (extracted from Context)
    extended_regex: bool, // Extended regex mode (extracted from Context)
    sandbox: bool,        // Sandbox mode - disable e/r/w commands (extracted from Context)
    utf8_locale: bool,    // UTF-8 locale - affects translate command length validation
}

impl Parser {
    /// Create new parser from tokens and context
    ///
    /// Extracts configuration from Context to avoid lifetime issues.
    /// Note: Uses strict_posix mode for validation (--posix flag only, not POSIXLY_CORRECT)
    pub fn new(tokens: Vec<Token>, ctx: &Context) -> Self {
        Self {
            tokens,
            position: 0,
            char_position: 1, // Start at char 1 (GNU sed convention)
            last_regex: None,
            posix: ctx.is_strict_posix(), // Only --posix, not POSIXLY_CORRECT
            extended_regex: ctx.extended_regex,
            sandbox: ctx.sandbox,
            utf8_locale: is_utf8_locale(),
        }
    }

    /// Create a parse error with current character position
    fn error_at(&self, message: impl Into<String>) -> SedError {
        SedError::parse_at(message, self.char_position)
    }

    /// Check that the current token is a valid command terminator (EOF, newline, semicolon, comment, or close brace)
    /// Returns an error if there are extra characters after the command
    fn expect_command_end(&self) -> Result<()> {
        match self.current_token() {
            Token::Eof
            | Token::Newline
            | Token::Semicolon
            | Token::Comment(_)
            | Token::CloseBrace => Ok(()),
            _ => Err(self.error_at("extra characters after command")),
        }
    }

    /// Parse tokens into command AST
    pub fn parse(&mut self) -> Result<Vec<Command>> {
        self.parse_command_list()
    }

    /// Parse a sed script string into commands
    pub fn parse_script(input: &str) -> Result<Vec<Command>> {
        // Use default context for testing/convenience
        let ctx = Context::new_test();
        Self::parse_script_with_context(input, &ctx)
    }

    /// Parse a sed script string into commands with Context
    pub fn parse_script_with_context(input: &str, ctx: &Context) -> Result<Vec<Command>> {
        let mut lexer = Lexer::new_with_posix(input, ctx.is_posix());
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token()?;
            let is_eof = matches!(token, Token::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        let mut parser = Parser::new(tokens, ctx);
        parser.parse()
    }

    /// Parse a sed script string into commands with raw bytes for accurate multibyte detection
    ///
    /// The raw_bytes parameter should be the original bytes before lossy UTF-8 conversion.
    /// This enables accurate detection of UTF-8 lead bytes for delimiter validation.
    pub fn parse_script_with_raw_bytes(
        input: &str,
        raw_bytes: &[u8],
        ctx: &Context,
    ) -> Result<Vec<Command>> {
        Self::parse_script_with_raw_bytes_and_state(input, raw_bytes, ctx, None)
    }

    /// Parse with raw bytes and optional initial last_regex state
    ///
    /// Returns the parsed commands and the final last_regex state for chaining.
    pub fn parse_script_with_raw_bytes_and_state(
        input: &str,
        raw_bytes: &[u8],
        ctx: &Context,
        initial_last_regex: Option<String>,
    ) -> Result<Vec<Command>> {
        let mut lexer = Lexer::new_with_raw_bytes(input, raw_bytes, ctx.is_posix());
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token()?;
            let is_eof = matches!(token, Token::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        let mut parser = Parser::new(tokens, ctx);
        parser.last_regex = initial_last_regex;
        parser.parse()
    }

    /// Get the last regex pattern after parsing (for chaining parsers)
    pub fn get_last_regex(&self) -> Option<String> {
        self.last_regex.clone()
    }

    /// Parse and return both commands and final parser state
    pub fn parse_with_state(&mut self) -> Result<(Vec<Command>, Option<String>)> {
        let commands = self.parse()?;
        Ok((commands, self.last_regex.clone()))
    }

    /// Parse with raw bytes and return final last_regex for chaining
    pub fn parse_script_with_raw_bytes_chained(
        input: &str,
        raw_bytes: &[u8],
        ctx: &Context,
        initial_last_regex: Option<String>,
    ) -> Result<(Vec<Command>, Option<String>)> {
        let mut lexer = Lexer::new_with_raw_bytes(input, raw_bytes, ctx.is_posix());
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token()?;
            let is_eof = matches!(token, Token::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        let mut parser = Parser::new(tokens, ctx);
        parser.last_regex = initial_last_regex;
        parser.parse_with_state()
    }

    /// Parse a list of commands separated by newlines or semicolons
    fn parse_command_list(&mut self) -> Result<Vec<Command>> {
        let mut commands = Vec::new();

        while self.current_token() != &Token::Eof {
            // Skip newlines
            if let Token::Newline = self.current_token() {
                self.advance();
                continue;
            }
            // Skip stray semicolons
            if let Token::Semicolon = self.current_token() {
                self.advance();
                continue;
            }

            // Parse command
            commands.push(self.parse_command(false)?);

            // Handle semicolon separator
            if let Token::Semicolon = self.current_token() {
                self.advance();
                // Continue parsing more commands on same line
                continue;
            }
        }

        Ok(commands)
    }

    fn current_token(&self) -> &Token {
        self.tokens.get(self.position).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) {
        // Calculate actual character length of current token for accurate position tracking
        let token_len = match self.current_token() {
            Token::LineNumber(n) => n.to_string().len(),
            Token::Command(_) => 1,
            Token::Comma => 1,
            Token::Bang => 1,
            Token::Dollar => 1,
            Token::PlusOffset(n) => 1 + n.to_string().len(), // +N
            Token::TildeStep(n) => 1 + n.to_string().len(),  // ~N
            Token::Semicolon => 1,
            Token::Newline => 1,
            Token::OpenBrace => 1,
            Token::CloseBrace => 1,
            Token::SubstitutionDelim(_) => 1,
            Token::SubstitutionBody(s, _) => s.len() + 1, // +1 for leading 's'
            Token::TranslateBody(s, _) => s.len() + 1,    // +1 for leading 'y'
            Token::RegexAddr(s, m) => s.len() + m.len() + 2, // +2 for delimiters
            Token::AppendBody(_) | Token::InsertBody(_) | Token::ChangeBody(_) => 1, // Just the command letter
            Token::ExecuteBody(s) => s.len() + 1,                                    // +1 for 'e'
            Token::Filename(s) => s.len(),
            Token::String(s) => s.len(),
            Token::Comment(s) => s.len() + 1, // +1 for '#'
            Token::SubstitutionFlag(_) => 1,
            Token::Eof => 0,
        };

        self.char_position += token_len;
        self.position += 1;
    }

    fn parse_command(&mut self, in_group: bool) -> Result<Command> {
        // First try to parse optional address range
        let (address_range, negated) = self.parse_address_range()?;

        // Validate address 0 usage before parsing command
        if let Some(ref range) = address_range {
            self.validate_address_zero(range)?;
        }

        // Then parse the actual command
        match self.current_token() {
            Token::Comment(text) => {
                // Comments don't accept addresses
                if address_range.is_some() {
                    return Err(self.error_at("comments don't accept any addresses"));
                }
                let comment = Command::Comment(text.clone());
                self.advance();
                Ok(comment)
            }
            Token::SubstitutionDelim(':') => {
                // Check if there's an address - labels don't accept addresses
                if address_range.is_some() {
                    return Err(self.error_at(": doesn't want any addresses"));
                }
                self.advance();
                let name = self.parse_label_word()?;
                Ok(Command::Label { name })
            }
            Token::OpenBrace => self.parse_group(address_range, negated),
            Token::Command('s') => self.parse_substitution(address_range, negated),
            Token::TranslateBody(body, raw_bytes) => {
                // Construct Translate from pre-parsed body (lexer handled leading 'y')
                let b = body.clone();
                let rb = raw_bytes.clone();
                self.advance();
                let cmd =
                    self.parse_translate_from_body(address_range, negated, &b, rb.as_deref())?;
                // Check for junk after y command
                match self.current_token() {
                    Token::Eof
                    | Token::Newline
                    | Token::Semicolon
                    | Token::Comment(_)
                    | Token::CloseBrace => {
                        // Valid: EOF, newline, semicolon, comment, or closing brace after y
                    }
                    _ => {
                        return Err(self.error_at("extra characters after command"));
                    }
                }
                Ok(cmd)
            }
            Token::Command('a') => {
                let cmd_pos = self.char_position;
                self.advance();
                // Phase 3.3: Centralized address range validation
                let has_range = address_range
                    .as_ref()
                    .map(|r| r.start.is_some() && r.end.is_some())
                    .unwrap_or(false);
                posix_rules::validate_address_range_posix('a', has_range, self.posix, cmd_pos)?;
                match self.current_token() {
                    Token::AppendBody(text) => {
                        let t = text.clone();
                        self.advance();
                        Ok(Command::Append {
                            range: address_range,
                            negated,
                            text: t,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "expected \\ after 'a', 'c' or 'i'",
                            cmd_pos,
                        ));
                    }
                }
            }
            Token::Command('i') => {
                let cmd_pos = self.char_position;
                self.advance();
                // Phase 3.3: Centralized address range validation
                let has_range = address_range
                    .as_ref()
                    .map(|r| r.start.is_some() && r.end.is_some())
                    .unwrap_or(false);
                posix_rules::validate_address_range_posix('i', has_range, self.posix, cmd_pos)?;
                match self.current_token() {
                    Token::InsertBody(text) => {
                        let t = text.clone();
                        self.advance();
                        Ok(Command::Insert {
                            range: address_range,
                            negated,
                            text: t,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "expected \\ after 'a', 'c' or 'i'",
                            cmd_pos,
                        ));
                    }
                }
            }
            Token::Command('c') => {
                let cmd_pos = self.char_position;
                self.advance();
                match self.current_token() {
                    Token::ChangeBody(text) => {
                        let t = text.clone();
                        self.advance();
                        Ok(Command::Change {
                            range: address_range,
                            negated,
                            text: t,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "expected \\ after 'a', 'c' or 'i'",
                            cmd_pos,
                        ));
                    }
                }
            }
            Token::Command('p') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::Print {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('P') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::PrintFirstLine {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('d') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::Delete {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('q') => {
                let cmd_pos = self.char_position;
                self.advance();
                // q command only accepts one address, not a range
                if let Some(ref range) = address_range {
                    if range.start.is_some() && range.end.is_some() {
                        return Err(SedError::parse_at("command only uses one address", cmd_pos));
                    }
                }
                // Optional exit code after q
                let mut code: Option<i32> = None;
                if let Token::LineNumber(n) = self.current_token() {
                    code = Some(*n as i32);
                    self.advance();
                }
                self.expect_command_end()?;
                Ok(Command::Quit {
                    range: address_range,
                    negated,
                    exit_code: code,
                })
            }
            Token::Command('Q') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('Q', self.posix, cmd_pos)?;
                self.advance();
                // Q command only accepts one address, not a range
                if let Some(ref range) = address_range {
                    if range.start.is_some() && range.end.is_some() {
                        return Err(SedError::parse_at("command only uses one address", cmd_pos));
                    }
                }
                // Optional exit code after Q
                let mut code: Option<i32> = None;
                if let Token::LineNumber(n) = self.current_token() {
                    code = Some(*n as i32);
                    self.advance();
                }
                self.expect_command_end()?;
                Ok(Command::QuitSilent {
                    range: address_range,
                    negated,
                    exit_code: code,
                })
            }
            Token::Command('h') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::HoldCopy {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('H') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::HoldAppend {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('g') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::GetCopy {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('G') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::GetAppend {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('x') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::Exchange {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('N') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::N {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('D') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::BigD {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('n') => {
                self.advance();
                self.expect_command_end()?;
                Ok(Command::Next)
            }
            Token::Command('b') => {
                self.advance();
                let label = self.parse_optional_label_word()?;
                Ok(Command::Branch {
                    range: address_range,
                    negated,
                    label,
                })
            }
            Token::Command('t') => {
                self.advance();
                let label = self.parse_optional_label_word()?;
                Ok(Command::Test {
                    range: address_range,
                    negated,
                    label,
                })
            }
            Token::Command('T') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('T', self.posix, cmd_pos)?;
                self.advance();
                let label = self.parse_optional_label_word()?;
                Ok(Command::TestNeg {
                    range: address_range,
                    negated,
                    label,
                })
            }
            Token::Command('e') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('e', self.posix, cmd_pos)?;
                // Sandbox mode validation
                if self.sandbox {
                    return Err(SedError::parse_at(
                        "e/r/w commands disabled in sandbox mode",
                        cmd_pos,
                    ));
                }
                self.advance();
                // Lexer will provide ExecuteBody token with command text if 'e' is standalone
                match self.current_token() {
                    Token::ExecuteBody(text) => {
                        let cmd = text.clone();
                        self.advance();
                        Ok(Command::Execute {
                            range: address_range,
                            negated,
                            command: if cmd.is_empty() { None } else { Some(cmd) },
                        })
                    }
                    _ => {
                        // No ExecuteBody means execute pattern space
                        Ok(Command::Execute {
                            range: address_range,
                            negated,
                            command: None,
                        })
                    }
                }
            }
            Token::Command('v') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('v', self.posix, cmd_pos)?;
                self.advance();
                // Read version string (allows dots for version format like "4.8.0")
                let version = self.parse_version_string()?;

                // Check version compatibility immediately
                let version_to_check = if version.is_empty() { "4.0" } else { &version };

                if compare_versions(version_to_check, constants::GNU_SED_COMPAT_VERSION)
                    == Ordering::Greater
                {
                    // Report error at the last character of the version string
                    return Err(SedError::parse_at(
                        "expected newer version of sed",
                        self.char_position - 1,
                    ));
                }

                Ok(Command::Version { version })
            }
            Token::Command('z') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('z', self.posix, cmd_pos)?;
                self.advance();
                self.expect_command_end()?;
                Ok(Command::Clear {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('f') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('f', self.posix, cmd_pos)?;
                self.advance();
                self.expect_command_end()?;
                Ok(Command::PrintFilename {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('F') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('F', self.posix, cmd_pos)?;
                self.advance();
                self.expect_command_end()?;
                Ok(Command::PrintFilename {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('=') => {
                let cmd_pos = self.char_position;
                self.advance();
                // Phase 3.3: Centralized address range validation
                let has_range = address_range
                    .as_ref()
                    .map(|r| r.start.is_some() && r.end.is_some())
                    .unwrap_or(false);
                posix_rules::validate_address_range_posix('=', has_range, self.posix, cmd_pos)?;
                // Check for junk after =
                match self.current_token() {
                    Token::Eof
                    | Token::Newline
                    | Token::Semicolon
                    | Token::Comment(_)
                    | Token::CloseBrace => {
                        // Valid: EOF, newline, semicolon, comment, or closing brace after =
                    }
                    _ => {
                        return Err(self.error_at("extra characters after command"));
                    }
                }
                Ok(Command::LineNumber {
                    range: address_range,
                    negated,
                })
            }
            Token::Command('l') => {
                let cmd_pos = self.char_position;
                self.advance();
                // Phase 3.3: Centralized address range validation
                let has_range = address_range
                    .as_ref()
                    .map(|r| r.start.is_some() && r.end.is_some())
                    .unwrap_or(false);
                posix_rules::validate_address_range_posix('l', has_range, self.posix, cmd_pos)?;
                // Optional line length after l
                let mut line_length: Option<usize> = None;
                if let Token::LineNumber(n) = self.current_token() {
                    line_length = Some(*n);
                    self.advance();
                }
                // Check for junk after l (after optional line number)
                match self.current_token() {
                    Token::Eof
                    | Token::Newline
                    | Token::Semicolon
                    | Token::Comment(_)
                    | Token::CloseBrace => {
                        // Valid: EOF, newline, semicolon, comment, or closing brace after l
                    }
                    _ => {
                        return Err(self.error_at("extra characters after command"));
                    }
                }
                Ok(Command::List {
                    range: address_range,
                    negated,
                    line_length,
                })
            }
            Token::Command('w') => {
                let cmd_pos = self.char_position;
                // Sandbox mode validation
                if self.sandbox {
                    return Err(SedError::parse_at(
                        "e/r/w commands disabled in sandbox mode",
                        cmd_pos,
                    ));
                }
                self.advance();
                match self.current_token() {
                    Token::Filename(path) => {
                        let p = path.clone();
                        self.advance();
                        Ok(Command::Write {
                            range: address_range,
                            negated,
                            path: p,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "missing filename in r/R/w/W commands",
                            cmd_pos,
                        ));
                    }
                }
            }
            Token::Command('W') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized POSIX validation
                posix_rules::validate_command_posix('W', self.posix, cmd_pos)?;
                // Sandbox mode validation
                if self.sandbox {
                    return Err(SedError::parse_at(
                        "e/r/w commands disabled in sandbox mode",
                        cmd_pos,
                    ));
                }
                self.advance();
                match self.current_token() {
                    Token::Filename(path) => {
                        let p = path.clone();
                        self.advance();
                        Ok(Command::WriteFirstLine {
                            range: address_range,
                            negated,
                            path: p,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "missing filename in r/R/w/W commands",
                            cmd_pos,
                        ));
                    }
                }
            }
            Token::Command('r') => {
                let cmd_pos = self.char_position;
                // Phase 3.3: Centralized address range validation
                let has_range = address_range
                    .as_ref()
                    .map(|r| r.start.is_some() && r.end.is_some())
                    .unwrap_or(false);
                posix_rules::validate_address_range_posix('r', has_range, self.posix, cmd_pos)?;
                // Sandbox mode validation
                if self.sandbox {
                    return Err(SedError::parse_at(
                        "e/r/w commands disabled in sandbox mode",
                        cmd_pos,
                    ));
                }
                self.advance();
                match self.current_token() {
                    Token::Filename(path) => {
                        let p = path.clone();
                        self.advance();
                        Ok(Command::Read {
                            range: address_range,
                            negated,
                            path: p,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "missing filename in r/R/w/W commands",
                            cmd_pos,
                        ));
                    }
                }
            }
            Token::Command('R') => {
                let cmd_pos = self.char_position;
                // GNU extension, not allowed in POSIX mode
                if self.posix {
                    return Err(SedError::parse_at("unknown command: 'R'", cmd_pos));
                }
                // Sandbox mode validation
                if self.sandbox {
                    return Err(SedError::parse_at(
                        "e/r/w commands disabled in sandbox mode",
                        cmd_pos,
                    ));
                }
                self.advance();
                match self.current_token() {
                    Token::Filename(path) => {
                        let p = path.clone();
                        self.advance();
                        Ok(Command::ReadLine {
                            range: address_range,
                            negated,
                            path: p,
                        })
                    }
                    _ => {
                        return Err(SedError::parse_at(
                            "missing filename in r/R/w/W commands",
                            cmd_pos,
                        ));
                    }
                }
            }
            // Handle special tokens with better error messages
            Token::Semicolon | Token::Newline => {
                return Err(self.error_at("missing command"));
            }
            Token::CloseBrace => {
                // Check if there's an address and we're inside a group
                // Inside group with address: "'}' doesn't want any addresses"
                // Outside group (or no address): "unexpected '}'"
                if in_group && address_range.is_some() {
                    return Err(self.error_at("'}' doesn't want any addresses"));
                }
                return Err(self.error_at("unexpected '}'"));
            }
            Token::Eof => {
                return Err(self.error_at("unexpected end of script"));
            }
            Token::Filename(name) => {
                // Filename appearing as command indicates lexer/parser issue
                return Err(self.error_at(format!("unknown command: '{}'", name)));
            }
            _ => {
                // Use Display trait for better error messages
                if let Token::Command(ch) = self.current_token() {
                    return Err(self.error_at(format!("unknown command: '{}'", ch)));
                }
                return Err(self.error_at(format!("unknown command: {}", self.current_token())));
            }
        }
    }

    fn parse_optional_label_word(&mut self) -> Result<String> {
        let mut name = String::new();
        while self.current_token() != &Token::Eof {
            match self.current_token() {
                Token::Newline | Token::Semicolon | Token::CloseBrace => break,
                Token::Command(c) => {
                    name.push(*c);
                    self.advance();
                }
                Token::LineNumber(n) => {
                    name.push_str(&n.to_string());
                    self.advance();
                }
                Token::SubstitutionDelim('_') => {
                    name.push('_');
                    self.advance();
                }
                _ => break,
            }
        }
        Ok(name)
    }

    fn parse_version_string(&mut self) -> Result<String> {
        let mut version = String::new();
        while self.current_token() != &Token::Eof {
            match self.current_token() {
                Token::Newline | Token::Semicolon | Token::CloseBrace => break,
                Token::Command(c) => {
                    version.push(*c);
                    self.advance();
                }
                Token::LineNumber(n) => {
                    version.push_str(&n.to_string());
                    self.advance();
                }
                Token::SubstitutionDelim(c) if *c == '.' || *c == '-' || *c == '_' => {
                    // Allow dots, dashes, underscores in version strings
                    version.push(*c);
                    self.advance();
                }
                _ => break,
            }
        }
        Ok(version)
    }

    fn parse_label_word(&mut self) -> Result<String> {
        let mut name = String::new();
        while self.current_token() != &Token::Eof {
            match self.current_token() {
                Token::Newline | Token::Semicolon | Token::CloseBrace => break,
                Token::Command(c) => {
                    name.push(*c);
                    self.advance();
                }
                Token::LineNumber(n) => {
                    name.push_str(&n.to_string());
                    self.advance();
                }
                Token::SubstitutionDelim('_') => {
                    name.push('_');
                    self.advance();
                }
                _ => break,
            }
        }
        if name.is_empty() {
            // GNU sed says `":" lacks a label` and reports char position of ':'
            // char_position is already incremented past ':', so subtract 1
            return Err(SedError::parse_at(
                "\":\" lacks a label",
                self.char_position - 1,
            ));
        }
        Ok(name)
    }

    // parse_filename_word removed in favor of filename literal handled by lexer

    /// Parse address range: [addr1][,addr2][!]
    fn parse_address_range(&mut self) -> Result<(Option<AddressRange>, bool)> {
        let mut start_addr = None;
        let mut end_addr = None;
        let mut negated = false;

        // Check if first token is +N or ~N (invalid as first address)
        match self.current_token() {
            Token::PlusOffset(_) | Token::TildeStep(_) => {
                // Position should point after the + or ~ character
                return Err(SedError::parse_at(
                    "invalid usage of +N or ~N as first address",
                    self.char_position + 1,
                ));
            }
            _ => {}
        }

        // Check for first address
        if let Some(addr) = self.try_parse_address()? {
            start_addr = Some(addr);

            // Check for comma (range)
            if let Token::Comma = self.current_token() {
                self.advance(); // consume comma

                // Parse second address
                if let Some(addr2) = self.try_parse_address()? {
                    end_addr = Some(addr2);
                } else {
                    return Err(self.error_at("unexpected ','"));
                }
            }
        }

        // Check for negation (!)
        if let Token::Bang = self.current_token() {
            negated = true;
            self.advance();
            // Check for double negation (BAD_BANG)
            if let Token::Bang = self.current_token() {
                return Err(SedError::parse_at("multiple '!'s", self.char_position));
            }
        }

        let range = if start_addr.is_some() || end_addr.is_some() {
            Some(AddressRange::new(start_addr, end_addr))
        } else {
            None
        };

        Ok((range, negated))
    }

    /// Validate address 0 usage - address 0 is only valid in specific contexts
    fn validate_address_zero(&self, range: &AddressRange) -> Result<()> {
        // Check if start address is 0
        if let Some(Address::Line(0)) = range.start {
            // If there's an end address, check its type
            if let Some(ref end) = range.end {
                match end {
                    Address::Line(_) => {
                        // 0,NUM is invalid (e.g., 0,4)
                        return Err(self.error_at("invalid usage of line address 0"));
                    }
                    Address::Dollar => {
                        // 0,$ is also invalid
                        return Err(self.error_at("invalid usage of line address 0"));
                    }
                    // 0,/pattern/ is OK
                    Address::Regex(_) => {}
                    // Other combinations - accepted (conservative validation)
                    _ => {}
                }
            }
            // Single address 0 is OK (for commands like 0r)
        }
        Ok(())
    }

    /// Try to parse an address, returns None if current token is not an address
    fn try_parse_address(&mut self) -> Result<Option<Address>> {
        // Parse base address
        let base_addr = match self.current_token() {
            Token::LineNumber(n) => {
                let line = *n;
                self.advance();
                Some(Address::Line(line))
            }
            Token::Dollar => {
                self.advance();
                Some(Address::Dollar)
            }
            Token::RegexAddr(pattern, modifiers) => {
                let mut regex = pattern.clone();

                // Handle empty regex pattern - reuse last or error
                if regex.is_empty() {
                    // Check if modifiers are present - this is an error
                    if !modifiers.is_empty() {
                        return Err(SedError::parse_at(
                            "cannot specify modifiers on empty regexp",
                            3,
                        ));
                    }
                    // No modifiers - try to reuse last regex
                    match self.last_regex.clone() {
                        Some(last) => regex = last,
                        None => {
                            return Err(SedError::parse_at("no previous regular expression", 0));
                        }
                    }
                } else {
                    // Save non-empty pattern as last regex
                    self.last_regex = Some(regex.clone());
                }

                // Validate that address regex doesn't contain backreferences
                use crate::util::regex::validate_address_regex;
                validate_address_regex(&regex).map_err(|e| {
                    // Add character position to error
                    // char_position is after RegexAddr token
                    // For /\1/: char_position is 2 after consuming token, regex.len() is 2, need +1 for closing /
                    let error_pos = self.char_position + regex.len() + 1;
                    SedError::parse_at(e.to_string(), error_pos)
                })?;

                self.advance();
                Some(Address::Regex(regex))
            }
            Token::PlusOffset(n) => {
                let offset = *n as isize;
                self.advance();
                // +N without base means relative to line 0 (special case for ranges like 0,+5)
                Some(Address::Relative(Box::new(Address::Line(0)), offset))
            }
            Token::TildeStep(n) => {
                let step = *n;
                self.advance();
                // ~N without base is not valid in GNU sed; we accept it as 0~N (lenient extension)
                Some(Address::Step(Box::new(Address::Line(0)), step))
            }
            _ => None, // Not an address
        };

        // If we have a base address, check for modifiers (~step or +offset)
        if let Some(addr) = base_addr {
            match self.current_token() {
                Token::TildeStep(n) => {
                    let step = *n;
                    self.advance();
                    Ok(Some(Address::Step(Box::new(addr), step)))
                }
                Token::PlusOffset(n) => {
                    let offset = *n as isize;
                    self.advance();
                    Ok(Some(Address::Relative(Box::new(addr), offset)))
                }
                _ => Ok(Some(addr)),
            }
        } else {
            Ok(None)
        }
    }

    fn parse_group(&mut self, range: Option<AddressRange>, negated: bool) -> Result<Command> {
        // Parse { command1; command2; ... }
        if let Token::OpenBrace = self.current_token() {
            self.advance(); // consume {

            let mut group_commands = Vec::new();

            while self.current_token() != &Token::CloseBrace && self.current_token() != &Token::Eof
            {
                // Skip newlines inside group
                if let Token::Newline = self.current_token() {
                    self.advance();
                    continue;
                }
                // Skip stray semicolons inside group
                if let Token::Semicolon = self.current_token() {
                    self.advance();
                    continue;
                }

                group_commands.push(self.parse_command(true)?);

                // Handle semicolon separator inside group
                if let Token::Semicolon = self.current_token() {
                    self.advance();
                }
            }

            if self.current_token() != &Token::CloseBrace {
                return Err(SedError::parse_at("unmatched '{'", 0));
            }
            self.advance(); // consume }

            // Check for junk after }
            match self.current_token() {
                Token::Eof
                | Token::Newline
                | Token::Semicolon
                | Token::Comment(_)
                | Token::CloseBrace => {
                    // Valid: EOF, newline, semicolon, comment, or closing brace after }
                    // (CloseBrace is for nested groups like {{p}})
                }
                _ => {
                    return Err(self.error_at("extra characters after command"));
                }
            }

            Ok(Command::Group {
                range,
                negated,
                commands: group_commands,
            })
        } else {
            return Err(self.error_at("missing command"));
        }
    }

    fn parse_substitution(
        &mut self,
        range: Option<AddressRange>,
        negated: bool,
    ) -> Result<Command> {
        if let Token::Command('s') = self.current_token() {
            // Save position of 's' for error reporting
            let s_position = self.char_position;
            self.advance();

            // Expect the next token to be SubstitutionBody
            let (body, body_raw_bytes) = match self.current_token() {
                Token::SubstitutionBody(s, rb) => (s.clone(), rb.clone()),
                other => {
                    return Err(SedError::parse(format!(
                        "unexpected token after 's': {:?}",
                        other
                    )));
                }
            };
            self.advance();

            // Body format: <delim><pattern><delim><replacement><delim><flags...>
            let chars: Vec<char> = body.chars().collect();
            if chars.is_empty() {
                // char_position is after 's' and SubstitutionBody tokens, report at 's'
                return Err(SedError::parse_at("unterminated 's' command", s_position));
            }
            let delim = chars[0];
            let mut i = 1;

            // Capture char_position for error reporting inside closure
            let base_char_pos = s_position;

            // Helper: parse until next delimiter, honoring character classes for pattern
            // is_pattern: if true, strip backslash when escaping delimiter (for regex pattern)
            //             if false, keep backslash when escaping delimiter (for replacement)
            let parse_until_with_class = |chars: &Vec<char>,
                                          start: usize,
                                          d: char,
                                          track_class: bool,
                                          is_pattern: bool|
             -> Result<(String, usize)> {
                let mut out = String::new();
                let mut idx = start;
                let mut escaped = false;
                let mut in_class = false;
                while idx < chars.len() {
                    let c = chars[idx];
                    idx += 1;
                    if escaped {
                        // In pattern: escaped delimiter becomes just the delimiter char
                        // In replacement: keep backslash so replacement parser handles special chars
                        if c == d && is_pattern {
                            // Pattern: strip backslash, just push the delimiter char
                            out.push(c);
                        } else {
                            // Replacement or non-delimiter: keep the backslash
                            out.push('\\');
                            out.push(c);
                        }
                        escaped = false;
                        continue;
                    }
                    if c == '\\' {
                        escaped = true;
                        continue;
                    }
                    if track_class {
                        if c == '[' {
                            in_class = true;
                            out.push('[');
                            continue;
                        }
                        if c == ']' && in_class {
                            in_class = false;
                            out.push(']');
                            continue;
                        }
                    }
                    if c == d && !in_class {
                        return Ok((out, idx));
                    }
                    out.push(c);
                }
                // Calculate error position: base_char_pos is position of 's'
                // idx is current position in chars vector (body)
                let error_pos = base_char_pos + idx;
                return Err(SedError::parse_at("unterminated 's' command", error_pos));
            };

            // Track character class only for the pattern if delimiter is not '['
            // is_pattern=true: strip backslash when escaping delimiter
            let (pattern, next_i) = parse_until_with_class(&chars, i, delim, delim != '[', true)?;
            i = next_i;

            // Handle empty pattern - reuse last regex or error if none exists
            // Track if original pattern was empty (for backref validation skip)
            let uses_last_regex = pattern.is_empty();

            // Extract pattern_raw_bytes from body_raw_bytes if available
            // Pattern starts at char index 1 (after delimiter) and ends at next_i-1 (before delimiter)
            let pattern_start_char = 1usize; // After the delimiter
            let pattern_raw_bytes: Option<Vec<u8>> = if !uses_last_regex {
                body_raw_bytes.as_ref().and_then(|raw| {
                    let mapping = build_char_to_byte_mapping(&body, raw);
                    if pattern_start_char < mapping.len() {
                        let start_byte = mapping[pattern_start_char];
                        // next_i points to character after the closing delimiter
                        // So next_i - 1 is the delimiter, and we want bytes up to (not including) that
                        let end_byte = if next_i > 0 && next_i - 1 < mapping.len() {
                            mapping[next_i - 1]
                        } else if next_i < mapping.len() {
                            mapping[next_i]
                        } else {
                            raw.len()
                        };
                        if start_byte <= end_byte && end_byte <= raw.len() {
                            Some(raw[start_byte..end_byte].to_vec())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            } else {
                None // Don't extract raw bytes when reusing last regex
            };

            if uses_last_regex {
                // Validate that there's a previous regex, but DON'T fill in the pattern.
                // Keep pattern empty so compile_substitution sets use_last=true,
                // allowing runtime lookup of last_s_regex (which can be updated by address patterns).
                if self.last_regex.is_none() {
                    return Err(SedError::parse_at("no previous regular expression", 0));
                }
                // Pattern stays empty - runtime will use last_s_regex
            } else {
                // Save non-empty pattern as last regex
                self.last_regex = Some(pattern.clone());
            }

            // is_pattern=false: keep backslash when escaping delimiter (for replacement parsing)
            let (replacement, next_i2) = parse_until_with_class(&chars, i, delim, false, false)?;
            // Store replacement start char position for raw bytes extraction
            let replacement_start_char = i;
            i = next_i2;

            // Extract replacement_raw_bytes from body_raw_bytes if available
            let replacement_raw_bytes: Option<Vec<u8>> = body_raw_bytes.as_ref().and_then(|raw| {
                // Build char-to-byte mapping for the body
                let mapping = build_char_to_byte_mapping(&body, raw);
                if replacement_start_char < mapping.len() {
                    let start_byte = mapping[replacement_start_char];
                    // next_i2 points to position AFTER delimiter, so next_i2 - 1 is the delimiter
                    // Use mapping[next_i2 - 1] to get byte position of delimiter (exclusive end)
                    let end_byte = if next_i2 > 0 && next_i2 - 1 < mapping.len() {
                        mapping[next_i2 - 1]
                    } else if next_i2 < mapping.len() {
                        mapping[next_i2]
                    } else {
                        raw.len()
                    };
                    if start_byte <= end_byte && end_byte <= raw.len() {
                        Some(raw[start_byte..end_byte].to_vec())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            // Flags parsing
            let mut flags = SubstitutionFlags::default();
            let mut write_filename: Option<String> = None;
            let mut saw_w = false;
            let mut saw_space_in_flags = false;
            // Phase 4.2: Stateless flag parsing - track positions instead of state
            let mut p_position: Option<usize> = None; // Position of last 'p' flag
            let mut e_position: Option<usize> = None; // Position of last 'e' flag
            while i < chars.len() {
                let c = chars[i];
                i += 1;
                match c {
                    'g' => {
                        if flags.global {
                            let error_pos = base_char_pos + i;
                            return Err(SedError::parse_at(
                                "multiple 'g' options to 's' command",
                                error_pos,
                            ));
                        }
                        flags.global = true;
                    }
                    'p' => {
                        // Phase 4.2: Multiple 'p' is always an error (GNU sed compatibility)
                        if flags.print {
                            let error_pos = base_char_pos + i;
                            return Err(SedError::parse_at(
                                "multiple 'p' options to 's' command",
                                error_pos,
                            ));
                        }
                        flags.print = true;
                        p_position = Some(i);
                    }
                    'I' | 'i' => {
                        // Phase 3.3: Centralized POSIX validation
                        posix_rules::validate_subst_flag_posix(c, self.posix, base_char_pos + i)?;
                        flags.ignore_case = true;
                    }
                    'm' => {
                        // Phase 3.3: Centralized POSIX validation
                        posix_rules::validate_subst_flag_posix(c, self.posix, base_char_pos + i)?;
                        if flags.multiline_dotall {
                            return Err(self.error_at("cannot use both m and M flags"));
                        }
                        flags.multiline = true;
                    }
                    'M' => {
                        // Phase 3.3: Centralized POSIX validation
                        posix_rules::validate_subst_flag_posix(c, self.posix, base_char_pos + i)?;
                        if flags.multiline {
                            return Err(self.error_at("cannot use both m and M flags"));
                        }
                        flags.multiline_dotall = true;
                    }
                    'e' => {
                        // Phase 3.3: Centralized POSIX validation
                        posix_rules::validate_subst_flag_posix(c, self.posix, base_char_pos + i)?;
                        // Sandbox mode validation
                        if self.sandbox {
                            return Err(SedError::parse_at(
                                "e/r/w commands disabled in sandbox mode",
                                base_char_pos + i,
                            ));
                        }
                        // Phase 4.2: Track FIRST position only (for timing determination)
                        flags.execute = true;
                        if e_position.is_none() {
                            e_position = Some(i);
                        }
                    }
                    '0' => {
                        let error_pos = base_char_pos + i;
                        return Err(SedError::parse_at(
                            "number option to 's' command may not be zero",
                            error_pos,
                        ));
                    }
                    '1'..='9' => {
                        if flags.occurrence.is_some() {
                            let error_pos = base_char_pos + i;
                            return Err(SedError::parse_at(
                                "multiple number options to 's' command",
                                error_pos,
                            ));
                        }
                        if saw_space_in_flags {
                            let error_pos = base_char_pos + i;
                            return Err(SedError::parse_at("unknown option to 's'", error_pos));
                        }
                        flags.occurrence = Some((c as u32 - '0' as u32) as usize);
                    }
                    'w' => {
                        if saw_space_in_flags {
                            let error_pos = base_char_pos + i;
                            return Err(SedError::parse_at("unknown option to 's'", error_pos));
                        }
                        // Sandbox mode validation
                        if self.sandbox {
                            return Err(SedError::parse_at(
                                "e/r/w commands disabled in sandbox mode",
                                base_char_pos + i,
                            ));
                        }
                        saw_w = true;
                        // Calculate position of 'w' for error reporting
                        // base_char_pos is position of 's' command
                        // i points to position after 'w' in body (after i++), so position is base_char_pos + i
                        let w_position = base_char_pos + i;
                        // Consume optional whitespace then read filename until whitespace/end
                        // Here we capture the rest of body as filename (simplified; refined later if needed)
                        let mut name = String::new();
                        while i < chars.len() && chars[i].is_whitespace() {
                            i += 1;
                        }
                        while i < chars.len() {
                            let ch = chars[i];
                            if ch == '\n' {
                                break;
                            }
                            name.push(ch);
                            i += 1;
                        }
                        if name.is_empty() {
                            return Err(SedError::parse_at(
                                "missing filename in r/R/w/W commands",
                                w_position,
                            ));
                        }
                        write_filename = Some(name.trim().to_string());
                    }
                    c if c.is_whitespace() => {
                        saw_space_in_flags = true;
                    }
                    _ => {
                        // Calculate position of unknown flag
                        // base_char_pos is position of 's' command
                        // i has already been incremented past the flag, so position is base_char_pos + i
                        let error_pos = base_char_pos + i;
                        return Err(SedError::parse_at(
                            format!("unknown option to 's'"),
                            error_pos,
                        ));
                    }
                }
            }
            if saw_w {
                flags.write_file = write_filename;
            }

            // Phase 4.2: Determine print/execute timing based on flag positions
            use crate::parser::ast::PrintTiming;
            flags.print_timing = match (p_position, e_position) {
                (Some(p), Some(e)) if p < e => PrintTiming::PrintThenExecute, // 'pe'
                (Some(p), Some(e)) if e < p => PrintTiming::ExecuteThenPrint, // 'ep'
                _ => PrintTiming::None, // Not both present, or only one present
            };

            // Calculate position after replacement (closing delimiter) for error reporting
            // base_char_pos is position of 's', i is position in chars after closing delimiter
            let replacement_end_pos = base_char_pos + i;

            use crate::util::regex::{validate_replacement_backrefs, validate_replacement_escapes};

            // Validate backreferences in replacement (not in POSIX mode - they're silently ignored there)
            // Skip validation when using last regex - we don't know the runtime pattern's group count
            // GNU sed allows \N references to non-existent groups (outputs empty string at runtime)
            if !self.posix && !uses_last_regex {
                // Phase 3.2: Use extended_regex flag to determine BRE/ERE mode for validation
                // Single validation instead of try-both-and-accept
                validate_replacement_backrefs(
                    &replacement,
                    &pattern,
                    self.extended_regex,
                    replacement_end_pos,
                )?;
            }

            // Validate escape sequences (e.g., \c not followed by \)
            validate_replacement_escapes(&replacement, replacement_end_pos)?;

            Ok(Command::Substitution {
                range,
                negated,
                pattern,
                pattern_raw_bytes,
                replacement,
                replacement_raw_bytes,
                flags,
                delimiter: delim,
            })
        } else {
            return Err(self.error_at("unterminated 's' command"));
        }
    }

    fn parse_translate_from_body(
        &mut self,
        range: Option<AddressRange>,
        negated: bool,
        body: &str,
        raw_bytes: Option<&[u8]>,
    ) -> Result<Command> {
        let chars: Vec<char> = body.chars().collect();
        if chars.is_empty() {
            return Err(self.error_at("unterminated 'y' command"));
        }
        let delim = chars[0];
        let mut i = 1;
        let parse_until = |start: usize| -> Result<(String, usize)> {
            let mut out = String::new();
            let mut idx = start;
            let mut escaped = false;
            while idx < chars.len() {
                let c = chars[idx];
                idx += 1;
                if escaped {
                    // If escaped char is the delimiter, it's an escaped delimiter
                    // (just the char, no backslash). Otherwise keep the backslash.
                    if c != delim {
                        out.push('\\');
                    }
                    out.push(c);
                    escaped = false;
                    continue;
                }
                if c == '\\' {
                    escaped = true;
                    continue;
                }
                if c == delim {
                    return Ok((out, idx));
                }
                out.push(c);
            }
            return Err(self.error_at("unterminated 'y' command"));
        };
        let (from_raw, next) = parse_until(i)?;
        i = next;
        let (to_raw, _next2) = parse_until(i)?;

        // GNU sed does NOT interpret octal escapes in y command
        let from = escapes::decode_standard_escapes(&from_raw);
        let to = escapes::decode_standard_escapes(&to_raw);

        // Extract from_bytes and to_bytes from raw_bytes if available
        let (from_bytes, to_bytes) = if let Some(raw) = raw_bytes {
            // raw_bytes format: <delim><from><delim><to><delim>
            // Find delimiter (first byte)
            if raw.is_empty() {
                (None, None)
            } else {
                let delim_byte = raw[0];
                // Helper to extract bytes, stripping backslash from escaped delimiters
                let extract_part = |slice: &[u8]| -> Vec<u8> {
                    let mut out = Vec::new();
                    let mut j = 0;
                    while j < slice.len() {
                        if slice[j] == b'\\' && j + 1 < slice.len() {
                            if slice[j + 1] == delim_byte {
                                // Escaped delimiter: skip backslash, push delimiter
                                out.push(slice[j + 1]);
                                j += 2;
                            } else {
                                // Other escape: keep both chars
                                out.push(slice[j]);
                                out.push(slice[j + 1]);
                                j += 2;
                            }
                        } else {
                            out.push(slice[j]);
                            j += 1;
                        }
                    }
                    out
                };

                let mut parts: Vec<Vec<u8>> = Vec::new();
                let mut start = 1;
                let mut i = 1;
                let mut escaped = false;
                while i < raw.len() {
                    if escaped {
                        escaped = false;
                        i += 1;
                        continue;
                    }
                    if raw[i] == b'\\' {
                        escaped = true;
                        i += 1;
                        continue;
                    }
                    if raw[i] == delim_byte {
                        parts.push(extract_part(&raw[start..i]));
                        start = i + 1;
                    }
                    i += 1;
                }
                if parts.len() >= 2 {
                    // GNU sed does NOT interpret octal escapes in y command
                    let fb = escapes::decode_standard_escapes_to_bytes(&parts[0]);
                    let tb = escapes::decode_standard_escapes_to_bytes(&parts[1]);
                    (Some(fb), Some(tb))
                } else {
                    (None, None)
                }
            }
        } else {
            (None, None)
        };
        // Validate lengths based on locale
        // In UTF-8 locale: compare character counts
        // In multibyte non-UTF-8 locale (e.g., Shift-JIS): compare multibyte character counts
        // In C/POSIX single-byte locale: compare byte counts
        let lengths_match = if self.utf8_locale {
            // UTF-8 locale: count characters
            from.chars().count() == to.chars().count()
        } else if crate::mbcs::is_multibyte_locale() {
            // Multibyte non-UTF-8 locale (e.g., Shift-JIS): count multibyte characters
            match (&from_bytes, &to_bytes) {
                (Some(fb), Some(tb)) => {
                    crate::mbcs::count_mb_chars(fb) == crate::mbcs::count_mb_chars(tb)
                }
                _ => from.len() == to.len(), // Fallback to string byte length
            }
        } else {
            // C locale: count bytes (use from_bytes/to_bytes if available)
            match (&from_bytes, &to_bytes) {
                (Some(fb), Some(tb)) => fb.len() == tb.len(),
                _ => from.len() == to.len(), // Fallback to string byte length
            }
        };

        if from.is_empty() || to.is_empty() || !lengths_match {
            // Error position: char_position is right after the TranslateBody token,
            // so the last character of the body is at char_position - 1
            let error_pos = self.char_position - 1;
            return Err(SedError::parse_at(
                "'y' command strings have different lengths",
                error_pos,
            ));
        }

        Ok(Command::Translate {
            range,
            negated,
            from,
            to,
            from_bytes,
            to_bytes,
            delimiter: delim,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, Command, Lexer, Parser};
    use crate::context::Context;
    use crate::parser::Token;

    #[test]
    fn test_lexer_basic_tokens() {
        let mut lexer = Lexer::new("$,;!{}");
        assert_eq!(lexer.next_token().unwrap(), Token::Dollar);
        assert_eq!(lexer.next_token().unwrap(), Token::Comma);
        assert_eq!(lexer.next_token().unwrap(), Token::Semicolon);
        assert_eq!(lexer.next_token().unwrap(), Token::Bang);
        assert_eq!(lexer.next_token().unwrap(), Token::OpenBrace);
        assert_eq!(lexer.next_token().unwrap(), Token::CloseBrace);
        assert_eq!(lexer.next_token().unwrap(), Token::Eof);
    }

    #[test]
    fn test_lexer_numbers() {
        let mut lexer = Lexer::new("4 20 $");
        assert_eq!(lexer.next_token().unwrap(), Token::LineNumber(4));
        assert_eq!(lexer.next_token().unwrap(), Token::LineNumber(20));
        assert_eq!(lexer.next_token().unwrap(), Token::Dollar);
    }

    #[test]
    fn test_lexer_regex_addr() {
        let mut lexer = Lexer::new("/pattern/");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::RegexAddr("pattern".to_string(), String::new())
        );
    }

    #[test]
    fn test_lexer_comment() {
        let mut lexer = Lexer::new("# this is a comment");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Comment(" this is a comment".to_string())
        );
    }

    #[test]
    fn test_lexer_commands() {
        // Test that 's' followed by space is treated as a substitution command
        // (space can be a delimiter, though unusual)
        let mut lexer = Lexer::new("s p d");
        assert_eq!(lexer.next_token().unwrap(), Token::Command('s'));
        // Next token should be SubstitutionBody, which will fail to parse
        // because the substitution is malformed (unterminated)
        assert!(lexer.next_token().is_err());
    }

    #[test]
    fn test_lexer_addresses_and_commands() {
        let mut lexer = Lexer::new("4p $d 1,5s");
        assert_eq!(lexer.next_token().unwrap(), Token::LineNumber(4));
        assert_eq!(lexer.next_token().unwrap(), Token::Command('p'));
        assert_eq!(lexer.next_token().unwrap(), Token::Dollar);
        assert_eq!(lexer.next_token().unwrap(), Token::Command('d'));
        assert_eq!(lexer.next_token().unwrap(), Token::LineNumber(1));
        assert_eq!(lexer.next_token().unwrap(), Token::Comma);
        assert_eq!(lexer.next_token().unwrap(), Token::LineNumber(5));
        assert_eq!(lexer.next_token().unwrap(), Token::Command('s'));
    }

    #[test]
    fn test_lexer_escaped_regex() {
        let mut lexer = Lexer::new(r"/a\/b/");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::RegexAddr("a/b".to_string(), String::new())
        );
    }

    #[test]
    fn test_lexer_relative_addresses() {
        let mut lexer = Lexer::new("+5 ~3 +");
        assert_eq!(lexer.next_token().unwrap(), Token::PlusOffset(5));
        assert_eq!(lexer.next_token().unwrap(), Token::TildeStep(3));
        // + without number should be treated as delimiter
        assert_eq!(lexer.next_token().unwrap(), Token::SubstitutionDelim('+'));
        assert_eq!(lexer.next_token().unwrap(), Token::Eof);
    }

    #[test]
    fn test_parser_simple_address() {
        let mut lexer = Lexer::new("4p");
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Print { range, negated } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(4)));
                assert_eq!(range.end, None);
            }
            _ => panic!("Expected Print command"),
        }
    }

    #[test]
    fn test_parser_address_range() {
        let mut lexer = Lexer::new("1,5d");
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Delete { range, negated } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(1)));
                assert_eq!(range.end, Some(Address::Line(5)));
            }
            _ => panic!("Expected Delete command"),
        }
    }

    #[test]
    fn test_parser_negated_address() {
        let mut lexer = Lexer::new("4!p");
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Print { range, negated } => {
                assert!(*negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(4)));
                assert_eq!(range.end, None);
            }
            _ => panic!("Expected Print command"),
        }
    }

    #[test]
    fn test_parser_command_group() {
        let mut lexer = Lexer::new("1,5{p;d}");
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Group {
                range,
                negated,
                commands: group_commands,
            } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(1)));
                assert_eq!(range.end, Some(Address::Line(5)));
                assert_eq!(group_commands.len(), 2);

                // Check first command is print
                if let Command::Print {
                    range: p_range,
                    negated: p_negated,
                } = &group_commands[0]
                {
                    assert!(!p_negated);
                    assert!(p_range.is_none()); // No address on inner command
                } else {
                    panic!("Expected Print command in group");
                }

                // Check second command is delete
                if let Command::Delete {
                    range: d_range,
                    negated: d_negated,
                } = &group_commands[1]
                {
                    assert!(!d_negated);
                    assert!(d_range.is_none()); // No address on inner command
                } else {
                    panic!("Expected Delete command in group");
                }
            }
            _ => panic!("Expected Group command"),
        }
    }

    #[test]
    fn test_parser_semicolon_separator() {
        let mut lexer = Lexer::new("p;d");
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 2);

        match &commands[0] {
            Command::Print { .. } => {}
            _ => panic!("Expected first command to be Print"),
        }

        match &commands[1] {
            Command::Delete { .. } => {}
            _ => panic!("Expected second command to be Delete"),
        }
    }

    #[test]
    fn test_lexer_line_continuation() {
        let input = "4\\\np";
        let mut lexer = Lexer::new(input);

        let token1 = lexer.next_token().unwrap();
        assert_eq!(token1, Token::LineNumber(4));

        let token2 = lexer.next_token().unwrap();
        assert_eq!(token2, Token::Command('p'));

        let token3 = lexer.next_token().unwrap();
        assert_eq!(token3, Token::Eof);
    }

    #[test]
    fn test_parser_complex_grouping_with_addresses() {
        // Simplified test focusing on grouping and addresses without substitution complexity
        let input = "/pattern/!{p;d}";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            match token {
                Ok(Token::Eof) => {
                    tokens.push(Token::Eof);
                    break;
                }
                Ok(t) => tokens.push(t),
                Err(e) => {
                    println!("Lexer error at input: {}", input);
                    println!("Error: {}", e);
                    println!("Tokens so far: {:?}", tokens);
                    panic!("Lexer failed");
                }
            }
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Group {
                range,
                negated,
                commands: group_commands,
            } => {
                assert!(*negated); // Negated with !
                assert!(range.is_some());

                let range = range.as_ref().unwrap();
                if let Some(Address::Regex(pattern)) = &range.start {
                    assert_eq!(pattern, "pattern");
                } else {
                    panic!("Expected regex address");
                }

                assert_eq!(group_commands.len(), 2);
                // Check we have print and delete commands
                matches!(group_commands[0], Command::Print { .. });
                matches!(group_commands[1], Command::Delete { .. });
            }
            _ => panic!("Expected Group command"),
        }
    }

    #[test]
    fn test_parser_multiline_with_continuation() {
        let input = "1\\\n,\\\n5\\\np";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Print { range, negated } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(1)));
                assert_eq!(range.end, Some(Address::Line(5)));
            }
            _ => panic!("Expected Print command"),
        }
    }

    // NOTE: Substitution parsing is handled by this parser (lexer builds a
    // SubstitutionBody with an arbitrary delimiter, including '['), then
    // converted to runtime commands in lib.rs (convert_new_command_to_old).
    // The previous note about handling in main.rs is obsolete.

    #[test]
    fn test_parser_line_number_command() {
        let input = "4=";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::LineNumber { range, negated } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(4)));
                assert_eq!(range.end, None);
            }
            _ => panic!("Expected LineNumber command"),
        }
    }

    #[test]
    fn test_parser_list_command() {
        let input = "1,5l";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::List { range, negated, .. } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(1)));
                assert_eq!(range.end, Some(Address::Line(5)));
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parser_multiple_commands() {
        let input = "p;d;q";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 3);

        matches!(&commands[0], Command::Print { .. });
        matches!(&commands[1], Command::Delete { .. });
        matches!(&commands[2], Command::Quit { .. });
    }

    #[test]
    fn test_parser_test_neg_command() {
        let input = "T";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::TestNeg {
                range,
                negated,
                label,
            } => {
                assert!(!negated);
                assert!(range.is_none());
                assert_eq!(label, "");
            }
            _ => panic!("Expected TestNeg command"),
        }
    }

    #[test]
    fn test_parser_test_neg_with_label() {
        let input = "T end";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::TestNeg {
                range,
                negated,
                label,
            } => {
                assert!(!negated);
                assert!(range.is_none());
                assert_eq!(label, "end");
            }
            _ => panic!("Expected TestNeg command with label"),
        }
    }

    #[test]
    fn test_parser_execute_command() {
        let input = "e";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Execute {
                range,
                negated,
                command,
            } => {
                assert!(!negated);
                assert!(range.is_none());
                assert_eq!(command, &None);
            }
            _ => panic!("Expected Execute command"),
        }
    }

    #[test]
    fn test_parser_execute_with_argument() {
        // Note: Due to tokenization limitations and conflict with label 'e',
        // multi-word arguments are not fully supported. Single-word works.
        let input = "e pwd";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::Execute {
                range,
                negated,
                command,
            } => {
                assert!(!negated);
                assert!(range.is_none());
                assert_eq!(command, &Some("pwd".to_string()), "Command should be 'pwd'");
            }
            _ => panic!("Expected Execute command with argument"),
        }
    }

    #[test]
    fn test_parser_print_first_line_command() {
        let input = "P";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::PrintFirstLine { range, negated } => {
                assert!(!negated);
                assert!(range.is_none());
            }
            _ => panic!("Expected PrintFirstLine command"),
        }
    }

    #[test]
    fn test_parser_print_first_line_with_address() {
        let input = "2,5P";
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        let ctx = Context::new_test();
        let mut parser = Parser::new(tokens, &ctx);
        let commands = parser.parse().unwrap();
        assert_eq!(commands.len(), 1);

        match &commands[0] {
            Command::PrintFirstLine { range, negated } => {
                assert!(!negated);
                assert!(range.is_some());
                let range = range.as_ref().unwrap();
                assert_eq!(range.start, Some(Address::Line(2)));
                assert_eq!(range.end, Some(Address::Line(5)));
            }
            _ => panic!("Expected PrintFirstLine command with address range"),
        }
    }
}
