// Copyright (c) 2026 Red Authors
// License: MIT
//

//! POSIX compliance rules for sed commands
//!
//! This module centralizes all POSIX compatibility checks in one place.
//! Instead of scattering `if self.posix { ... }` checks throughout
//! parser and lexer, all rules are documented and enforced here.
//!
//! ## POSIX Mode Levels
//!
//! - **Extended** (default): All GNU extensions enabled
//! - **Correct** (POSIXLY_CORRECT): Some behavioral changes, but GNU extensions allowed
//! - **Basic** (--posix): Strict POSIX compliance, GNU extensions rejected
//!
//! This module enforces rules for **Basic** mode only (--posix flag).

use crate::errors::{Result, SedError};

/// Commands that are GNU extensions (not allowed in strict POSIX mode)
///
/// These commands will be rejected with "unknown command" error
/// when running with --posix flag.
///
/// Reference: GNU sed manual section "GNU Extensions"
const GNU_EXTENSION_COMMANDS: &[char] = &[
    'Q', // Quit immediately (GNU)
    'T', // Branch if no substitution (GNU)
    'e', // Execute shell command (GNU)
    'v', // Version check (GNU)
    'z', // Clear pattern space (GNU)
    'f', // Print sed script filename (GNU)
    'F', // Print input filename (GNU)
    'W', // Write first line (GNU)
    'R', // Read line from file (GNU)
];

/// Substitution flags that are GNU extensions
///
/// These flags will be rejected with "unknown option to 's'" error
/// when running with --posix flag.
const GNU_EXTENSION_SUBST_FLAGS: &[char] = &[
    'i', // Case-insensitive matching (GNU)
    'I', // Case-insensitive matching (GNU)
    'm', // Multiline mode (GNU)
    'M', // Multiline-dotall mode (GNU)
    'e', // Execute replacement as shell command (GNU)
];

/// Commands that only accept one address in POSIX mode
///
/// In GNU sed, these can have address ranges (e.g., "1,10i\text").
/// In POSIX mode, they can only have a single address (e.g., "5i\text").
const SINGLE_ADDRESS_ONLY_COMMANDS: &[char] = &[
    'a', // Append text
    'i', // Insert text
    '=', // Print line number
    'l', // List pattern space
    'r', // Read file
];

/// Check if a command is a GNU extension (not allowed in strict POSIX mode)
///
/// # Arguments
/// * `cmd` - The command character to check
///
/// # Returns
/// * `true` if this is a GNU extension command
/// * `false` if this is a standard POSIX command
///
/// # Example
/// ```
/// use red::posix_rules::is_gnu_extension_command;
/// assert!(is_gnu_extension_command('Q'));  // GNU extension
/// assert!(!is_gnu_extension_command('d')); // POSIX standard
/// ```
pub fn is_gnu_extension_command(cmd: char) -> bool {
    GNU_EXTENSION_COMMANDS.contains(&cmd)
}

/// Check if a substitution flag is a GNU extension
///
/// # Arguments
/// * `flag` - The substitution flag character to check
///
/// # Returns
/// * `true` if this is a GNU extension flag
/// * `false` if this is a standard POSIX flag
///
/// # Example
/// ```
/// use red::posix_rules::is_gnu_extension_subst_flag;
/// assert!(is_gnu_extension_subst_flag('i'));  // GNU extension
/// assert!(!is_gnu_extension_subst_flag('g')); // POSIX standard
/// ```
pub fn is_gnu_extension_subst_flag(flag: char) -> bool {
    GNU_EXTENSION_SUBST_FLAGS.contains(&flag)
}

/// Check if a command requires single address in POSIX mode
///
/// # Arguments
/// * `cmd` - The command character to check
///
/// # Returns
/// * `true` if this command only accepts one address in POSIX mode
/// * `false` if this command can have address ranges in POSIX mode
pub fn requires_single_address_in_posix(cmd: char) -> bool {
    SINGLE_ADDRESS_ONLY_COMMANDS.contains(&cmd)
}

/// Validate command is allowed in strict POSIX mode
///
/// # Arguments
/// * `cmd` - The command character to validate
/// * `strict_posix` - Whether strict POSIX mode is enabled (--posix flag)
/// * `error_pos` - Character position for error reporting
///
/// # Returns
/// * `Ok(())` if command is allowed
/// * `Err(SedError)` if command is a GNU extension in strict POSIX mode
///
/// # Example
/// ```
/// use red::posix_rules::validate_command_posix;
/// assert!(validate_command_posix('d', true, 5).is_ok());   // OK - 'd' is POSIX
/// assert!(validate_command_posix('Q', true, 5).is_err());  // Error - 'Q' is GNU extension
/// assert!(validate_command_posix('Q', false, 5).is_ok());  // OK - not in strict POSIX mode
/// ```
pub fn validate_command_posix(cmd: char, strict_posix: bool, error_pos: usize) -> Result<()> {
    if strict_posix && is_gnu_extension_command(cmd) {
        return Err(SedError::parse_at(
            format!("unknown command: '{}'", cmd),
            error_pos,
        ));
    }
    Ok(())
}

