// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Configuration and program validation layer.
//!
//! Provides validation functions for CLI argument combinations,
//! runtime configuration constraints, and option interaction checks.

use crate::errors::{Result, SedError};
use crate::RunConfig;

/// Validate RunConfig for constraint violations
///
/// Checks:
/// - In-place editing requires at least one input file
/// - Option combinations are valid
/// - Configuration is internally consistent
///
/// This should be called after CLI parsing but before program execution.
pub fn validate_config(config: &RunConfig) -> Result<()> {
    // Validation: -i (in-place editing) requires at least one input file
    if config.in_place.is_some() && config.input_files.is_empty() {
        return Err(SedError::inplace("no input files"));
    }

    // Future validations can be added here:
    // - Check incompatible option combinations
    // - Validate file paths
    // - Check permission requirements
    // etc.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::DEFAULT_LINE_LENGTH;
    use crate::errors::ScriptSource;

    #[test]
    fn test_validate_in_place_requires_files() {
        let config = RunConfig {
            scripts_with_sources: vec![(
                "s/a/b/".to_string(),
                b"s/a/b/".to_vec(),
                ScriptSource::Expression(0),
            )],
            input_files: vec![],
            quiet: false,
            in_place: Some(String::new()),
            extended_regex: false,
            separate_files: false,
            line_length: DEFAULT_LINE_LENGTH,
            unbuffered: false,
            posix: false,
            strict_posix: false,
            follow_symlinks: false,
            sandbox: false,
            null_data: false,
            binary: false,
        };

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no input files"));
    }

    #[test]
    fn test_validate_in_place_with_files_ok() {
        let config = RunConfig {
            scripts_with_sources: vec![(
                "s/a/b/".to_string(),
                b"s/a/b/".to_vec(),
                ScriptSource::Expression(0),
            )],
            input_files: vec!["file.txt".to_string()],
            quiet: false,
            in_place: Some(String::new()),
            extended_regex: false,
            separate_files: false,
            line_length: DEFAULT_LINE_LENGTH,
            unbuffered: false,
            posix: false,
            strict_posix: false,
            follow_symlinks: false,
            sandbox: false,
            null_data: false,
            binary: false,
        };

        let result = validate_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_no_in_place_ok() {
        let config = RunConfig {
            scripts_with_sources: vec![(
                "s/a/b/".to_string(),
                b"s/a/b/".to_vec(),
                ScriptSource::Expression(0),
            )],
            input_files: vec![],
            quiet: false,
            in_place: None,
            extended_regex: false,
            separate_files: false,
            line_length: DEFAULT_LINE_LENGTH,
            unbuffered: false,
            posix: false,
            strict_posix: false,
            follow_symlinks: false,
            sandbox: false,
            null_data: false,
            binary: false,
        };

        let result = validate_config(&config);
        assert!(result.is_ok());
    }
}
