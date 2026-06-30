// Copyright (c) 2026 Red Authors
// License: MIT
//

//! SELinux security context support for in-place editing.
//!
//! This module provides functions to get and set SELinux security contexts
//! on files, which is needed to preserve contexts during in-place editing.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Check if SELinux is enabled (enforcing or permissive mode).
pub fn is_selinux_enabled() -> bool {
    use selinux::{current_mode, SELinuxMode};
    matches!(
        current_mode(),
        SELinuxMode::Enforcing | SELinuxMode::Permissive
    )
}

/// Get SELinux context from a path.
///
/// If `follow_symlinks` is false, gets the context of the symlink itself (lgetfilecon).
/// If `follow_symlinks` is true, gets the context of the target file (getfilecon).
///
/// Returns `None` if SELinux context cannot be retrieved (e.g., SELinux disabled,
/// filesystem doesn't support xattrs, or file doesn't exist).
pub fn get_context(path: &Path, follow_symlinks: bool) -> Option<String> {
    let path_cstr = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut context: *mut c_char = std::ptr::null_mut();

    let result = unsafe {
        if follow_symlinks {
            selinux_sys::getfilecon(path_cstr.as_ptr(), &mut context)
        } else {
            selinux_sys::lgetfilecon(path_cstr.as_ptr(), &mut context)
        }
    };

    if result < 0 || context.is_null() {
        return None;
    }

    let ctx_str = unsafe { CStr::from_ptr(context) }
        .to_string_lossy()
        .into_owned();

    unsafe { selinux_sys::freecon(context) };

    Some(ctx_str)
}

/// Set SELinux context on a path.
///
/// This function follows symlinks (uses setfilecon, not lsetfilecon).
///
/// Returns an error if the context cannot be set.
pub fn set_context(path: &Path, context: &str) -> std::io::Result<()> {
    let path_cstr = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let ctx_cstr = CString::new(context)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let result = unsafe { selinux_sys::setfilecon(path_cstr.as_ptr(), ctx_cstr.as_ptr()) };

    if result < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_selinux_enabled_returns_bool() {
        // Just verify it doesn't panic - actual result depends on system
        let _ = is_selinux_enabled();
    }

    #[test]
    fn test_get_context_nonexistent_file() {
        let result = get_context(Path::new("/nonexistent/file/path"), false);
        assert!(result.is_none());
    }

    #[test]
    fn test_set_context_nonexistent_file() {
        let result = set_context(
            Path::new("/nonexistent/file/path"),
            "system_u:object_r:tmp_t:s0",
        );
        assert!(result.is_err());
    }
}
