// Copyright (c) 2026 Red Authors
// License: MIT
//

mod common;

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

fn bin() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::cargo_bin("red").unwrap();
    // On Windows, use binary mode to match Unix behavior (LF instead of CRLF)
    // This allows tests to use the same expectations across platforms
    #[cfg(windows)]
    cmd.arg("-b");
    cmd
}

#[test]
fn prints_input_by_default() {
    let mut cmd = bin();
    cmd.arg("s/.*/X/").write_stdin("abc\n");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("X\n"));
    verify_against_sed!("s/.*/X/", "abc\n", &[]);
}

#[test]
fn quiet_flag_suppresses_default_print() {
    let mut cmd = bin();
    cmd.arg("-n").arg("s/.*/X/").write_stdin("abc\n");
    cmd.assert().success().stdout(predicate::eq(""));
    verify_against_sed!("s/.*/X/", "abc\n", &["-n"]);
}

#[test]
fn prints_help() {
    let mut cmd = bin();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("stream editor"));
}

#[test]
fn requires_script_when_no_args() {
    // GNU sed compatibility: when no script provided, show usage (not just error message)
    let mut cmd = bin();
    cmd.write_stdin("hello\n");
    cmd.assert()
        .code(4) // sed exit code for usage errors
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn exit_code_version_is_zero() {
    let mut cmd = bin();
    cmd.arg("--version");
    cmd.assert().code(0).stdout(predicate::str::contains("red"));
}

#[test]
fn exit_code_invalid_option_is_one() {
    let mut cmd = bin();
    cmd.arg("--invalid-option");
    cmd.assert()
        .code(1)
        .stderr(predicate::str::contains("--invalid-option"));
}

#[test]
fn stdin_and_file_input_cli_path() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "hello").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    cmd.args(["-e", "s/hello/world/"]).arg(tmp.path());
    cmd.assert().success().stdout("world\n");
}

#[test]
fn print_first_line_single_line() {
    let mut cmd = bin();
    cmd.arg("-n").arg("P").write_stdin("hello\n");
    cmd.assert().success().stdout("hello\n");
    verify_against_sed!("P", "hello\n", &["-n"]);
}

#[test]
fn print_first_line_multiline() {
    let mut cmd = bin();
    cmd.arg("-n").arg("N;P").write_stdin("line1\nline2\n");
    cmd.assert().success().stdout("line1\n");
    verify_against_sed!("N;P", "line1\nline2\n", &["-n"]);
}

#[test]
fn print_first_line_with_address() {
    let mut cmd = bin();
    cmd.arg("-n").arg("2P").write_stdin("line1\nline2\nline3\n");
    cmd.assert().success().stdout("line2\n");
    verify_against_sed!("2P", "line1\nline2\nline3\n", &["-n"]);
}

#[test]
fn print_first_line_vs_print_full() {
    // P prints only first line, p prints all
    let mut cmd = bin();
    cmd.arg("-n").arg("N;p").write_stdin("line1\nline2\n");
    let full_output = cmd.assert().success().get_output().stdout.clone();

    let mut cmd2 = bin();
    cmd2.arg("-n").arg("N;P").write_stdin("line1\nline2\n");
    let first_line_output = cmd2.assert().success().get_output().stdout.clone();

    // p should print "line1\nline2\n", P should print "line1\n"
    assert_eq!(String::from_utf8_lossy(&full_output), "line1\nline2\n");
    assert_eq!(String::from_utf8_lossy(&first_line_output), "line1\n");
}

#[test]
fn backref_simple_match() {
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("N;/^\\(.*\\)\\n\\1$/p")
        .write_stdin("hello\nhello\n");
    cmd.assert().success().stdout("hello\nhello\n");
    verify_against_sed!("N;/^\\(.*\\)\\n\\1$/p", "hello\nhello\n", &["-n"]);
}

#[test]
fn backref_no_match() {
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("N;/^\\(.*\\)\\n\\1$/p")
        .write_stdin("hello\nworld\n");
    cmd.assert().success().stdout("");
    verify_against_sed!("N;/^\\(.*\\)\\n\\1$/p", "hello\nworld\n", &["-n"]);
}

#[test]
fn backref_substitution() {
    let mut cmd = bin();
    cmd.arg("s/\\(hello\\) .* \\1/MATCHED/")
        .write_stdin("hello world hello\n");
    cmd.assert().success().stdout("MATCHED\n");
    verify_against_sed!("s/\\(hello\\) .* \\1/MATCHED/", "hello world hello\n", &[]);
}

#[test]
fn backref_multiple_groups() {
    let mut cmd = bin();
    cmd.arg("s/\\(\\w*\\) \\(\\w*\\)/\\2 \\1/")
        .write_stdin("hello world\n");
    cmd.assert().success().stdout("world hello\n");
    verify_against_sed!("s/\\(\\w*\\) \\(\\w*\\)/\\2 \\1/", "hello world\n", &[]);
}

#[test]
fn test_neg_branches_when_no_substitution() {
    let mut cmd = bin();
    cmd.arg("T end; s/x/y/; :end; s/$/AFTER/")
        .write_stdin("hello\n");
    cmd.assert().success().stdout("helloAFTER\n");
    verify_against_sed!("T end; s/x/y/; :end; s/$/AFTER/", "hello\n", &[]);
}

#[test]
fn test_neg_no_branch_after_substitution() {
    let mut cmd = bin();
    cmd.arg("s/h/H/; T end; s/$/SKIPPED/; :end; s/$/AFTER/")
        .write_stdin("hello\n");
    cmd.assert().success().stdout("HelloSKIPPEDAFTER\n");
    verify_against_sed!(
        "s/h/H/; T end; s/$/SKIPPED/; :end; s/$/AFTER/",
        "hello\n",
        &[]
    );
}

#[test]
fn test_neg_vs_test_opposite_behavior() {
    // t branches when substitution occurs
    let mut cmd1 = bin();
    cmd1.arg("s/h/H/; t end; s/$/SKIPPED/; :end; s/$/AFTER/")
        .write_stdin("hello\n");
    let output_t = cmd1.assert().success().get_output().stdout.clone();

    // T branches when NO substitution occurs
    let mut cmd2 = bin();
    cmd2.arg("s/x/X/; T end; s/$/SKIPPED/; :end; s/$/AFTER/")
        .write_stdin("hello\n");
    let output_big_t = cmd2.assert().success().get_output().stdout.clone();

    // Both should skip the middle s command but for opposite reasons
    assert_eq!(String::from_utf8_lossy(&output_t), "HelloAFTER\n");
    assert_eq!(String::from_utf8_lossy(&output_big_t), "helloAFTER\n");
}

#[test]
fn test_neg_resets_flag() {
    let mut cmd = bin();
    cmd.arg("s/h/H/; T; s/$/FIRST/; T; s/$/SECOND/")
        .write_stdin("hello\n");
    cmd.assert().success().stdout("HelloFIRSTSECOND\n");
    verify_against_sed!("s/h/H/; T; s/$/FIRST/; T; s/$/SECOND/", "hello\n", &[]);
}

#[test]
fn execute_pattern_space_as_command() {
    let mut cmd = bin();
    cmd.arg("e").write_stdin("echo hello\n");
    cmd.assert().success().stdout("hello\n");
}

#[test]
fn execute_with_explicit_command() {
    let mut cmd = bin();
    // Note: due to tokenization, multi-word commands need quoting or special handling
    // For now, test with single-word command
    cmd.arg("e pwd").write_stdin("ignored\n");
    // When e has explicit command, both command output AND pattern space are printed
    let output = cmd.assert().success().get_output().stdout.clone();
    let output_str = String::from_utf8_lossy(&output);
    assert!(output_str.contains("/red"), "Should contain pwd output");
    assert!(
        output_str.contains("ignored"),
        "Should contain pattern space"
    );
}

#[test]
fn execute_with_address() {
    let mut cmd = bin();
    cmd.arg("2e").write_stdin("echo A\necho B\n");
    cmd.assert().success().stdout("echo A\nB\n");
}

#[test]
fn execute_pattern_space_replaced() {
    let mut cmd = bin();
    cmd.arg("-n").arg("e;p").write_stdin("echo EXEC\n");
    // The 'e' command executes pattern space and replaces it with output
    // Pattern space "echo EXEC" becomes "EXEC" after execution
    // Then 'p' prints the new pattern space "EXEC"
    cmd.assert().success().stdout("EXEC\n");
}