/// Validate substitution flag is allowed in strict POSIX mode
///
/// # Arguments
/// * `flag` - The substitution flag character to validate
/// * `strict_posix` - Whether strict POSIX mode is enabled (--posix flag)
/// * `error_pos` - Character position for error reporting
///
/// # Returns
/// * `Ok(())` if flag is allowed
/// * `Err(SedError)` if flag is a GNU extension in strict POSIX mode
pub fn validate_subst_flag_posix(flag: char, strict_posix: bool, error_pos: usize) -> Result<()> {
    if strict_posix && is_gnu_extension_subst_flag(flag) {
        return Err(SedError::parse_at("unknown option to 's'", error_pos));
    }
    Ok(())
}

/// Validate address range for command in strict POSIX mode
///
/// Some commands (a, i, =, l, r) only accept single addresses in POSIX mode.
/// In GNU mode, they can accept ranges (e.g., "1,10a\text").
///
/// # Arguments
/// * `cmd` - The command character
/// * `has_range` - Whether the command has an address range (two addresses)
/// * `strict_posix` - Whether strict POSIX mode is enabled
/// * `error_pos` - Character position for error reporting
///
/// # Returns
/// * `Ok(())` if address range is valid
/// * `Err(SedError)` if command has two addresses in strict POSIX mode but only accepts one
pub fn validate_address_range_posix(
    cmd: char,
    has_range: bool,
    strict_posix: bool,
    error_pos: usize,
) -> Result<()> {
    if strict_posix && has_range && requires_single_address_in_posix(cmd) {
        return Err(SedError::parse_at(
            "command only uses one address",
            error_pos,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gnu_extension_commands() {
        // GNU extensions
        assert!(is_gnu_extension_command('Q'));
        assert!(is_gnu_extension_command('T'));
        assert!(is_gnu_extension_command('e'));
        assert!(is_gnu_extension_command('v'));
        assert!(is_gnu_extension_command('z'));
        assert!(is_gnu_extension_command('f'));
        assert!(is_gnu_extension_command('F'));
        assert!(is_gnu_extension_command('W'));
        assert!(is_gnu_extension_command('R'));

        // POSIX standard commands
        assert!(!is_gnu_extension_command('d'));
        assert!(!is_gnu_extension_command('p'));
        assert!(!is_gnu_extension_command('s'));
        assert!(!is_gnu_extension_command('a'));
        assert!(!is_gnu_extension_command('i'));
    }

    #[test]
    fn test_gnu_extension_subst_flags() {
        // GNU extensions
        assert!(is_gnu_extension_subst_flag('i'));
        assert!(is_gnu_extension_subst_flag('I'));
        assert!(is_gnu_extension_subst_flag('m'));
        assert!(is_gnu_extension_subst_flag('M'));
        assert!(is_gnu_extension_subst_flag('e'));

        // POSIX standard flags
        assert!(!is_gnu_extension_subst_flag('g'));
        assert!(!is_gnu_extension_subst_flag('p'));
        assert!(!is_gnu_extension_subst_flag('w'));
    }

    #[test]
    fn test_single_address_commands() {
        // Commands requiring single address in POSIX
        assert!(requires_single_address_in_posix('a'));
        assert!(requires_single_address_in_posix('i'));
        assert!(requires_single_address_in_posix('='));
        assert!(requires_single_address_in_posix('l'));
        assert!(requires_single_address_in_posix('r'));

        // Commands accepting ranges
        assert!(!requires_single_address_in_posix('d'));
        assert!(!requires_single_address_in_posix('p'));
        assert!(!requires_single_address_in_posix('s'));
    }

    #[test]
    fn test_validate_command_posix() {
        // POSIX command is OK in both modes
        assert!(validate_command_posix('d', false, 0).is_ok());
        assert!(validate_command_posix('d', true, 0).is_ok());

        // GNU extension OK in non-POSIX mode
        assert!(validate_command_posix('Q', false, 0).is_ok());

        // GNU extension rejected in POSIX mode
        let result = validate_command_posix('Q', true, 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown command"));
    }

    #[test]
    fn test_validate_subst_flag_posix() {
        // POSIX flag OK in both modes
        assert!(validate_subst_flag_posix('g', false, 0).is_ok());
        assert!(validate_subst_flag_posix('g', true, 0).is_ok());

        // GNU extension OK in non-POSIX mode
        assert!(validate_subst_flag_posix('i', false, 0).is_ok());

        // GNU extension rejected in POSIX mode
        let result = validate_subst_flag_posix('i', true, 7);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown option to 's'"));
    }

    #[test]
    fn test_validate_address_range_posix() {
        // 'd' command accepts ranges in both modes
        assert!(validate_address_range_posix('d', true, false, 0).is_ok());
        assert!(validate_address_range_posix('d', true, true, 0).is_ok());

        // 'a' command accepts range in GNU mode
        assert!(validate_address_range_posix('a', true, false, 0).is_ok());

        // 'a' command rejects range in POSIX mode
        let result = validate_address_range_posix('a', true, true, 3);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only uses one address"));

        // 'a' command accepts single address in POSIX mode
        assert!(validate_address_range_posix('a', false, true, 0).is_ok());
    }
}
