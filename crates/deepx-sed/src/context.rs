// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Central configuration context for sed execution.
//!
//! Provides the `Context` struct - a unified container for all configuration
//! parameters used throughout the sed pipeline (parsing, compilation, execution).

use crate::constants::DEFAULT_LINE_LENGTH;
use crate::errors::ScriptSource;

/// POSIX compatibility level
///
/// Mirrors GNU sed's posixicity levels:
/// - `Extended`: Default mode with all GNU extensions enabled
/// - `Correct`: POSIX-compatible with some non-conflicting GNU extensions
/// - `Basic`: Strict POSIX compliance (--posix flag)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosixMode {
    /// GNU extensions enabled (default)
    Extended,
    /// POSIX-compatible with harmless GNU extensions
    Correct,
    /// Strict POSIX mode (--posix flag)
    Basic,
}

impl Default for PosixMode {
    fn default() -> Self {
        PosixMode::Extended
    }
}

impl PosixMode {
    /// Check if running in any POSIX mode (Correct or Basic)
    pub fn is_posix(&self) -> bool {
        matches!(self, PosixMode::Correct | PosixMode::Basic)
    }

    /// Check if running in strict POSIX mode (Basic only)
    pub fn is_strict_posix(&self) -> bool {
        matches!(self, PosixMode::Basic)
    }

    /// Create from boolean flags
    ///
    /// # Arguments
    /// * `posix` - POSIXLY_CORRECT environment variable is set
    /// * `strict_posix` - --posix flag was passed
    pub fn from_flags(posix: bool, strict_posix: bool) -> Self {
        if strict_posix {
            PosixMode::Basic
        } else if posix {
            PosixMode::Correct
        } else {
            PosixMode::Extended
        }
    }
}

/// Central configuration context for sed execution
///
/// Contains all runtime configuration parameters used throughout
/// the sed pipeline: lexing, parsing, compilation, and execution.
///
/// ## Usage
///
/// The `Context` is created from a `RunConfig` and passed to the parser,
/// compiler, and execution engine to provide consistent configuration.
///
/// ```
/// use red::context::Context;
/// // Create a test context with default settings
/// let ctx = Context::new_test();
/// assert!(!ctx.extended_regex);
/// assert!(!ctx.sandbox);
/// ```
#[derive(Debug, Clone)]
pub struct Context {
    // === Core Operation Mode ===
    /// POSIX compatibility level
    pub posix_mode: PosixMode,

    /// Use Extended Regular Expressions (ERE) instead of Basic (BRE)
    /// Controlled by -E/-r flags
    pub extended_regex: bool,

    /// Sandbox mode - disable e/r/w/R/W commands for security
    /// Controlled by --sandbox flag
    pub sandbox: bool,

    // === Output Behavior ===
    /// Suppress automatic printing of pattern space
    /// Controlled by -n flag or #n shebang
    pub quiet: bool,

    /// Treat each input file separately (reset line numbers, addresses, etc.)
    /// Controlled by -s flag
    pub separate_files: bool,

    /// Use NUL (\0) as line separator instead of newline (\n)
    /// Controlled by -z flag
    pub null_data: bool,

    /// Flush output buffer after each line
    /// Controlled by -u flag
    pub unbuffered: bool,

    // === In-Place Editing ===
    /// Edit files in-place with optional backup suffix
    /// None = no in-place editing
    /// Some("") = in-place without backup
    /// Some(suffix) = in-place with backup (original renamed to <file><suffix>)
    pub in_place_suffix: Option<String>,

    /// Follow symbolic links when editing in-place
    /// Controlled by --follow-symlinks flag
    pub follow_symlinks: bool,

    // === Formatting ===
    /// Line width for l (list) command formatting
    pub line_length: usize,

    // === Scripts (for error reporting) ===
    /// Original scripts with their sources (needed for error messages)
    pub scripts_with_sources: Vec<(String, ScriptSource)>,
}

impl Context {
    /// Create Context from RunConfig
    ///
    /// This is the primary way to create a Context during normal execution.
    ///
    /// # Arguments
    /// * `config` - The RunConfig from CLI parsing
    /// * `scripts` - Scripts with their sources and raw bytes (raw bytes stripped for Context)
    pub fn from_run_config(
        config: &crate::RunConfig,
        scripts: Vec<(String, Vec<u8>, ScriptSource)>,
    ) -> Self {
        // Extract just string + source for Context (raw bytes not needed for error messages)
        let scripts_for_context: Vec<(String, ScriptSource)> =
            scripts.into_iter().map(|(s, _raw, src)| (s, src)).collect();
        Context {
            posix_mode: PosixMode::from_flags(config.posix, config.strict_posix),
            extended_regex: config.extended_regex,
            sandbox: config.sandbox,
            quiet: config.quiet,
            separate_files: config.separate_files,
            null_data: config.null_data,
            unbuffered: config.unbuffered,
            in_place_suffix: config.in_place.clone(),
            follow_symlinks: config.follow_symlinks,
            line_length: config.line_length,
            scripts_with_sources: scripts_for_context,
        }
    }