#[test]
fn execute_suppresses_autoprint() {
    let mut cmd = bin();
    cmd.arg("e").write_stdin("pwd\n");
    // Should output only pwd result, not pattern space "pwd"
    let output = cmd.assert().success().get_output().stdout.clone();
    let output_str = String::from_utf8_lossy(&output);
    // Should be path ending with /red, not "pwd\n"
    assert!(output_str.contains("/red"), "Should contain path");
    assert_eq!(output_str.lines().count(), 1, "Should be single line");
}

#[test]
fn version_check_passes_for_old_version() {
    let mut cmd = bin();
    cmd.arg("v 0.0.1; s/test/OK/").write_stdin("test\n");
    cmd.assert().success().stdout("OK\n");
}

#[test]
fn version_check_fails_for_new_version() {
    let mut cmd = bin();
    cmd.arg("v 99.0").write_stdin("test\n");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("newer version"));
}

#[test]
fn version_check_default_4_0() {
    let mut cmd = bin();
    // v without version defaults to 4.0, which we claim compatibility with (4.9)
    cmd.arg("v; s/test/OK/").write_stdin("test\n");
    cmd.assert().success().stdout("OK\n");
}

#[test]
fn version_check_equal_version() {
    let mut cmd = bin();
    cmd.arg("v 1.0.0; s/test/EQUAL/").write_stdin("test\n");
    cmd.assert().success().stdout("EQUAL\n");
}

#[test]
fn clear_pattern_space() {
    let mut cmd = bin();
    cmd.arg("z").write_stdin("hello world\n");
    cmd.assert().success().stdout("\n");
    verify_against_sed!("z", "hello world\n", &[]);
}

#[test]
fn clear_then_substitute() {
    let mut cmd = bin();
    cmd.arg("z; s/.*/REPLACED/").write_stdin("hello\n");
    cmd.assert().success().stdout("REPLACED\n");
    verify_against_sed!("z; s/.*/REPLACED/", "hello\n", &[]);
}

#[test]
fn clear_with_address() {
    let mut cmd = bin();
    cmd.arg("2z").write_stdin("line1\nline2\nline3\n");
    cmd.assert().success().stdout("line1\n\nline3\n");
    verify_against_sed!("2z", "line1\nline2\nline3\n", &[]);
}

#[test]
fn clear_preserves_cycle() {
    let mut cmd = bin();
    cmd.arg("-n").arg("z; p").write_stdin("hello\n");
    // z clears, p prints empty string
    cmd.assert().success().stdout("\n");
}

#[test]
fn print_filename_stdin() {
    let mut cmd = bin();
    cmd.arg("F").write_stdin("test\n");
    cmd.assert().success().stdout("-\ntest\n");
}

#[test]
fn print_filename_with_file() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "content").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("F").arg(tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    let output_str = String::from_utf8_lossy(&output);

    // Should print filename then content
    assert!(
        output_str.contains(tmp.path().to_str().unwrap()),
        "Should contain filename"
    );
    assert!(
        output_str.contains("content"),
        "Should contain file content"
    );
}

#[test]
fn print_filename_with_address() {
    let mut cmd = bin();
    cmd.arg("2F").write_stdin("line1\nline2\nline3\n");
    cmd.assert().success().stdout("line1\n-\nline2\nline3\n");
}

#[test]
fn print_filename_quiet_mode() {
    let mut cmd = bin();
    cmd.arg("-n").arg("F").write_stdin("test\n");
    cmd.assert().success().stdout("-\n");
}

#[test]
fn read_line_basic() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "line1").unwrap();
    writeln!(tmp, "line2").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    let path_str = tmp.path().to_str().unwrap();
    cmd.arg(format!("1R{} ; 2R{}", path_str, path_str))
        .write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("a\nline1\nb\nline2\nc\n");
}

#[test]
fn read_line_eof_silently_ignored() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "X").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    let path_str = tmp.path().to_str().unwrap();
    // Read line 3 times, but file has only 1 line
    cmd.arg(format!("R{}", path_str)).write_stdin("a\nb\nc\n");
    // First line reads X, second and third get EOF (silently ignored)
    cmd.assert().success().stdout("a\nX\nb\nc\n");
}

#[test]
fn read_line_missing_file() {
    let mut cmd = bin();
    cmd.arg("R/nonexistent/file.txt").write_stdin("test\n");
    // Missing file is silently ignored
    cmd.assert().success().stdout("test\n");
}

#[test]
fn read_line_with_address() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "INSERT").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    let path_str = tmp.path().to_str().unwrap();
    cmd.arg(format!("2R{}", path_str))
        .write_stdin("line1\nline2\nline3\n");
    cmd.assert()
        .success()
        .stdout("line1\nline2\nINSERT\nline3\n");
}

#[test]
fn write_first_line_multiline() {
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let path_str = tmp.path().to_str().unwrap().to_string();

    let mut cmd = bin();
    cmd.arg(format!("N; W{}", path_str))
        .write_stdin("line1\nline2\n");
    cmd.assert().success();

    // Check file contains only first line
    let content = std::fs::read_to_string(&path_str).unwrap();
    assert_eq!(content.trim(), "line1");
}

#[test]
fn write_first_line_single_line() {
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let path_str = tmp.path().to_str().unwrap().to_string();

    let mut cmd = bin();
    cmd.arg(format!("W{}", path_str)).write_stdin("hello\n");
    cmd.assert().success();

    // Single line should be written fully
    let content = std::fs::read_to_string(&path_str).unwrap();
    assert_eq!(content.trim(), "hello");
}

#[test]
fn write_first_line_appends() {
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let path_str = tmp.path().to_str().unwrap().to_string();

    let mut cmd = bin();
    cmd.arg(format!("W{}", path_str)).write_stdin("a\nb\n");
    cmd.assert().success();

    // Both lines should each write 'a' and 'b' separately
    let content = std::fs::read_to_string(&path_str).unwrap();
    assert_eq!(content, "a\nb\n");
}

#[test]
fn write_first_line_with_address() {
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let path_str = tmp.path().to_str().unwrap().to_string();

    let mut cmd = bin();
    cmd.arg(format!("2W{}", path_str))
        .write_stdin("line1\nline2\nline3\n");
    cmd.assert().success();

    // Only line 2 should be written
    let content = std::fs::read_to_string(&path_str).unwrap();
    assert_eq!(content.trim(), "line2");
}

#[test]
fn quit_silent_no_print() {
    let mut cmd = bin();
    cmd.arg("2Q").write_stdin("line1\nline2\nline3\n");
    // Q should quit without printing line2 (unlike q which would print it)
    cmd.assert().success().stdout("line1\n");
}

#[test]
fn quit_silent_vs_quit() {
    // q prints pattern space before quitting
    let mut cmd1 = bin();
    cmd1.arg("2q").write_stdin("line1\nline2\n");
    let output_q = cmd1.assert().success().get_output().stdout.clone();

    // Q does NOT print pattern space
    let mut cmd2 = bin();
    cmd2.arg("2Q").write_stdin("line1\nline2\n");
    let output_big_q = cmd2.assert().success().get_output().stdout.clone();

    // q should have printed line2, Q should not
    assert_eq!(String::from_utf8_lossy(&output_q), "line1\nline2\n");
    assert_eq!(String::from_utf8_lossy(&output_big_q), "line1\n");
}

#[test]
fn quit_silent_with_exit_code() {
    let mut cmd = bin();
    cmd.arg("2Q 42").write_stdin("a\nb\n");
    cmd.assert().code(42).stdout("a\n");
}

#[test]
fn quit_silent_quiet_mode() {
    let mut cmd = bin();
    cmd.arg("-n").arg("p; Q").write_stdin("test\n");
    // In quiet mode, Q still doesn't print (unlike q which also wouldn't in -n)
    cmd.assert().success().stdout("test\n");
}

#[test]
fn zero_address_range_first_match() {
    let mut cmd = bin();
    cmd.arg("-n").arg("0,/x/p").write_stdin("x\ny\nz\n");
    // 0,/x/ matches on first line and ends immediately
    cmd.assert().success().stdout("x\n");
    verify_against_sed!("0,/x/p", "x\ny\nz\n", &["-n"]);
}

