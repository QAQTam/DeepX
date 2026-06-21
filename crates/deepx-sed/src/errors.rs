// Copyright (c) 2026 Red Authors
// License: MIT
//

//! GNU sed-compatible error handling and messages

use std::fmt;

/// GNU sed-compatible error type with context
#[derive(Debug)]
pub enum SedError {
    /// Parse errors in sed scripts (compile-time)
    Parse {
        message: String,
        line: Option<usize>,
        char_pos: Option<usize>,
        context: ErrorContext,
    },
    /// I/O errors (file not found, permission denied, etc.)
    Io {
        operation: String,
        path: String,
        source: std::io::Error,
    },
    /// Rename errors during in-place editing (exit code 4)
    Rename {
        source_path: String,
        dest_path: String,
        error: std::io::Error,
    },
    /// Runtime errors (undefined labels, etc.)
    Runtime { message: String },
    /// Usage errors (missing arguments, invalid options)
    Usage { message: String },
    /// In-place editing errors (exit code 4)
    InPlace { message: String },
}

/// Context for where the error occurred
#[derive(Debug, Clone)]
pub enum ErrorContext {
    /// Error in -e expression
    Expression { index: usize },
    /// Error in -f script file
    ScriptFile { path: String },
    /// Error with no specific context
    None,
}

/// Source of a script (for error reporting)
#[derive(Debug, Clone)]
pub enum ScriptSource {
    /// Script from -e flag (with 0-based index)
    Expression(usize),
    /// Script from -f flag (with file path)
    File(String),
}

impl ScriptSource {
    /// Convert to ErrorContext
    pub fn to_error_context(&self) -> ErrorContext {
        match self {
            ScriptSource::Expression(index) => ErrorContext::Expression { index: *index },
            ScriptSource::File(path) => ErrorContext::ScriptFile { path: path.clone() },
        }
    }
}

impl fmt::Display for SedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SedError::Parse {
                message,
                line,
                char_pos,
                context,
            } => match context {
                ErrorContext::Expression { index } => {
                    if let Some(ln) = line {
                        if let Some(ch) = char_pos {
                            write!(f, "-e expression #{}, char {}: {}", index + 1, ch, message)
                        } else {
                            write!(f, "-e expression #{}, line {}: {}", index + 1, ln, message)
                        }
                    } else if let Some(ch) = char_pos {
                        write!(f, "-e expression #{}, char {}: {}", index + 1, ch, message)
                    } else {
                        write!(f, "-e expression #{}: {}", index + 1, message)
                    }
                }
                ErrorContext::ScriptFile { path } => {
                    // For file-based scripts, default to line 1 if not specified
                    // (most errors occur on line 1, and full line tracking is complex)
                    let ln = line.unwrap_or(1);
                    write!(f, "file {} line {}: {}", path, ln, message)
                }
                ErrorContext::None => {
                    if let Some(ln) = line {
                        write!(f, "line {}: {}", ln, message)
                    } else {
                        write!(f, "{}", message)
                    }
                }
            },
            SedError::Io {
                operation,
                path,
                source,
            } => {
                // Format error message like GNU sed (without "os error N" suffix)
                let error_str = source.to_string();
                let error_msg = match source.kind() {
                    std::io::ErrorKind::NotFound => "No such file or directory",
                    std::io::ErrorKind::PermissionDenied => "Permission denied",
                    std::io::ErrorKind::AlreadyExists => "File exists",
                    std::io::ErrorKind::InvalidInput => {
                        // Custom error messages (like "symbolic link loop detected")
                        // are passed via InvalidInput - use them as-is
                        error_str.split(" (os error").next().unwrap_or(&error_str)
                    }
                    _ => {
                        // Strip "(os error N)" suffix from other errors
                        error_str.split(" (os error").next().unwrap_or(&error_str)
                    }
                };
                write!(f, "{} {}: {}", operation, path, error_msg)
            }
            SedError::Rename {
                source_path,
                dest_path,
                error,
            } => {
                // Format error message like GNU sed (without "os error N" suffix)
                let error_str = error.to_string();
                let error_msg = match error.kind() {
                    std::io::ErrorKind::NotFound => "No such file or directory",
                    std::io::ErrorKind::PermissionDenied => "Permission denied",
                    std::io::ErrorKind::AlreadyExists => "File exists",
                    _ => error_str.split(" (os error").next().unwrap_or(&error_str),
                };
                write!(
                    f,
                    "cannot rename {} to {}: {}",
                    source_path, dest_path, error_msg
                )
            }
            SedError::Runtime { message } => {
                write!(f, "{}", message)
            }
            SedError::Usage { message } => {
                write!(f, "{}", message)
            }
            SedError::InPlace { message } => {
                write!(f, "{}", message)
            }
        }
    }
}

