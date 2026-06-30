// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Global constants for Red sed implementation
//!
//! This module centralizes all magic numbers and configuration values
//! to improve maintainability and reduce duplication.

/// GNU sed compatibility version
///
/// Red aims to be compatible with GNU sed 4.9 behavior.
/// Commands or features from later versions will be rejected.
pub const GNU_SED_COMPAT_VERSION: &str = "4.9";

/// Maximum symlink resolution depth
///
/// Matches GNU sed and typical Unix SYMLOOP_MAX limits.
/// Prevents infinite loops in symlink chains.
pub const MAX_SYMLINK_DEPTH: usize = 40;

/// Default line length for 'l' command
///
/// Used when neither -l flag nor COLS environment variable is set.
/// This value matches GNU sed's default.
pub const DEFAULT_LINE_LENGTH: usize = 70;

/// Maximum regex backtracking iterations
///
/// Prevents stack overflow and infinite loops on pathological regex patterns.
/// This limit applies to the custom backtracking engine.
pub const MAX_REGEX_BACKTRACK_ITERATIONS: usize = 100_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_valid() {
        assert!(!GNU_SED_COMPAT_VERSION.is_empty());
        assert!(MAX_SYMLINK_DEPTH > 0);
        assert!(DEFAULT_LINE_LENGTH > 0);
        assert!(MAX_REGEX_BACKTRACK_ITERATIONS > 0);
    }

    #[test]
    fn test_compat_version_format() {
        // Should be in format "major.minor"
        let parts: Vec<&str> = GNU_SED_COMPAT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].parse::<u32>().is_ok());
        assert!(parts[1].parse::<u32>().is_ok());
    }
}