#[test]
fn zero_address_range_second_match() {
    let mut cmd = bin();
    cmd.arg("-n").arg("0,/y/p").write_stdin("x\ny\nz\n");
    // 0,/y/ starts at first line, matches y on second line
    cmd.assert().success().stdout("x\ny\n");
    verify_against_sed!("0,/y/p", "x\ny\nz\n", &["-n"]);
}

#[test]
fn zero_address_vs_one_address_range() {
    // 1,/x/ matches line 1, then looks for next x (goes to EOF)
    let mut cmd1 = bin();
    cmd1.arg("-n").arg("1,/x/p").write_stdin("x\ny\nz\n");
    let output1 = cmd1.assert().success().get_output().stdout.clone();

    // 0,/x/ checks x on first line and stops
    let mut cmd2 = bin();
    cmd2.arg("-n").arg("0,/x/p").write_stdin("x\ny\nz\n");
    let output2 = cmd2.assert().success().get_output().stdout.clone();

    assert_eq!(String::from_utf8_lossy(&output1), "x\ny\nz\n");
    assert_eq!(String::from_utf8_lossy(&output2), "x\n");
}

#[test]
fn zero_address_read_prepend() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "HEADER").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    let path_str = tmp.path().to_str().unwrap();
    cmd.arg(format!("0r{}", path_str))
        .write_stdin("line1\nline2\n");
    // 0r should insert file BEFORE first line
    cmd.assert().success().stdout("HEADER\nline1\nline2\n");
}

#[test]
fn ere_plus_quantifier() {
    let mut cmd = bin();
    cmd.arg("-E").arg("s/a+/X/").write_stdin("aaa\n");
    cmd.assert().success().stdout("X\n");
    verify_against_sed!("s/a+/X/", "aaa\n", &["-E"]);
}

#[test]
fn ere_question_quantifier() {
    let mut cmd = bin();
    cmd.arg("-E").arg("s/colou?r/MATCH/").write_stdin("color\n");
    cmd.assert().success().stdout("MATCH\n");
    verify_against_sed!("s/colou?r/MATCH/", "color\n", &["-E"]);
}

#[test]
fn ere_alternation() {
    let mut cmd = bin();
    cmd.arg("-E").arg("s/cat|dog/ANIMAL/").write_stdin("cat\n");
    cmd.assert().success().stdout("ANIMAL\n");
    verify_against_sed!("s/cat|dog/ANIMAL/", "cat\n", &["-E"]);
}

#[test]
fn ere_groups_and_backreferences() {
    let mut cmd = bin();
    cmd.arg("-E").arg("s/(t)(e)/\\2\\1/").write_stdin("test\n");
    cmd.assert().success().stdout("etst\n");
    verify_against_sed!("s/(t)(e)/\\2\\1/", "test\n", &["-E"]);
}

#[test]
fn ere_curly_braces() {
    let mut cmd = bin();
    cmd.arg("-E").arg("s/a{2,3}/X/").write_stdin("aaa\n");
    cmd.assert().success().stdout("X\n");
    verify_against_sed!("s/a{2,3}/X/", "aaa\n", &["-E"]);
}

#[test]
fn ere_with_r_flag() {
    let mut cmd = bin();
    cmd.arg("-r").arg("s/he(l)+o/MATCH/").write_stdin("hello\n");
    cmd.assert().success().stdout("MATCH\n");
}

#[test]
fn ere_address_patterns() {
    let mut cmd = bin();
    cmd.arg("-E")
        .arg("-n")
        .arg("/t+/p")
        .write_stdin("t\ntt\nttt\n");
    cmd.assert().success().stdout("t\ntt\nttt\n");
}

#[test]
fn bre_still_works_without_flag() {
    // Without -E, + is literal
    let mut cmd = bin();
    cmd.arg("s/test+/MATCH/").write_stdin("test+\n");
    cmd.assert().success().stdout("MATCH\n");

    // With \+ it's quantifier in BRE
    let mut cmd2 = bin();
    cmd2.arg("s/a\\+/X/").write_stdin("aaa\n");
    cmd2.assert().success().stdout("X\n");
}

#[test]
fn separate_files_resets_line_numbers() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut f1 = NamedTempFile::new().unwrap();
    writeln!(f1, "a").unwrap();
    f1.flush().unwrap();

    let mut f2 = NamedTempFile::new().unwrap();
    writeln!(f2, "b").unwrap();
    f2.flush().unwrap();

    let mut cmd = bin();
    cmd.arg("-s").arg("=").arg(f1.path()).arg(f2.path());
    cmd.assert().success().stdout("1\na\n1\nb\n");
}

#[test]
fn separate_files_resets_hold_space() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut f1 = NamedTempFile::new().unwrap();
    writeln!(f1, "A").unwrap();
    f1.flush().unwrap();

    let mut f2 = NamedTempFile::new().unwrap();
    writeln!(f2, "B").unwrap();
    f2.flush().unwrap();

    // h stores in hold, $!d deletes non-last, g retrieves from hold
    let mut cmd = bin();
    cmd.arg("-s").arg("h; $!d; g").arg(f1.path()).arg(f2.path());
    // With -s: each file outputs its own content (A then B)
    cmd.assert().success().stdout("A\nB\n");
}

#[test]
fn separate_files_vs_continuous() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut f1 = NamedTempFile::new().unwrap();
    writeln!(f1, "x").unwrap();
    f1.flush().unwrap();

    let mut f2 = NamedTempFile::new().unwrap();
    writeln!(f2, "y").unwrap();
    f2.flush().unwrap();

    // Without -s: continuous line numbering
    let mut cmd1 = bin();
    cmd1.arg("=").arg(f1.path()).arg(f2.path());
    let output1 = cmd1.assert().success().get_output().stdout.clone();

    // With -s: reset line numbers
    let mut cmd2 = bin();
    cmd2.arg("-s").arg("=").arg(f1.path()).arg(f2.path());
    let output2 = cmd2.assert().success().get_output().stdout.clone();

    assert_eq!(String::from_utf8_lossy(&output1), "1\nx\n2\ny\n");
    assert_eq!(String::from_utf8_lossy(&output2), "1\nx\n1\ny\n");
}

// Error message tests
#[test]
fn error_unknown_command() {
    let mut cmd = bin();
    cmd.arg("-e").arg("X");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("sed:").and(predicate::str::contains("unknown command")));
}

#[test]
fn error_unterminated_s_command() {
    let mut cmd = bin();
    cmd.arg("-e").arg("s/foo");
    cmd.assert().failure().stderr(
        predicate::str::contains("sed:").and(predicate::str::contains("unterminated 's' command")),
    );
}

#[test]
fn error_undefined_label() {
    let mut cmd = bin();
    cmd.arg("-e").arg("b foo");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("sed: undefined label 'foo'"));
}

#[test]
fn error_cant_read_file() {
    let mut cmd = bin();
    cmd.arg("s/.*/X/").arg("/nonexistent/file/path.txt");
    cmd.assert().failure().stderr(predicate::str::contains(
        "sed: can't read /nonexistent/file/path.txt",
    ));
}

#[test]
fn error_missing_filename_in_w_command() {
    let mut cmd = bin();
    cmd.arg("-e").arg("w");
    cmd.assert().failure().stderr(
        predicate::str::contains("sed:").and(predicate::str::contains(
            "missing filename in r/R/w/W commands",
        )),
    );
}

#[test]
fn error_unterminated_address_regex() {
    let mut cmd = bin();
    cmd.arg("-e").arg("/foo");
    cmd.assert().failure().stderr(
        predicate::str::contains("sed:")
            .and(predicate::str::contains("unterminated address regex")),
    );
}

#[test]
fn error_unterminated_y_command() {
    let mut cmd = bin();
    cmd.arg("-e").arg("y/abc/de");
    cmd.assert().failure().stderr(
        predicate::str::contains("sed:").and(predicate::str::contains("unterminated 'y' command")),
    );
}

#[test]
fn error_y_command_different_lengths() {
    let mut cmd = bin();
    cmd.arg("-e").arg("y/abc/de/");
    cmd.assert().failure().stderr(
        predicate::str::contains("sed:").and(predicate::str::contains(
            "'y' command strings have different lengths",
        )),
    );
}

