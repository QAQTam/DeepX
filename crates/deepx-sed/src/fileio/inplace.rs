// Copyright (c) 2026 Red Authors
// License: MIT
//

//! In-place file editing utilities

/// Expand '*' in backup suffix according to GNU sed rules
///
/// If the file path contains a directory separator:
///   - Each '*' is replaced with the full file path
///   - Example: "***" with file "./e" becomes "./e./e./e"
///
/// If the file path is just a basename (no directory):
///   - Each '*' is replaced with just the basename
///   - Example: "==*==" with file "c" becomes "==c=="
///   - The result is placed in the parent directory of the original file
pub fn expand_backup_suffix(file_path: &str, suffix: &str) -> String {
    if !suffix.contains('*') {
        // No wildcard - just append suffix
        return format!("{}{}", file_path, suffix);
    }

    // Determine what to replace '*' with based on whether path contains directory
    let replacement = if file_path.contains('/') {
        // Path contains directory - use full path
        file_path
    } else {
        // Simple filename - use just the basename
        std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file_path)
    };

    // Replace each '*' with the appropriate value
    suffix.replace('*', replacement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_backup_suffix_no_wildcard() {
        assert_eq!(expand_backup_suffix("file.txt", ".bak"), "file.txt.bak");
    }

    #[test]
    fn test_expand_backup_suffix_with_wildcard() {
        assert_eq!(expand_backup_suffix("file.txt", "*.bak"), "file.txt.bak");
    }

    #[test]
    fn test_expand_backup_suffix_multiple_wildcards() {
        assert_eq!(expand_backup_suffix("file", "***"), "filefilefile");
    }

    #[test]
    fn test_expand_backup_suffix_with_path() {
        assert_eq!(
            expand_backup_suffix("./dir/file", "***"),
            "./dir/file./dir/file./dir/file"
        );
    }

    #[test]
    fn test_expand_backup_suffix_prefix_and_suffix() {
        assert_eq!(expand_backup_suffix("c", "==*=="), "==c==");
    }
}
