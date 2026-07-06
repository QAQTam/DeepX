// Copyright (c) 2026 Red Authors
// License: MIT
//

// Test for Windows CRLF behavior based on GNU sed obinary.sh
// This test verifies the -b (binary) flag works correctly on Windows

#![cfg(windows)]

mod common;

use assert_cmd::Command;
use std::io::Write;
use tempfile::NamedTempFile;

fn bin() -> Command {
    Command::cargo_bin("red").unwrap()
}

#[test]
fn test_platform_uses_crlf() {
    // First check: does red output CRLF by default?
    let mut cmd = bin();
    cmd.arg("p").write_stdin("a");
    let output = cmd.output().unwrap();

    println!("Platform check - output bytes: {:?}", output.stdout);
    println!("Platform check - output len: {}", output.stdout.len());

    // Skip test if platform doesn't use CRLF
    if output.stdout.len() < 3 {
        // Expected: "a\r\n" = 3 bytes
        println!("Platform does not enable CRLF by default, skipping test");
        return;
    }
}

#[test]
fn obinary_input_crlf_no_flag() {
    // Test 1: Input file with CRLF, no -b flag
    // Expected: output should preserve CRLF
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\r\n").unwrap();
    infile.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("s/a/z/").arg(infile.path());
    let output = cmd.output().unwrap();

    println!("Test 1 - Input CRLF, no -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"z\r\n",
        "Input with CRLF should output with CRLF (no -b flag)"
    );
}

#[test]
fn obinary_input_lf_no_flag() {
    // Test 2: Input file with LF only, no -b flag
    // Expected: output should be converted to CRLF on Windows
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\n").unwrap();
    infile.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("s/a/z/").arg(infile.path());
    let output = cmd.output().unwrap();

    println!("Test 2 - Input LF, no -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"z\r\n",
        "Input with LF should output with CRLF on Windows (no -b flag)"
    );
}

#[test]
fn obinary_input_lf_with_b_flag() {
    // Test 3: Input file with LF only, WITH -b flag
    // Expected: output should keep LF (binary mode)
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\n").unwrap();
    infile.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("-b").arg("s/a/z/").arg(infile.path());
    let output = cmd.output().unwrap();

    println!("Test 3 - Input LF, with -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"z\n",
        "Input with LF should output with LF when using -b flag"
    );
}

#[test]
fn obinary_stdin_lf_no_flag() {
    // Test 4: STDIN with LF, no -b flag
    // Expected: output should be CRLF
    let mut cmd = bin();
    cmd.arg("s/a/z/").write_stdin("a\n");
    let output = cmd.output().unwrap();

    println!("Test 4 - STDIN LF, no -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"z\r\n",
        "STDIN with LF should output with CRLF on Windows (no -b flag)"
    );
}

#[test]
fn obinary_stdin_lf_with_b_flag() {
    // Test 5: STDIN with LF, WITH -b flag
    // Expected: output should be LF
    let mut cmd = bin();
    cmd.arg("-b").arg("s/a/z/").write_stdin("a\n");
    let output = cmd.output().unwrap();

    println!("Test 5 - STDIN LF, with -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"z\n",
        "STDIN with LF should output with LF when using -b flag"
    );
}

#[test]
fn obinary_eol_test_no_flag() {
    // Test 6: End-of-line handling with CRLF input, no -b
    // In TEXT mode, \r\n is end-of-line, "y" should be added before \r\n
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\r\n").unwrap();
    infile.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("s/$/y/").arg(infile.path());
    let output = cmd.output().unwrap();

    println!("Test 6 - EOL with CRLF, no -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"ay\r\n",
        "In text mode, $ should match before CRLF, insert 'y' before \\r\\n"
    );
}

#[test]
fn obinary_eol_test_with_b_flag() {
    // Test 7: End-of-line handling with CRLF input, WITH -b
    // In BINARY mode, \r is just a character, "y" should be added after \r
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\r\n").unwrap();
    infile.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("-b").arg("s/$/y/").arg(infile.path());
    let output = cmd.output().unwrap();

    println!("Test 7 - EOL with CRLF, with -b:");
    println!("  Output bytes: {:?}", output.stdout);

    assert_eq!(
        output.stdout, b"a\ry\n",
        "In binary mode, $ should match before \\n only, insert 'y' after \\r"
    );
}

#[test]
fn obinary_inplace_crlf() {
    // Test 8: In-place editing should preserve CRLF
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\r\n").unwrap();
    infile.flush().unwrap();
    let (file, path) = infile.keep().unwrap(); // Keep file for in-place editing
    drop(file);

    let mut cmd = bin();
    cmd.arg("-i").arg("s/a/z/").arg(&path);
    let output = cmd.output().unwrap();

    println!("Test 8 - In-place with CRLF:");
    println!("  Command succeeded: {}", output.status.success());
    if !output.status.success() {
        println!("  stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Read the modified file
    let content = std::fs::read(&path).unwrap();
    println!("  File content after -i: {:?}", content);

    // Cleanup
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        content, b"z\r\n",
        "In-place editing should preserve CRLF in text mode"
    );
}

#[test]
fn obinary_inplace_eol() {
    // Test 9: In-place editing with EOL replacement (text mode)
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\r\n").unwrap();
    infile.flush().unwrap();
    let (file, path) = infile.keep().unwrap();
    drop(file);

    let mut cmd = bin();
    cmd.arg("-i").arg("s/$/y/").arg(&path);
    let output = cmd.output().unwrap();

    println!("Test 9 - In-place EOL, no -b:");
    println!("  Command succeeded: {}", output.status.success());
    if !output.status.success() {
        println!("  stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    let content = std::fs::read(&path).unwrap();
    println!("  File content: {:?}", content);

    // Cleanup
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        content, b"ay\r\n",
        "In-place $ replacement should add before CRLF in text mode"
    );
}

#[test]
fn obinary_inplace_eol_binary() {
    // Test 10: In-place editing with EOL replacement (binary mode)
    let mut infile = NamedTempFile::new().unwrap();
    infile.write_all(b"a\r\n").unwrap();
    infile.flush().unwrap();
    let (file, path) = infile.keep().unwrap();
    drop(file);

    let mut cmd = bin();
    cmd.arg("-b").arg("-i").arg("s/$/y/").arg(&path);
    let output = cmd.output().unwrap();

    println!("Test 10 - In-place EOL, with -b:");
    println!("  Command succeeded: {}", output.status.success());
    if !output.status.success() {
        println!("  stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    let content = std::fs::read(&path).unwrap();
    println!("  File content: {:?}", content);

    // Cleanup
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        content, b"a\ry\n",
        "In-place $ replacement in binary mode should add after \\r"
    );
}