// Line length tests for -l option
#[test]
fn list_command_default_line_length() {
    let mut cmd = bin();
    cmd.arg("-n").arg("l").write_stdin("short line\n");
    cmd.assert().success().stdout("short line$\n");
}

#[test]
fn list_command_with_custom_line_length_20() {
    let mut cmd = bin();
    cmd.arg("-l")
        .arg("20")
        .arg("-n")
        .arg("l")
        .write_stdin("this is a very long line that should wrap\n");
    let output = cmd.assert().success().get_output().stdout.clone();
    let output_str = String::from_utf8_lossy(&output);
    // Should wrap because line exceeds 20 characters
    assert!(output_str.contains("\\"));
}

#[test]
fn list_command_with_custom_line_length_100() {
    let mut cmd = bin();
    cmd.arg("-l")
        .arg("100")
        .arg("-n")
        .arg("l")
        .write_stdin("this is a moderately long line\n");
    let output = cmd.assert().success().get_output().stdout.clone();
    let output_str = String::from_utf8_lossy(&output);
    // Should NOT wrap because line is under 100 characters
    assert!(output_str.ends_with("$\n"));
    assert!(!output_str.contains("\\$"));
}

#[test]
fn list_command_wraps_at_specified_length() {
    let mut cmd = bin();
    cmd.arg("-l")
        .arg("10")
        .arg("-n")
        .arg("l")
        .write_stdin("0123456789abcdefgh\n");
    let output = cmd.assert().success().get_output().stdout.clone();
    let output_str = String::from_utf8_lossy(&output);
    // With length 10, should wrap multiple times
    assert!(output_str.contains("\\"));
    assert!(output_str.ends_with("$\n"));
}

// ===== Unbuffered output tests =====

