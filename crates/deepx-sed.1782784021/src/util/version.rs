// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Version comparison utilities

use std::cmp::Ordering;

/// Compare two version strings in dotted-decimal format (e.g., "4.9", "4.10.0")
///
/// # Returns
/// - `Ordering::Less` if v1 < v2
/// - `Ordering::Equal` if v1 == v2
/// - `Ordering::Greater` if v1 > v2
///
/// # Examples
/// ```
/// use std::cmp::Ordering;
/// // compare_versions("4.8", "4.9") == Ordering::Less
/// // compare_versions("4.9", "4.9") == Ordering::Equal
/// // compare_versions("4.10", "4.9") == Ordering::Greater
/// ```
pub fn compare_versions(v1: &str, v2: &str) -> Ordering {
    let parts1: Vec<u32> = v1.split('.').filter_map(|s| s.parse().ok()).collect();
    let parts2: Vec<u32> = v2.split('.').filter_map(|s| s.parse().ok()).collect();

    let max_len = parts1.len().max(parts2.len());
    for i in 0..max_len {
        let p1 = parts1.get(i).copied().unwrap_or(0);
        let p2 = parts2.get(i).copied().unwrap_or(0);

        match p1.cmp(&p2) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equal_versions() {
        assert_eq!(compare_versions("4.9", "4.9"), Ordering::Equal);
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn test_less_than() {
        assert_eq!(compare_versions("4.8", "4.9"), Ordering::Less);
        assert_eq!(compare_versions("4.9", "4.10"), Ordering::Less);
        assert_eq!(compare_versions("1.0", "2.0"), Ordering::Less);
    }

    #[test]
    fn test_greater_than() {
        assert_eq!(compare_versions("4.10", "4.9"), Ordering::Greater);
        assert_eq!(compare_versions("5.0", "4.99"), Ordering::Greater);
    }

    #[test]
    fn test_different_lengths() {
        assert_eq!(compare_versions("4.9", "4.9.0"), Ordering::Equal);
        assert_eq!(compare_versions("4.9.1", "4.9"), Ordering::Greater);
        assert_eq!(compare_versions("4.9", "4.9.1"), Ordering::Less);
    }

    #[test]
    fn test_empty_and_invalid() {
        // Empty parts treated as 0
        assert_eq!(compare_versions("", ""), Ordering::Equal);
        assert_eq!(compare_versions("4", "4.0"), Ordering::Equal);
    }
}