impl std::error::Error for SedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SedError::Io { source, .. } => Some(source),
            SedError::Rename { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl SedError {
    /// Get the appropriate exit code for this error type
    /// - 0: success (not an error)
    /// - 1: general errors (parse, runtime, usage)
    /// - 2: I/O errors (file could not be opened)
    /// - 4: I/O errors during processing (symlink errors, rename errors, etc.)
    pub fn exit_code(&self) -> i32 {
        match self {
            SedError::Io { operation, .. } => {
                // Symlink-related operations get exit code 4 (like GNU sed)
                if operation.contains("readlink") || operation.contains("follow symlink") {
                    4
                } else {
                    2
                }
            }
            SedError::Rename { .. } => 4,
            SedError::InPlace { .. } => 4,
            SedError::Parse { .. } => 1,
            SedError::Runtime { .. } => 1,
            SedError::Usage { .. } => 1,
        }
    }

    /// Create a parse error
    pub fn parse(message: impl Into<String>) -> Self {
        SedError::Parse {
            message: message.into(),
            line: None,
            char_pos: None,
            context: ErrorContext::None,
        }
    }

    /// Create a parse error with line number
    pub fn parse_line(message: impl Into<String>, line: usize) -> Self {
        SedError::Parse {
            message: message.into(),
            line: Some(line),
            char_pos: None,
            context: ErrorContext::None,
        }
    }

    /// Create a parse error with character position
    pub fn parse_at(message: impl Into<String>, char_pos: usize) -> Self {
        SedError::Parse {
            message: message.into(),
            line: None,
            char_pos: Some(char_pos),
            context: ErrorContext::None,
        }
    }

    /// Set the error context (expression index or script file)
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        if let SedError::Parse {
            context: ref mut ctx,
            ..
        } = self
        {
            *ctx = context;
        }
        self
    }

    /// Create an I/O error
    pub fn io(
        operation: impl Into<String>,
        path: impl Into<String>,
        source: std::io::Error,
    ) -> Self {
        SedError::Io {
            operation: operation.into(),
            path: path.into(),
            source,
        }
    }

    /// Create a rename error (for in-place editing failures)
    pub fn rename(
        source_path: impl Into<String>,
        dest_path: impl Into<String>,
        error: std::io::Error,
    ) -> Self {
        SedError::Rename {
            source_path: source_path.into(),
            dest_path: dest_path.into(),
            error,
        }
    }

    /// Create a runtime error
    pub fn runtime(message: impl Into<String>) -> Self {
        SedError::Runtime {
            message: message.into(),
        }
    }

    /// Create a usage error
    pub fn usage(message: impl Into<String>) -> Self {
        SedError::Usage {
            message: message.into(),
        }
    }

    /// Create an in-place editing error (exit code 4)
    pub fn inplace(message: impl Into<String>) -> Self {
        SedError::InPlace {
            message: message.into(),
        }
    }
}

/// Result type using SedError
pub type Result<T> = std::result::Result<T, SedError>;

/// Convert from std::io::Error
impl From<std::io::Error> for SedError {
    fn from(err: std::io::Error) -> Self {
        SedError::Runtime {
            message: err.to_string(),
        }
    }
}

/// Convert from lexopt::Error (CLI parsing errors)
impl From<lexopt::Error> for SedError {
    fn from(err: lexopt::Error) -> Self {
        SedError::Usage {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_simple() {
        let err = SedError::parse("unknown command: 'X'");
        assert_eq!(err.to_string(), "unknown command: 'X'");
    }

    #[test]
    fn test_parse_error_with_line() {
        let err = SedError::parse_line("unterminated address regex", 5);
        assert_eq!(err.to_string(), "line 5: unterminated address regex");
    }

    #[test]
    fn test_parse_error_with_expression_context() {
        let err = SedError::parse("unknown command: 'X'")
            .with_context(ErrorContext::Expression { index: 0 });
        assert_eq!(err.to_string(), "-e expression #1: unknown command: 'X'");
    }

    #[test]
    fn test_parse_error_with_script_file_context() {
        let err =
            SedError::parse_line("unexpected '}'", 3).with_context(ErrorContext::ScriptFile {
                path: "test.sed".to_string(),
            });
        assert_eq!(err.to_string(), "file test.sed line 3: unexpected '}'");
    }

    #[test]
    fn test_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let err = SedError::io("can't read", "/tmp/test.txt", io_err);
        assert!(err.to_string().contains("can't read /tmp/test.txt"));
    }

    #[test]
    fn test_runtime_error() {
        let err = SedError::runtime("undefined label 'foo'");
        assert_eq!(err.to_string(), "undefined label 'foo'");
    }

    #[test]
    fn test_usage_error() {
        let err = SedError::usage("no input files");
        assert_eq!(err.to_string(), "no input files");
    }

    #[test]
    fn test_parse_error_with_char_pos() {
        let err = SedError::parse_at("unexpected '}'", 10);
        assert!(err.to_string().contains("unexpected '}'"));
    }

    #[test]
    fn test_parse_error_expression_with_line() {
        let err = SedError::Parse {
            message: "error".to_string(),
            line: Some(3),
            char_pos: None,
            context: ErrorContext::Expression { index: 1 },
        };
        assert_eq!(err.to_string(), "-e expression #2, line 3: error");
    }

    #[test]
    fn test_parse_error_expression_with_char_pos() {
        let err = SedError::Parse {
            message: "error".to_string(),
            line: None,
            char_pos: Some(5),
            context: ErrorContext::Expression { index: 0 },
        };
        assert_eq!(err.to_string(), "-e expression #1, char 5: error");
    }

    #[test]
    fn test_parse_error_expression_with_line_and_char_pos() {
        let err = SedError::Parse {
            message: "error".to_string(),
            line: Some(2),
            char_pos: Some(10),
            context: ErrorContext::Expression { index: 0 },
        };
        // char_pos takes precedence over line
        assert_eq!(err.to_string(), "-e expression #1, char 10: error");
    }

    #[test]
    fn test_inplace_error() {
        let err = SedError::inplace("cannot edit in place");
        assert_eq!(err.to_string(), "cannot edit in place");
    }

    #[test]
    fn test_rename_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        let err = SedError::rename("/tmp/src", "/tmp/dst", io_err);
        assert!(err
            .to_string()
            .contains("cannot rename /tmp/src to /tmp/dst"));
        assert!(err.to_string().contains("Permission denied"));
    }