#[test]
fn unbuffered_short_flag() {
    // Test that -u flag is accepted and works
    let mut cmd = bin();
    cmd.arg("-u")
        .arg("s/foo/bar/")
        .write_stdin("foo\nfoo\nfoo\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("bar\nbar\nbar\n"));
}

#[test]
fn unbuffered_long_flag() {
    // Test that --unbuffered flag is accepted and works
    let mut cmd = bin();
    cmd.arg("--unbuffered")
        .arg("s/hello/world/")
        .write_stdin("hello\n");
    cmd.assert().success().stdout(predicate::eq("world\n"));
}

#[test]
fn unbuffered_with_print_command() {
    // Test unbuffered mode with explicit p command
    let mut cmd = bin();
    cmd.arg("-u")
        .arg("-n")
        .arg("p")
        .write_stdin("line1\nline2\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("line1\nline2\n"));
}

#[test]
fn unbuffered_with_multiple_lines() {
    // Test unbuffered mode outputs each line immediately
    let mut cmd = bin();
    cmd.arg("-u").arg("s/^/> /").write_stdin("a\nb\nc\nd\ne\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("> a\n> b\n> c\n> d\n> e\n"));
}

#[test]
fn unbuffered_with_quiet_mode() {
    // Test unbuffered mode combined with quiet mode
    let mut cmd = bin();
    cmd.arg("-u")
        .arg("-n")
        .arg("/pattern/p")
        .write_stdin("no match\npattern here\nanother line\npattern again\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("pattern here\npattern again\n"));
}

// ===== POSIX mode tests =====

#[test]
fn posix_flag_accepted() {
    // Test that --posix flag is accepted
    let mut cmd = bin();
    cmd.arg("--posix").arg("s/foo/bar/").write_stdin("foo\n");
    cmd.assert().success().stdout(predicate::eq("bar\n"));
}

#[test]
fn posix_forbids_e_command() {
    // Test that e command is forbidden in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix")
        .arg("e echo test")
        .write_stdin("ignored\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'e'"));
}

#[test]
fn posix_forbids_f_command() {
    // Test that F command is forbidden in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix").arg("F").write_stdin("line1\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'F'"));
}

#[test]
fn posix_forbids_z_command() {
    // Test that z command is forbidden in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix").arg("z").write_stdin("line1\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'z'"));
}

#[test]
fn posix_forbids_big_q_command() {
    // Test that Q command is forbidden in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix").arg("Q").write_stdin("line1\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'Q'"));
}

#[test]
fn posix_forbids_big_t_command() {
    // Test that T command is forbidden in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix").arg("T").write_stdin("line1\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'T'"));
}

#[test]
fn posix_forbids_big_r_command() {
    // Test that R command is forbidden in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix").arg("R /dev/null").write_stdin("line1\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'R'"));
}

#[test]
fn posix_forbids_big_w_command() {
    // Test that W command is forbidden in POSIX mode
    // Use a simple path without special characters to avoid parsing issues
    let mut cmd = bin();
    cmd.arg("--posix")
        .arg("W /tmp/test.txt")
        .write_stdin("line1\n");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown command: 'W'"));
}

#[test]
fn posix_allows_standard_commands() {
    // Test that standard POSIX commands still work
    let mut cmd = bin();
    cmd.arg("--posix")
        .arg("-n")
        .arg("p")
        .write_stdin("line1\nline2\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("line1\nline2\n"));
}

#[test]
fn posix_allows_substitution() {
    // Test that substitution works in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix")
        .arg("s/old/new/g")
        .write_stdin("old old\n");
    cmd.assert().success().stdout(predicate::eq("new new\n"));
}

#[test]
fn posix_allows_delete() {
    // Test that delete command works in POSIX mode
    let mut cmd = bin();
    cmd.arg("--posix")
        .arg("/skip/d")
        .write_stdin("keep\nskip\n");
    cmd.assert().success().stdout(predicate::eq("keep\n"));
}

// ===== Follow symlinks tests =====

#[test]
fn follow_symlinks_flag_accepted() {
    // Test that --follow-symlinks flag is accepted (basic test without symlinks)
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "test line").unwrap();
    let path = temp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.arg("--follow-symlinks")
        .arg("-i")
        .arg("s/test/result/")
        .arg(path);
    cmd.assert().success();

    // Read back the file
    let content = std::fs::read_to_string(path).unwrap();
    assert_eq!(content, "result line\n");
}

#[test]
#[cfg(unix)] // Symlinks work differently on Unix vs Windows
fn follow_symlinks_default_replaces_symlink_with_file() {
    use std::os::unix::fs::symlink;

    // Create a temp file
    let mut target = NamedTempFile::new().unwrap();
    writeln!(target, "original content").unwrap();
    let target_path = target.path().to_str().unwrap().to_string();

    // Create temp directory for symlink
    let temp_dir = tempfile::tempdir().unwrap();
    let link_path = temp_dir.path().join("link");
    symlink(&target_path, &link_path).unwrap();

    // Verify it's a symlink before editing
    assert!(std::fs::symlink_metadata(&link_path)
        .unwrap()
        .file_type()
        .is_symlink());

    // Edit symlink without --follow-symlinks (should replace symlink with regular file)
    let mut cmd = bin();
    cmd.arg("-i")
        .arg("s/original/modified/")
        .arg(link_path.to_str().unwrap());
    cmd.assert().success();

    // Verify the symlink was replaced with a regular file
    let meta = std::fs::symlink_metadata(&link_path).unwrap();
    assert!(
        meta.file_type().is_file(),
        "Expected regular file, got symlink"
    );

    // Verify the content was edited
    let content = std::fs::read_to_string(&link_path).unwrap();
    assert_eq!(content, "modified content\n");

    // Verify the original target file was NOT modified
    let target_content = std::fs::read_to_string(&target_path).unwrap();
    assert_eq!(target_content, "original content\n");
}

#[test]
#[cfg(unix)] // Symlinks work differently on Unix vs Windows
fn follow_symlinks_edits_target() {
    use std::os::unix::fs::symlink;

    // Create a temp file
    let mut target = NamedTempFile::new().unwrap();
    writeln!(target, "original content").unwrap();
    let target_path = target.path().to_str().unwrap().to_string();

    // Create temp directory for symlink
    let temp_dir = tempfile::tempdir().unwrap();
    let link_path = temp_dir.path().join("link");
    symlink(&target_path, &link_path).unwrap();

    // Edit via symlink with --follow-symlinks
    let mut cmd = bin();
    cmd.arg("--follow-symlinks")
        .arg("-i")
        .arg("s/original/modified/")
        .arg(link_path.to_str().unwrap());
    cmd.assert().success();

    // Verify target file was modified
    let content = std::fs::read_to_string(&target_path).unwrap();
    assert_eq!(content, "modified content\n");

    // Verify symlink still exists and points to same target
    assert!(link_path.exists());
    assert!(std::fs::symlink_metadata(&link_path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
#[cfg(unix)]
fn follow_symlinks_with_backup() {
    use std::os::unix::fs::symlink;

    // Create a temp file
    let mut target = NamedTempFile::new().unwrap();
    writeln!(target, "test data").unwrap();
    let target_path = target.path().to_str().unwrap().to_string();

    // Create temp directory for symlink
    let temp_dir = tempfile::tempdir().unwrap();
    let link_path = temp_dir.path().join("link");
    symlink(&target_path, &link_path).unwrap();

    // Edit via symlink with backup
    let mut cmd = bin();
    cmd.arg("--follow-symlinks")
        .arg("-i.bak")
        .arg("s/test/final/")
        .arg(link_path.to_str().unwrap());
    cmd.assert().success();

    // Verify target was modified
    let content = std::fs::read_to_string(&target_path).unwrap();
    assert_eq!(content, "final data\n");

    // Verify backup was created
    let backup_path = format!("{}.bak", target_path);
    let backup_content = std::fs::read_to_string(&backup_path).unwrap();
    assert_eq!(backup_content, "test data\n");
}

// ===== Sandbox mode tests =====

#[test]
fn sandbox_flag_accepted() {
    // Test that --sandbox flag is accepted with safe commands
    let mut cmd = bin();
    cmd.arg("--sandbox").arg("s/foo/bar/").write_stdin("foo\n");
    cmd.assert().success().stdout(predicate::eq("bar\n"));
}

#[test]
fn sandbox_forbids_e_command() {
    // Test that e command is forbidden in sandbox mode
    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg("e echo test")
        .write_stdin("ignored\n");
    cmd.assert().failure().stderr(predicate::str::contains(
        "e/r/w commands disabled in sandbox mode",
    ));
}

#[test]
fn sandbox_forbids_r_command() {
    // Test that r command is forbidden in sandbox mode
    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg("r /dev/null")
        .write_stdin("line1\n");
    cmd.assert().failure().stderr(predicate::str::contains(
        "e/r/w commands disabled in sandbox mode",
    ));
}

#[test]
fn sandbox_forbids_big_r_command() {
    // Test that R command is forbidden in sandbox mode
    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg("R /dev/null")
        .write_stdin("line1\n");
    cmd.assert().failure().stderr(predicate::str::contains(
        "e/r/w commands disabled in sandbox mode",
    ));
}

#[test]
fn sandbox_forbids_w_command() {
    // Test that w command is forbidden in sandbox mode
    use tempfile::NamedTempFile;
    let temp = NamedTempFile::new().unwrap();
    let path = temp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg(format!("w {}", path))
        .write_stdin("line1\n");
    cmd.assert().failure().stderr(predicate::str::contains(
        "e/r/w commands disabled in sandbox mode",
    ));
}

#[test]
fn sandbox_forbids_big_w_command() {
    // Test that W command is forbidden in sandbox mode
    use tempfile::NamedTempFile;
    let temp = NamedTempFile::new().unwrap();
    let path = temp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg(format!("W {}", path))
        .write_stdin("line1\n");
    cmd.assert().failure().stderr(predicate::str::contains(
        "e/r/w commands disabled in sandbox mode",
    ));
}

#[test]
fn sandbox_allows_substitution() {
    // Test that substitution works in sandbox mode
    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg("s/old/new/g")
        .write_stdin("old old\n");
    cmd.assert().success().stdout(predicate::eq("new new\n"));
}

#[test]
fn sandbox_allows_print() {
    // Test that print commands work in sandbox mode
    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg("-n")
        .arg("p")
        .write_stdin("line1\nline2\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("line1\nline2\n"));
}

#[test]
fn sandbox_allows_delete() {
    // Test that delete command works in sandbox mode
    let mut cmd = bin();
    cmd.arg("--sandbox")
        .arg("/skip/d")
        .write_stdin("keep\nskip\n");
    cmd.assert().success().stdout(predicate::eq("keep\n"));
}

// ===== Multibyte/UTF-8 character support tests =====
// Note: GNU sed in C locale outputs non-ASCII bytes as octal escapes in 'l' command

#[test]
fn utf8_list_command_cyrillic() {
    // Test that l command outputs Cyrillic bytes as octal escapes (GNU sed compatible)
    let mut cmd = bin();
    cmd.arg("-n").arg("l").write_stdin("Привіт\n");
    cmd.assert().success().stdout(predicate::eq(
        "\\320\\237\\321\\200\\320\\270\\320\\262\\321\\226\\321\\202$\n",
    ));
}

#[test]
fn utf8_list_command_accented() {
    // Test that l command outputs accented bytes as octal escapes (GNU sed compatible)
    let mut cmd = bin();
    cmd.arg("-n").arg("l").write_stdin("Café naïve\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Caf\\303\\251 na\\303\\257ve$\n"));
}

#[test]
fn utf8_list_command_mixed() {
    // Test that l command handles mix of ASCII and UTF-8 (GNU sed compatible)
    let mut cmd = bin();
    cmd.arg("-n").arg("l").write_stdin("Hello мир world\n");
    cmd.assert().success().stdout(predicate::eq(
        "Hello \\320\\274\\320\\270\\321\\200 world$\n",
    ));
}

#[test]
fn utf8_list_command_emoji() {
    // Test that l command outputs emoji bytes as octal escapes (GNU sed compatible)
    let mut cmd = bin();
    cmd.arg("-n").arg("l").write_stdin("Hello 🌍 world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Hello \\360\\237\\214\\215 world$\n"));
}

#[test]
fn utf8_list_command_chinese() {
    // Test that l command outputs Chinese bytes as octal escapes (GNU sed compatible)
    let mut cmd = bin();
    cmd.arg("-n").arg("l").write_stdin("你好世界\n");
    cmd.assert().success().stdout(predicate::eq(
        "\\344\\275\\240\\345\\245\\275\\344\\270\\226\\347\\225\\214$\n",
    ));
}

#[test]
fn utf8_substitution_works() {
    // Test that substitution works with UTF-8 characters
    let mut cmd = bin();
    cmd.arg("s/світ/world/").write_stdin("Привіт світ\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Привіт world\n"));
}

#[test]
fn utf8_pattern_matching() {
    // Test that pattern matching works with UTF-8
    let mut cmd = bin();
    cmd.arg("/Привіт/d").write_stdin("Привіт\nworld\n");
    cmd.assert().success().stdout(predicate::eq("world\n"));
}

#[test]
fn utf8_transliterate_cyrillic() {
    // Test that y command works with Cyrillic characters
    let mut cmd = bin();
    cmd.arg("y/абв/xyz/").write_stdin("абвгд\n");
    cmd.assert().success().stdout(predicate::eq("xyzгд\n"));
}

#[test]
fn utf8_transliterate_cyrillic_to_cyrillic() {
    // Test transliteration from Cyrillic to Cyrillic
    let mut cmd = bin();
    cmd.arg("y/абв/деж/").write_stdin("абв\n");
    cmd.assert().success().stdout(predicate::eq("деж\n"));
}

#[test]
fn utf8_transliterate_mixed() {
    // Test transliteration with mixed ASCII and UTF-8
    let mut cmd = bin();
    cmd.arg("y/aбв/xдe/").write_stdin("aбв\n");
    cmd.assert().success().stdout(predicate::eq("xдe\n"));
}

#[test]
fn utf8_transliterate_emoji() {
    // Test that y command works with emoji
    let mut cmd = bin();
    cmd.arg("y/🌍🌎/AB/").write_stdin("Hello 🌍 and 🌎\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Hello A and B\n"));
}

#[test]
fn utf8_transliterate_chinese() {
    // Test that y command works with Chinese characters
    let mut cmd = bin();
    cmd.arg("y/你好/ab/").write_stdin("你好世界\n");
    cmd.assert().success().stdout(predicate::eq("ab世界\n"));
}

#[test]
fn utf8_transliterate_accented() {
    // Test that y command works with accented characters
    let mut cmd = bin();
    cmd.arg("y/éàè/xyz/").write_stdin("Café\n");
    cmd.assert().success().stdout(predicate::eq("Cafx\n"));
}

#[test]
fn utf8_transliterate_multiple_lines() {
    // Test that y command works across multiple lines
    let mut cmd = bin();
    cmd.arg("y/аб/xy/").write_stdin("аб\nба\n");
    cmd.assert().success().stdout(predicate::eq("xy\nyx\n"));
}

#[test]
fn utf8_transliterate_preserve_untranslated() {
    // Test that y command preserves characters not in translation set
    let mut cmd = bin();
    cmd.arg("y/а/x/").write_stdin("аб\n");
    cmd.assert().success().stdout(predicate::eq("xб\n"));
}

// ===== Case conversion escapes tests =====

#[test]
fn case_uppercase_all() {
    // Test \U - uppercase all following characters
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\)/\U\1/").write_stdin("hello world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("HELLO WORLD\n"));
}

#[test]
fn case_lowercase_all() {
    // Test \L - lowercase all following characters
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\)/\L\1/").write_stdin("HELLO WORLD\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hello world\n"));
}

