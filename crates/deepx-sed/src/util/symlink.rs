// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Symlink resolution utilities

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::constants::MAX_SYMLINK_DEPTH;
use crate::errors::{Result, SedError};

/// Resolve symlink chain, optionally detecting loops and returning errors
///
/// # Arguments
/// * `path` - Path to resolve
/// * `strict` - If true, returns errors on failures; if false, silently breaks
///
/// # Returns
/// - Resolved path following all symlinks
/// - Error if strict=true and loop/depth exceeded/read error
pub fn resolve_symlink_chain(path: &Path, strict: bool) -> Result<PathBuf> {
    let mut current_path = path.to_path_buf();
    let mut seen_paths = HashSet::new();

    loop {
        // Check metadata to see if it's a symlink
        let metadata = match std::fs::symlink_metadata(&current_path) {
            Ok(m) => m,
            Err(e) => {
                if strict {
                    return Err(SedError::io(
                        "couldn't follow symlink",
                        path.display().to_string(),
                        e,
                    ));
                }
                // Non-strict: return current path on error
                return Ok(current_path);
            }
        };

        // If not a symlink, we're done
        if !metadata.file_type().is_symlink() {
            break;
        }

        // Check for loop - must happen AFTER symlink check
        if !seen_paths.insert(current_path.clone()) {
            if strict {
                return Err(SedError::io(
                    "couldn't follow symlink",
                    path.display().to_string(),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "symbolic link loop detected",
                    ),
                ));
            }
            // Non-strict: break on loop
            break;
        }

        // Check depth limit
        if seen_paths.len() > MAX_SYMLINK_DEPTH {
            if strict {
                return Err(SedError::io(
                    "couldn't follow symlink",
                    path.display().to_string(),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "too many levels of symbolic links",
                    ),
                ));
            }
            // Non-strict: break on depth exceeded
            break;
        }

        // Read the symlink target
        match std::fs::read_link(&current_path) {
            Ok(target) => {
                // If target is relative, resolve it relative to the symlink's directory
                if target.is_relative() {
                    if let Some(parent) = current_path.parent() {
                        current_path = parent.join(target);
                    } else {
                        current_path = target;
                    }
                } else {
                    current_path = target;
                }
            }
            Err(e) => {
                if strict {
                    return Err(SedError::io(
                        "couldn't follow symlink",
                        path.display().to_string(),
                        e,
                    ));
                }
                // Non-strict: break on read error
                break;
            }
        }
    }

    Ok(current_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_regular_file() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_symlink_regular.txt");
        fs::write(&test_file, "test").unwrap();

        let result = resolve_symlink_chain(&test_file, true).unwrap();
        assert_eq!(result, test_file);

        fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_symlink() {
        let temp_dir = std::env::temp_dir();
        let target = temp_dir.join("test_symlink_target.txt");
        let link = temp_dir.join("test_symlink_link.txt");

        // Clean up any existing files
        fs::remove_file(&link).ok();
        fs::remove_file(&target).ok();

        fs::write(&target, "test").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();

        #[cfg(unix)]
        {
            let result = resolve_symlink_chain(&link, true).unwrap();
            assert_eq!(result, target);
        }

        fs::remove_file(&link).ok();
        fs::remove_file(&target).ok();
    }

    #[test]
    fn test_nonexistent_strict() {
        let result = resolve_symlink_chain(Path::new("/nonexistent/path/file"), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_nonexistent_nonstrict() {
        let result = resolve_symlink_chain(Path::new("/nonexistent/path/file"), false);
        // Non-strict returns the original path
        assert!(result.is_ok());
    }
}