    #[test]
    fn test_rename_error_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "Not found");
        let err = SedError::rename("/tmp/src", "/tmp/dst", io_err);
        assert!(err.to_string().contains("No such file or directory"));
    }

    #[test]
    fn test_rename_error_already_exists() {
        let io_err = std::io::Error::new(std::io::ErrorKind::AlreadyExists, "Exists");
        let err = SedError::rename("/tmp/src", "/tmp/dst", io_err);
        assert!(err.to_string().contains("File exists"));
    }

    #[test]
    fn test_io_error_permission_denied() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        let err = SedError::io("cannot read", "/tmp/test", io_err);
        assert!(err.to_string().contains("Permission denied"));
    }

    #[test]
    fn test_io_error_already_exists() {
        let io_err = std::io::Error::new(std::io::ErrorKind::AlreadyExists, "File exists");
        let err = SedError::io("cannot create", "/tmp/test", io_err);
        assert!(err.to_string().contains("File exists"));
    }

    #[test]
    fn test_io_error_invalid_input() {
        let io_err = std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "symbolic link loop detected (os error 40)",
        );
        let err = SedError::io("readlink", "/tmp/link", io_err);
        assert!(err.to_string().contains("symbolic link loop detected"));
        assert!(!err.to_string().contains("os error"));
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(SedError::parse("error").exit_code(), 1);
        assert_eq!(SedError::runtime("error").exit_code(), 1);
        assert_eq!(SedError::usage("error").exit_code(), 1);
        assert_eq!(SedError::inplace("error").exit_code(), 4);

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "Not found");
        assert_eq!(SedError::io("read", "file", io_err).exit_code(), 2);

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "Not found");
        assert_eq!(SedError::io("readlink", "file", io_err).exit_code(), 4);

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "Not found");
        assert_eq!(SedError::rename("a", "b", io_err).exit_code(), 4);
    }

    #[test]
    fn test_error_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let err = SedError::io("read", "file", io_err);
        assert!(std::error::Error::source(&err).is_some());

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let err = SedError::rename("a", "b", io_err);
        assert!(std::error::Error::source(&err).is_some());

        let err = SedError::parse("test");
        assert!(std::error::Error::source(&err).is_none());

        let err = SedError::runtime("test");
        assert!(std::error::Error::source(&err).is_none());

        let err = SedError::usage("test");
        assert!(std::error::Error::source(&err).is_none());

        let err = SedError::inplace("test");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn test_script_source_to_error_context() {
        let source = ScriptSource::Expression(0);
        if let ErrorContext::Expression { index } = source.to_error_context() {
            assert_eq!(index, 0);
        } else {
            panic!("Expected Expression context");
        }

        let source = ScriptSource::File("test.sed".to_string());
        if let ErrorContext::ScriptFile { path } = source.to_error_context() {
            assert_eq!(path, "test.sed");
        } else {
            panic!("Expected ScriptFile context");
        }
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test error");
        let sed_err: SedError = io_err.into();
        if let SedError::Runtime { message } = sed_err {
            assert!(message.contains("test error"));
        } else {
            panic!("Expected Runtime error");
        }
    }

    #[test]
    fn test_script_file_context_no_line() {
        let err = SedError::Parse {
            message: "error".to_string(),
            line: None,
            char_pos: None,
            context: ErrorContext::ScriptFile {
                path: "test.sed".to_string(),
            },
        };
        // Without line, defaults to line 1
        assert_eq!(err.to_string(), "file test.sed line 1: error");
    }

    #[test]
    fn test_io_error_other_kind() {
        // Test "other" error kinds that go through the default path
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "custom error (os error 123)");
        let err = SedError::io("operation", "/path", io_err);
        assert!(err.to_string().contains("custom error"));
        assert!(!err.to_string().contains("os error"));
    }
}