#[test]
fn case_uppercase_next() {
    // Test \u - uppercase next character only
    let mut cmd = bin();
    cmd.arg(r"s/\(h\)\(.*\)/\u\1\2/")
        .write_stdin("hello world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Hello world\n"));
}

#[test]
fn case_lowercase_next() {
    // Test \l - lowercase next character only
    let mut cmd = bin();
    cmd.arg(r"s/\(H\)\(.*\)/\l\1\2/")
        .write_stdin("HELLO WORLD\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hELLO WORLD\n"));
}

#[test]
fn case_end_conversion() {
    // Test \E - end case conversion
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\) \(.*\)/\U\1\E \2/")
        .write_stdin("hello world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("HELLO world\n"));
}

#[test]
fn case_uppercase_with_utf8() {
    // Test case conversion with UTF-8 characters
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\)/\U\1/").write_stdin("привіт світ\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("ПРИВІТ СВІТ\n"));
}

#[test]
fn case_lowercase_with_utf8() {
    // Test lowercase with UTF-8 characters
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\)/\L\1/").write_stdin("ПРИВІТ СВІТ\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("привіт світ\n"));
}

#[test]
fn case_mixed_conversions() {
    // Test multiple case conversions in one replacement
    let mut cmd = bin();
    cmd.arg(r"s/\(.\)\(.*\) \(.\)\(.*\)/\u\1\L\2\E \u\3\L\4/")
        .write_stdin("HELLO WORLD\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Hello World\n"));
}

#[test]
fn case_lowercase_then_uppercase_next() {
    // Test \L\u combination - uppercase next overrides lowercase for one char
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\) \(.*\)/\L\u\1\E \L\u\2/")
        .write_stdin("hello world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("Hello World\n"));
}

#[test]
fn case_with_backreferences() {
    // Test case conversion with backreferences
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\) \(.*\)/\U\1\E and \U\2/")
        .write_stdin("hello world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("HELLO and WORLD\n"));
}

#[test]
fn case_uppercase_next_on_group() {
    // Test \u on captured group
    let mut cmd = bin();
    cmd.arg(r"s/\(.*\) \(.*\)/\1 \u\2/")
        .write_stdin("hello world\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hello World\n"));
}

// ===== Extended address forms tests =====