    /// Create a default Context for testing
    ///
    /// Uses sensible defaults:
    /// - GNU extended mode (no POSIX restrictions)
    /// - BRE regex (not ERE)
    /// - No sandbox
    /// - Auto-print enabled (not quiet)
    /// - Single-file mode
    /// - Newline separators (not null-data)
    /// - No buffering control
    /// - No in-place editing
    /// - Default line length
    ///
    /// Note: This is public for use in parser.rs and tests
    pub fn new_test() -> Self {
        Context {
            posix_mode: PosixMode::Extended,
            extended_regex: false,
            sandbox: false,
            quiet: false,
            separate_files: false,
            null_data: false,
            unbuffered: false,
            in_place_suffix: None,
            follow_symlinks: false,
            line_length: DEFAULT_LINE_LENGTH,
            scripts_with_sources: vec![],
        }
    }

    /// Create Context with custom settings for testing
    ///
    /// # Arguments
    /// * `posix_mode` - POSIX compatibility level
    /// * `extended_regex` - Use ERE instead of BRE
    ///
    /// Note: This is public for use in parser.rs and tests
    pub fn new_test_with(posix_mode: PosixMode, extended_regex: bool) -> Self {
        Context {
            posix_mode,
            extended_regex,
            ..Self::new_test()
        }
    }

    // === Convenience Methods ===

    /// Check if running in any POSIX mode
    pub fn is_posix(&self) -> bool {
        self.posix_mode.is_posix()
    }

    /// Check if running in strict POSIX mode
    pub fn is_strict_posix(&self) -> bool {
        self.posix_mode.is_strict_posix()
    }

    /// Check if in-place editing is enabled
    pub fn is_in_place(&self) -> bool {
        self.in_place_suffix.is_some()
    }

    /// Get the in-place backup suffix, if any
    ///
    /// Returns None if:
    /// - Not in in-place mode
    /// - In-place mode without backup (suffix is empty string)
    pub fn backup_suffix(&self) -> Option<&str> {
        self.in_place_suffix.as_ref().and_then(
            |s| {
                if s.is_empty() {
                    None
                } else {
                    Some(s.as_str())
                }
            },
        )
    }
}

impl Default for Context {
    fn default() -> Self {
        Context {
            posix_mode: PosixMode::default(),
            extended_regex: false,
            sandbox: false,
            quiet: false,
            separate_files: false,
            null_data: false,
            unbuffered: false,
            in_place_suffix: None,
            follow_symlinks: false,
            line_length: DEFAULT_LINE_LENGTH,
            scripts_with_sources: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_posix_mode_from_flags() {
        assert_eq!(PosixMode::from_flags(false, false), PosixMode::Extended);
        assert_eq!(PosixMode::from_flags(true, false), PosixMode::Correct);
        assert_eq!(PosixMode::from_flags(false, true), PosixMode::Basic);
        assert_eq!(PosixMode::from_flags(true, true), PosixMode::Basic); // strict_posix takes precedence
    }

    #[test]
    fn test_posix_mode_checks() {
        assert!(!PosixMode::Extended.is_posix());
        assert!(!PosixMode::Extended.is_strict_posix());

        assert!(PosixMode::Correct.is_posix());
        assert!(!PosixMode::Correct.is_strict_posix());

        assert!(PosixMode::Basic.is_posix());
        assert!(PosixMode::Basic.is_strict_posix());
    }

    #[test]
    fn test_context_defaults() {
        let ctx = Context::default();
        assert_eq!(ctx.posix_mode, PosixMode::Extended);
        assert!(!ctx.extended_regex);
        assert!(!ctx.quiet);
        assert!(!ctx.sandbox);
        assert_eq!(ctx.line_length, DEFAULT_LINE_LENGTH);
    }

    #[test]
    fn test_context_convenience_methods() {
        let mut ctx = Context::default();

        // In-place checks
        assert!(!ctx.is_in_place());
        assert!(ctx.backup_suffix().is_none());

        ctx.in_place_suffix = Some("".to_string());
        assert!(ctx.is_in_place());
        assert!(ctx.backup_suffix().is_none()); // Empty suffix = no backup

        ctx.in_place_suffix = Some(".bak".to_string());
        assert!(ctx.is_in_place());
        assert_eq!(ctx.backup_suffix(), Some(".bak"));
    }

    #[test]
    fn test_context_test_helpers() {
        let ctx = Context::new_test();
        assert_eq!(ctx.posix_mode, PosixMode::Extended);
        assert!(!ctx.extended_regex);

        let ctx = Context::new_test_with(PosixMode::Basic, true);
        assert_eq!(ctx.posix_mode, PosixMode::Basic);
        assert!(ctx.extended_regex);
    }
}
