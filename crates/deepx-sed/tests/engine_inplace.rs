// Copyright (c) 2026 Red Authors
// License: MIT
//

use std::fs::{self, OpenOptions};
use std::io::Write;
use tempfile::tempdir;

// Verify that in-place editing restores file mode (permissions)
#[test]
fn inplace_restores_mode() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("file.txt");
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&file_path)
            .unwrap();
        writeln!(f, "hello").unwrap();
    }
    // Make the file executable by user (typical 0o744 baseline)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o744);
        fs::set_permissions(&file_path, perms).unwrap();
    }

    // Run red with -i (no backup), substituting content
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_red"))
        .arg("-i")
        .arg("s/hello/bye/")
        .arg(file_path.to_string_lossy().to_string())
        .status()
        .unwrap();
    assert!(status.success());

    // Mode should be preserved
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let after = fs::metadata(&file_path).unwrap().permissions();
        assert_eq!(after.mode() & 0o777, 0o744);
    }

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content.trim_end(), "bye");
}

// Verify N behavior in-place does not error for a simple case
#[test]
fn inplace_handles_n_basic() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("ncase.txt");
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&file_path)
            .unwrap();
        writeln!(f, "A").unwrap();
        writeln!(f, "B").unwrap();
        writeln!(f, "C").unwrap();
    }
    // Minimal script exercising AppendNextAndResume
    let script = "N";
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_red"))
        .arg("-i")
        .arg("-e")
        .arg(script)
        .arg(file_path.to_string_lossy().to_string())
        .status()
        .unwrap();
    assert!(status.success());
    // Ensure file still exists and has some content
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(!content.is_empty());
}