#[test]
fn address_step_basic() {
    // Test addr~step - print every 2nd line starting from line 5
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("5~2p")
        .write_stdin("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    cmd.assert().success().stdout(predicate::eq("5\n7\n9\n"));
    verify_against_sed!("5~2p", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", &["-n"]);
}

#[test]
fn address_step_from_start() {
    // Test addr~step from line 1
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("1~3p")
        .write_stdin("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("1\n4\n7\n10\n"));
    verify_against_sed!("1~3p", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", &["-n"]);
}

#[test]
fn address_step_zero() {
    // Test 0~step - every Nth line starting from line N
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("0~2p")
        .write_stdin("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("2\n4\n6\n8\n10\n"));
    verify_against_sed!("0~2p", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", &["-n"]);
}

#[test]
fn address_range_with_offset() {
    // Test addr,+N - range from addr to addr+N
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("5,+2p")
        .write_stdin("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    cmd.assert().success().stdout(predicate::eq("5\n6\n7\n"));
    verify_against_sed!("5,+2p", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", &["-n"]);
}

#[test]
fn address_range_offset_from_start() {
    // Test start,+offset from first line
    let mut cmd = bin();
    cmd.arg("-n")
        .arg("1,+3p")
        .write_stdin("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    cmd.assert().success().stdout(predicate::eq("1\n2\n3\n4\n"));
}

#[test]
fn address_step_with_substitution() {
    // Test ~step with substitution command
    let mut cmd = bin();
    cmd.arg("1~2s/^/even: /").write_stdin("1\n2\n3\n4\n5\n6\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("even: 1\n2\neven: 3\n4\neven: 5\n6\n"));
}

#[test]
fn address_step_with_delete() {
    // Test ~step with delete command
    let mut cmd = bin();
    cmd.arg("2~2d").write_stdin("1\n2\n3\n4\n5\n6\n");
    cmd.assert().success().stdout(predicate::eq("1\n3\n5\n"));
}

#[test]
fn address_offset_in_range_with_zero() {
    // Test 0,+N range (from first line for N lines)
    let mut cmd = bin();
    cmd.arg("-n").arg("0,+2p").write_stdin("1\n2\n3\n4\n5\n");
    cmd.assert().success().stdout(predicate::eq("1\n2\n3\n"));
}

// ===== Alternative regex delimiter tests =====

#[test]
fn alt_delimiter_pipe_address() {
    // Test \|pattern| as address
    let mut cmd = bin();
    cmd.arg("-n")
        .arg(r"\|/|p")
        .write_stdin("hello/world\ntest\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hello/world\n"));
}

#[test]
fn alt_delimiter_at_address() {
    // Test \@pattern@ as address
    let mut cmd = bin();
    cmd.arg("-n")
        .arg(r"\@hello@p")
        .write_stdin("hello\nworld\n");
    cmd.assert().success().stdout(predicate::eq("hello\n"));
}

#[test]
fn alt_delimiter_hash_address() {
    // Test \#pattern# as address
    let mut cmd = bin();
    cmd.arg("-n").arg(r"\#test#p").write_stdin("test\nother\n");
    cmd.assert().success().stdout(predicate::eq("test\n"));
}

#[test]
fn alt_delimiter_colon_address() {
    // Test \:pattern: as address
    let mut cmd = bin();
    cmd.arg("-n")
        .arg(r"\:world:p")
        .write_stdin("hello\nworld\n");
    cmd.assert().success().stdout(predicate::eq("world\n"));
}

#[test]
fn alt_delimiter_in_range() {
    // Test alternative delimiter in address range
    let mut cmd = bin();
    cmd.arg("-n")
        .arg(r"\|hello|,\|end|p")
        .write_stdin("hello\nworld\nend\nafter\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hello\nworld\nend\n"));
}

#[test]
fn alt_delimiter_with_delete() {
    // Test alternative delimiter with delete command
    let mut cmd = bin();
    cmd.arg(r"\:world:d").write_stdin("hello\nworld\ngoodbye\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hello\ngoodbye\n"));
}

#[test]
fn alt_delimiter_with_substitution() {
    // Test alternative delimiter with substitution
    let mut cmd = bin();
    cmd.arg(r"\@test@s/e/E/g").write_stdin("test line\nother\n");
    cmd.assert()
        .success()
        .stdout(predicate::eq("tEst linE\nother\n"));
}

#[test]
fn alt_delimiter_semicolon() {
    // Test \;pattern; as address
    let mut cmd = bin();
    cmd.arg("-n").arg(r"\;foo;p").write_stdin("foo\nbar\n");
    cmd.assert().success().stdout(predicate::eq("foo\n"));
}

#[test]
fn alt_delimiter_percent() {
    // Test \%pattern% as address
    let mut cmd = bin();
    cmd.arg("-n").arg(r"\%test%p").write_stdin("test\nline\n");
    cmd.assert().success().stdout(predicate::eq("test\n"));
}

// ===== Exit code compatibility tests =====

#[test]
fn exit_code_success() {
    // Test exit code 0 on success
    let mut cmd = bin();
    cmd.arg("s/test/replaced/").write_stdin("test\n");
    cmd.assert().success().code(0);
}

#[test]
fn exit_code_parse_error() {
    // Test exit code 1 for parse errors
    let mut cmd = bin();
    cmd.arg("s/[/");
    cmd.assert().failure().code(1);
}

#[test]
fn exit_code_io_error() {
    // Test exit code 2 for I/O errors (file not found)
    let mut cmd = bin();
    cmd.arg("s/test/replaced/").arg("/nonexistent/file/path");
    cmd.assert().failure().code(2);
}

#[test]
fn exit_code_invalid_command() {
    // Test exit code 1 for unknown command
    let mut cmd = bin();
    cmd.arg("X");
    cmd.assert().failure().code(1);
}

#[test]
fn exit_code_unterminated_regex() {
    // Test exit code 1 for unterminated regex
    let mut cmd = bin();
    cmd.arg("/test");
    cmd.assert().failure().code(1);
}

// ============================================================================
// BRE Compliance Tests (Task 21)
// ============================================================================

#[test]
fn bre_word_boundary_start() {
    // Test \< (word boundary at start of word)
    // Note: \< matches beginning of word, so "worldwide" does NOT match because 'w' is not at word start
    let mut cmd = bin();
    cmd.args(&["-n", r"/\<world\>/p"])
        .write_stdin("hello world\nworldwide\n");
    cmd.assert().success().stdout("hello world\n");
}

#[test]
fn bre_word_boundary_end() {
    // Test \> (word boundary at end of word)
    let mut cmd = bin();
    cmd.args(&[r"s/rld\>/RLD/"])
        .write_stdin("hello world\nworldwide\n");
    cmd.assert().success().stdout("hello woRLD\nworldwide\n");
}

#[test]
fn bre_word_boundary_both() {
    // Test \< and \> together
    let mut cmd = bin();
    cmd.args(&[r"s/\<test\>/TEST/"])
        .write_stdin("test testing retest\n");
    cmd.assert().success().stdout("TEST testing retest\n");
}

#[test]
fn bre_word_boundary_b() {
    // Test \b (word boundary, GNU extension)
    let mut cmd = bin();
    cmd.args(&[r"s/\btest\b/TEST/"])
        .write_stdin("test testing retest\n");
    cmd.assert().success().stdout("TEST testing retest\n");
}

#[test]
fn bre_word_boundary_b_start() {
    // Test \b at start of word
    let mut cmd = bin();
    cmd.args(&[r"s/\bw/W/"]).write_stdin("hello world\n");
    cmd.assert().success().stdout("hello World\n");
}

#[test]
fn bre_word_boundary_big_b() {
    // Test \B (non-word boundary, GNU extension)
    let mut cmd = bin();
    cmd.args(&[r"s/\Bst/ST/"]).write_stdin("testing start\n");
    cmd.assert().success().stdout("teSTing start\n");
}

#[test]
fn bre_posix_class_alpha() {
    // Test [[:alpha:]] character class
    let mut cmd = bin();
    cmd.args(&[r"s/[[:alpha:]]/X/"]).write_stdin("Test123\n");
    cmd.assert().success().stdout("Xest123\n");
}

#[test]
fn bre_posix_class_digit() {
    // Test [[:digit:]] character class
    let mut cmd = bin();
    cmd.args(&[r"s/[[:digit:]]/X/"]).write_stdin("Test123\n");
    cmd.assert().success().stdout("TestX23\n");
}

#[test]
fn bre_posix_class_alnum() {
    // Test [[:alnum:]] character class (alphanumeric)
    let mut cmd = bin();
    cmd.args(&[r"s/[[:alnum:]]\+/WORD/g"])
        .write_stdin("test-123 hello\n");
    cmd.assert().success().stdout("WORD-WORD WORD\n");
}

#[test]
fn bre_posix_class_space() {
    // Test [[:space:]] character class
    let mut cmd = bin();
    cmd.args(&[r"s/[[:space:]]\+/_/g"])
        .write_stdin("hello   world\ttab\n");
    cmd.assert().success().stdout("hello_world_tab\n");
}

#[test]
fn bre_posix_class_upper() {
    // Test [[:upper:]] character class
    let mut cmd = bin();
    cmd.args(&[r"s/[[:upper:]]\+/UP/g"])
        .write_stdin("HELLO world TEST\n");
    cmd.assert().success().stdout("UP world UP\n");
}

#[test]
fn bre_posix_class_lower() {
    // Test [[:lower:]] character class
    let mut cmd = bin();
    cmd.args(&[r"s/[[:lower:]]\+/low/g"])
        .write_stdin("HELLO world TEST\n");
    cmd.assert().success().stdout("HELLO low TEST\n");
}

#[test]
fn bre_posix_class_punct() {
    // Test [[:punct:]] character class (punctuation)
    let mut cmd = bin();
    cmd.args(&[r"s/[[:punct:]]/X/g"])
        .write_stdin("Hello, world!\n");
    cmd.assert().success().stdout("HelloX worldX\n");
}

#[test]
fn bre_posix_class_xdigit() {
    // Test [[:xdigit:]] character class (hex digits: 0-9, a-f, A-F)
    // Note: Letters 'a', 'n', 'd' in "and" are hex digits, so they match too
    let mut cmd = bin();
    cmd.args(&[r"s/[[:xdigit:]]\+/HEX/g"])
        .write_stdin("0xDEADBEEF and 123\n");
    cmd.assert().success().stdout("HEXxHEX HEXnHEX HEX\n");
}

#[test]
fn bre_posix_class_blank() {
    // Test [[:blank:]] character class (space and tab only)
    let mut cmd = bin();
    cmd.args(&[r"s/[[:blank:]]\+/_/g"])
        .write_stdin("hello\tworld test\n");
    cmd.assert().success().stdout("hello_world_test\n");
}

#[test]
fn bre_escape_sequences_tab_newline() {
    // Test \t and \n escape sequences
    let mut cmd = bin();
    cmd.args(&[r"s/\t/TAB/g"]).write_stdin("hello\tworld\n");
    cmd.assert().success().stdout("helloTABworld\n");
}

#[test]
fn bre_combined_word_boundary_posix() {
    // Test combination of word boundaries and POSIX classes
    let mut cmd = bin();
    cmd.args(&[r"s/\<[[:digit:]]\+\>/NUM/g"])
        .write_stdin("test123 456 end789\n");
    cmd.assert().success().stdout("test123 NUM end789\n");
}

#[test]
fn bre_word_boundary_with_repetition() {
    // Test word boundaries with repetition operators
    // Note: "test123" contains digits, so [a-z]\+ only matches "test" not "123"
    let mut cmd = bin();
    cmd.args(&[r"s/\<[a-z]\+\>/WORD/g"])
        .write_stdin("Hello world test123\n");
    cmd.assert().success().stdout("Hello WORD test123\n");
}

#[test]
fn bre_backslash_b_in_character_class() {
    // Test \b inside character class (should be backspace, not word boundary)
    let mut cmd = bin();
    cmd.args(&[r"s/a[\b]c/X/"])
        .write_stdin("a\x08c normal abc\n");
    cmd.assert().success().stdout("X normal abc\n");
}

// ============================================================================
// Advanced Regex Features Tests (Task 24)
// ============================================================================

#[test]
fn regex_collating_symbol_single() {
    // Test collating symbols [.x.] for single characters
    let mut cmd = bin();
    cmd.args(&[r"s/[.t.]/X/"]).write_stdin("test\n");
    cmd.assert().success().stdout("Xest\n");
}

#[test]
fn regex_collating_symbol_multiple() {
    // Test multiple collating symbols in character class
    let mut cmd = bin();
    cmd.args(&[r"s/[[.1.][.2.][.3.]]/X/g"])
        .write_stdin("abc123\n");
    cmd.assert().success().stdout("abcXXX\n");
}

#[test]
fn regex_equivalence_class_single() {
    // Test equivalence class [=x=] for single character
    let mut cmd = bin();
    cmd.args(&[r"s/[=e=]/X/"]).write_stdin("test\n");
    cmd.assert().success().stdout("tXst\n");
}

#[test]
fn regex_equivalence_class_with_unicode() {
    // Test equivalence class with accented characters
    // Note: equivalence matching depends on locale/implementation
    let mut cmd = bin();
    cmd.args(&[r"s/[=e=]/X/g"]).write_stdin("naïve café\n");
    cmd.assert().success().stdout("naïvX café\n");
}

#[test]
fn regex_backreference_simple() {
    // Test simple backreference \1
    let mut cmd = bin();
    cmd.args(&[r"s/\(.\)\1/X/"]).write_stdin("hello\n");
    cmd.assert().success().stdout("heXo\n");
}

#[test]
fn regex_backreference_multiple() {
    // Test multiple backreferences
    let mut cmd = bin();
    cmd.args(&[r"s/\(.\)\(.\)\2\1/[\1\2]/"])
        .write_stdin("abba\n");
    cmd.assert().success().stdout("[ab]\n");
}

#[test]
fn regex_all_posix_classes_combined() {
    // Test combining multiple POSIX character classes
    // Note: space is not matched by alpha/digit/punct, so it remains
    let mut cmd = bin();
    cmd.args(&[r"s/[[:alpha:]]*[[:digit:]]*[[:punct:]]*/WORD/g"])
        .write_stdin("test123, hello456!\n");
    cmd.assert().success().stdout("WORD WORD\n");
}

// ============================================================================
// Signal Handling and Temp File Cleanup Tests (Task 27)
// ============================================================================

#[test]
fn signal_temp_file_cleaned_on_error() {
    // Test that temp files are cleaned up when an error occurs during processing
    use assert_fs::prelude::*;
    use std::fs;

    let temp = assert_fs::TempDir::new().unwrap();
    let input_file = temp.child("input.txt");
    input_file.write_str("line 1\nline 2\nline 3\n").unwrap();

    let input_path = input_file.path().to_str().unwrap();

    // Create a command that will fail (invalid regex)
    let mut cmd = bin();
    cmd.args(&["-i", r"s/\(/replacement/", input_path]);
    cmd.assert().failure();

    // Check that no temp files remain
    let temp_files: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().unwrap().contains("red_temp"))
        .collect();

    assert_eq!(
        temp_files.len(),
        0,
        "Temporary files should be cleaned up on error"
    );

    temp.close().unwrap();
}

#[test]
fn signal_temp_file_not_present_after_success() {
    // Test that temp files are removed after successful in-place edit
    use assert_fs::prelude::*;
    use std::fs;

    let temp = assert_fs::TempDir::new().unwrap();
    let input_file = temp.child("input.txt");
    input_file.write_str("foo\nbar\nbaz\n").unwrap();

    let input_path = input_file.path().to_str().unwrap();

    // Perform successful in-place edit
    let mut cmd = bin();
    cmd.args(&["-i", "s/foo/FOO/", input_path]);
    cmd.assert().success();

    // Check that no temp files remain
    let temp_files: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().unwrap().contains("red_temp"))
        .collect();

    assert_eq!(
        temp_files.len(),
        0,
        "Temporary files should be removed after successful operation"
    );

    // Verify the file was modified
    let content = fs::read_to_string(input_file.path()).unwrap();
    assert_eq!(content, "FOO\nbar\nbaz\n");

    temp.close().unwrap();
}

#[test]
fn signal_backup_created_with_suffix() {
    // Test that backup files are created with the specified suffix
    use assert_fs::prelude::*;
    use std::fs;

    let temp = assert_fs::TempDir::new().unwrap();
    let input_file = temp.child("input.txt");
    input_file.write_str("original\n").unwrap();

    let input_path = input_file.path().to_str().unwrap();
    let backup_path = format!("{}.bak", input_path);

    // Perform in-place edit with backup
    let mut cmd = bin();
    cmd.args(&["-i.bak", "s/original/modified/", input_path]);
    cmd.assert().success();

    // Check that backup exists
    assert!(
        fs::metadata(&backup_path).is_ok(),
        "Backup file should exist"
    );

    // Verify backup content
    let backup_content = fs::read_to_string(&backup_path).unwrap();
    assert_eq!(backup_content, "original\n");

    // Verify modified content
    let modified_content = fs::read_to_string(input_file.path()).unwrap();
    assert_eq!(modified_content, "modified\n");

    // Check no temp files remain
    let temp_files: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().unwrap().contains("red_temp"))
        .collect();

    assert_eq!(temp_files.len(), 0, "No temp files should remain");

    temp.close().unwrap();
}

// ===== Null-data (-z/--null-data) tests =====

#[test]
fn null_data_short_flag() {
    // Test basic null-data mode with -z flag
    let mut cmd = bin();
    cmd.arg("-z")
        .arg("s/^./x/")
        .write_stdin("AB\x00CD\nEF\n\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("xB\x00xD\nEF\n\x00"));
    verify_against_sed_bytes!("s/^./x/", b"AB\x00CD\nEF\n\x00", &["-z"]);
}

#[test]
fn null_data_long_flag() {
    // Test basic null-data mode with --null-data flag
    let mut cmd = bin();
    cmd.arg("--null-data")
        .arg("s/^./x/")
        .write_stdin("AB\x00CD\x00");
    cmd.assert().success().stdout(predicate::eq("xB\x00xD\x00"));
    verify_against_sed_bytes!("s/^./x/", b"AB\x00CD\x00", &["-z"]);
}

#[test]
fn null_data_with_line_numbers() {
    // Test null-data mode with = command (line numbers)
    let mut cmd = bin();
    cmd.arg("-z").arg("=").write_stdin("A\x00B\x00C\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("1\x00A\x002\x00B\x003\x00C\x00"));
}

#[test]
fn null_data_with_print_command() {
    // Test null-data mode with explicit print
    let mut cmd = bin();
    cmd.arg("-zn").arg("p").write_stdin("hello\x00world\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hello\x00world\x00"));
}

#[test]
fn null_data_with_delete() {
    // Test null-data mode with delete command
    let mut cmd = bin();
    cmd.arg("-z").arg("/CD/d").write_stdin("AB\x00CD\x00EF\x00");
    cmd.assert().success().stdout(predicate::eq("AB\x00EF\x00"));
}

#[test]
fn null_data_with_substitution_global() {
    // Test null-data mode with global substitution
    let mut cmd = bin();
    cmd.arg("-z")
        .arg("s/[aeiou]/X/g")
        .write_stdin("hello\x00world\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("hXllX\x00wXrld\x00"));
}

#[test]
fn null_data_preserves_newlines_in_records() {
    // Test that newlines within null-separated records are preserved
    let mut cmd = bin();
    cmd.arg("-z")
        .arg("s/^/>> /")
        .write_stdin("line1\nline2\x00line3\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq(">> line1\nline2\x00>> line3\x00"));
}

#[test]
fn null_data_with_multiple_commands() {
    // Test null-data mode with multiple commands in sequence
    let mut cmd = bin();
    cmd.arg("-z")
        .arg("1s/^/FIRST: /; 2s/^/SECOND: /")
        .write_stdin("A\x00B\x00C\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("FIRST: A\x00SECOND: B\x00C\x00"));
}

#[test]
fn null_data_with_quiet_mode() {
    // Test null-data mode with quiet mode
    let mut cmd = bin();
    cmd.arg("-zn")
        .arg("s/test/FOUND/p")
        .write_stdin("test\x00other\x00test\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("FOUND\x00FOUND\x00"));
}

#[test]
fn null_data_with_address_range() {
    // Test null-data mode with address ranges
    let mut cmd = bin();
    cmd.arg("-z")
        .arg("2,3s/^/>> /")
        .write_stdin("A\x00B\x00C\x00D\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("A\x00>> B\x00>> C\x00D\x00"));
}

#[test]
fn null_data_empty_records() {
    // Test null-data mode with empty records between nulls
    let mut cmd = bin();
    cmd.arg("-z")
        .arg("s/^$/EMPTY/")
        .write_stdin("\x00\x00test\x00");
    cmd.assert()
        .success()
        .stdout(predicate::eq("EMPTY\x00EMPTY\x00test\x00"));
}

#[test]
fn null_data_no_trailing_null() {
    // Test null-data mode when input doesn't end with null
    // GNU sed preserves the lack of trailing separator
    let mut cmd = bin();
    cmd.arg("-z").arg("s/^/>> /").write_stdin("A\x00B");
    cmd.assert().success().stdout(predicate::eq(">> A\x00>> B"));
}
