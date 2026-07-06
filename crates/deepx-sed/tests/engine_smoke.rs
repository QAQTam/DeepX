// Copyright (c) 2026 Red Authors
// License: MIT
//

mod common;

use assert_cmd::Command;
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
fn basic_substitution_once() {
    let mut cmd = bin();
    cmd.args(["-e", "s/foo/bar/"]).write_stdin("foo\nfoo\n");
    cmd.assert().success().stdout("bar\nbar\n");
    verify_against_sed!("s/foo/bar/", "foo\nfoo\n", &[]);
}

#[test]
fn basic_substitution_global() {
    let mut cmd = bin();
    cmd.args(["-e", "s/foo/bar/g"]) // replace all
        .write_stdin("foo foo\n");
    cmd.assert().success().stdout("bar bar\n");
    verify_against_sed!("s/foo/bar/g", "foo foo\n", &[]);
}

#[test]
fn replacement_ampersand_is_whole_match() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[0-9][0-9]*/&X/g"]) // append X to each number
        .write_stdin("a1 b22\n");
    cmd.assert().success().stdout("a1X b22X\n");
    verify_against_sed!("s/[0-9][0-9]*/&X/g", "a1 b22\n", &[]);
}

#[test]
fn backreferences_work() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\([a-z][a-z]*\\)-\\([0-9][0-9]*\\)/\\2-\\1/"]) // \1, \2
        .write_stdin("abc-123\n");
    cmd.assert().success().stdout("123-abc\n");
    verify_against_sed!(
        "s/\\([a-z][a-z]*\\)-\\([0-9][0-9]*\\)/\\2-\\1/",
        "abc-123\n",
        &[]
    );
}

#[test]
fn delimiter_custom() {
    let mut cmd = bin();
    cmd.args(["-e", "s#foo/bar#baz#"]) // custom delimiter '#'
        .write_stdin("foo/bar\n");
    cmd.assert().success().stdout("baz\n");
    verify_against_sed!("s#foo/bar#baz#", "foo/bar\n", &[]);
}

#[test]
fn quiet_mode_suppresses_output() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/x/y/"]).write_stdin("x\n");
    cmd.assert().success().stdout("");
    verify_against_sed!("s/x/y/", "x\n", &["-n"]);
}

#[test]
fn script_from_file() {
    let mut tmp_script = NamedTempFile::new().unwrap();
    writeln!(tmp_script, "s/a/A/g").unwrap();
    tmp_script.flush().unwrap();

    let mut cmd = bin();
    cmd.args(["-f"])
        .arg(tmp_script.path())
        .write_stdin("a aa\n");
    cmd.assert().success().stdout("A AA\n");
}

#[test]
fn multiple_scripts_sequentially() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/A/", "-e", "s/b/B/"])
        .write_stdin("ab\n");
    cmd.assert().success().stdout("AB\n");
}

#[test]
fn empty_script_via_e_flag_succeeds() {
    let mut cmd = bin();
    cmd.args(["-e", ""]);
    cmd.write_stdin("hello\nworld\n");
    cmd.assert().success().code(0).stdout("hello\nworld\n");
}

#[test]
fn empty_script_via_f_flag_succeeds() {
    let tmp_script = NamedTempFile::new().unwrap();
    let mut cmd = bin();
    cmd.args(["-f"]).arg(tmp_script.path());
    cmd.write_stdin("hello\nworld\n");
    cmd.assert().success().code(0).stdout("hello\nworld\n");
}

#[test]
fn empty_script_positional_arg_succeeds() {
    let mut cmd = bin();
    cmd.arg("");
    cmd.write_stdin("hello\nworld\n");
    cmd.assert().success().code(0).stdout("hello\nworld\n");
}

#[test]
fn whitespace_only_script_succeeds() {
    let mut cmd = bin();
    cmd.args(["-e", "   \t  \n  "]);
    cmd.write_stdin("hello\nworld\n");
    cmd.assert().success().code(0).stdout("hello\nworld\n");
}

#[test]
fn multiple_files_processing() {
    let mut tmp1 = NamedTempFile::new().unwrap();
    writeln!(tmp1, "hello world").unwrap();
    tmp1.flush().unwrap();

    let mut tmp2 = NamedTempFile::new().unwrap();
    writeln!(tmp2, "foo bar").unwrap();
    tmp2.flush().unwrap();

    let mut cmd = bin();
    cmd.args(["-e", "s/o/O/g"])
        .arg(tmp1.path())
        .arg(tmp2.path());
    cmd.assert().success().stdout("hellO wOrld\nfOO bar\n");
}

#[test]
fn substitution_delimiter_bracket() {
    let mut cmd = bin();
    cmd.args(["-e", "s[abc[X[g"]).write_stdin("abc abc\n");
    cmd.assert().success().stdout("X X\n");
}

#[test]
fn address_numeric_single_line() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2p"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("b\n");
    verify_against_sed!("2p", "a\nb\nc\n", &["-n"]);
}

#[test]
fn address_last_line_dollar() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "$p"]).write_stdin("x\ny\nz\n");
    cmd.assert().success().stdout("z\n");
    verify_against_sed!("$p", "x\ny\nz\n", &["-n"]);
}

#[test]
fn address_regex_single_line() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "/^foo$/p"])
        .write_stdin("bar\nfoo\nbaz\n");
    cmd.assert().success().stdout("foo\n");
    verify_against_sed!("/^foo$/p", "bar\nfoo\nbaz\n", &["-n"]);
}

#[test]
fn range_numeric_numeric_inclusive() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "1,2p"]).write_stdin("l1\nl2\nl3\n");
    cmd.assert().success().stdout("l1\nl2\n");
    verify_against_sed!("1,2p", "l1\nl2\nl3\n", &["-n"]);
}

#[test]
fn range_numeric_to_dollar() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2,$p"]).write_stdin("l1\nl2\nl3\n");
    cmd.assert().success().stdout("l2\nl3\n");
    verify_against_sed!("2,$p", "l1\nl2\nl3\n", &["-n"]);
}

#[test]
fn range_regex_regex_same_line_single() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "/^x$/,/^x$/p"])
        .write_stdin("a\nx\ny\n");
    // GNU sed behavior: /^x$/,/^x$/ checks end from NEXT line after start matches,
    // so it continues until EOF (no second ^x$ found)
    cmd.assert().success().stdout("x\ny\n");
}

#[test]
fn negation_after_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2!p"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("a\nc\n");
}

#[test]
fn range_with_plus_offset_end() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "/^x$/,+1p"])
        .write_stdin("a\nx\ny\nz\n");
    cmd.assert().success().stdout("x\ny\n");
}

#[test]
fn step_address_matches_every_nth() {
    let mut cmd = bin();
    // From base 0, every 2nd line → 2,4,...
    cmd.args(["-n", "-e", "0~2p"])
        .write_stdin("1\n2\n3\n4\n5\n");
    cmd.assert().success().stdout("2\n4\n");
}

#[test]
fn bre_counted_repetition_exact_pairs() {
    let mut cmd = bin();
    // \(ab\)\{2\} matches "abab"
    cmd.args(["-e", "s/\\(ab\\)\\{2\\}/X/"])
        .write_stdin("abab\naba\n");
    cmd.assert().success().stdout("X\naba\n");
}

#[test]
fn bre_counted_repetition_range() {
    let mut cmd = bin();
    // a\{2,3\} matches aa or aaa
    cmd.args(["-e", "s/a\\{2,3\\}/X/g"])
        .write_stdin("a aa aaa aaaa\n");
    // "a" -> no change; "aa" -> X; "aaa" -> X; "aaaa" -> X + remaining "a"
    cmd.assert().success().stdout("a X X Xa\n");
}

#[test]
fn bre_posix_class_alpha() {
    let mut cmd = bin();
    // [[:alpha:]]\{3\} should match three letters
    cmd.args(["-e", "s/[[:alpha:]]\\{3\\}/X/"])
        .write_stdin("abc-123\n12abc34\n");
    cmd.assert().success().stdout("X-123\n12X34\n");
}

#[test]
fn bre_escape_class_bracket() {
    let mut cmd = bin();
    // character class including ']' and 'a': []a]
    cmd.args(["-e", "s/[]a]/_/g"]).write_stdin("] a b ]a\n");
    cmd.assert().success().stdout("_ _ b __\n");
}

#[test]
fn bre_ignore_case_flag_i() {
    let mut cmd = bin();
    cmd.args(["-e", "s/foo/bar/Ig"])
        .write_stdin("Foo fOo foo\n");
    cmd.assert().success().stdout("bar bar bar\n");
}

#[test]
fn replacement_escape_ampersand_and_backslash() {
    let mut cmd = bin();
    // \\& should render literal '&'; \\\\ -> literal '\\'
    cmd.args(["-e", "s/[0-9][0-9]*/\\&-\\\\/g"]) // number -> &-\\
        .write_stdin("a1 b22 c333\n");
    cmd.assert().success().stdout("a&-\\ b&-\\ c&-\\\n");
}

#[test]
fn substitution_nth_occurrence_flag() {
    let mut cmd = bin();
    // Replace only the 4th occurrence of '.' with 'X'
    cmd.args(["-e", "s/./X/4"]).write_stdin("abcd\n");
    cmd.assert().success().stdout("abcX\n");
}

#[test]
fn substitution_write_flag_appends_to_file() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    let mut cmd = bin();
    // Use -n to avoid default print; rely on p to see output and w file to write
    cmd.args(["-n", "-e"])
        .arg(format!("s/foo/bar/pw {}", path.display()))
        .write_stdin("foo\nxxx\nfoo\n");
    cmd.assert().success().stdout("bar\nbar\n");

    // Verify file content has the substituted lines (each on its own line)
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, "bar\nbar\n");
}

#[test]
fn write_command_with_address_and_range() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    // 2w file writes only line 2
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("2w {} ; p", path.display()))
        .write_stdin("1\n2\n3\n");
    cmd.assert().success().stdout("1\n2\n3\n");
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "2\n");

    // 2,3w also appends line 3
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("2,3w {} ; p", path.display()))
        .write_stdin("1\n2\n3\n4\n");
    cmd.assert().success().stdout("1\n2\n3\n4\n");
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "2\n2\n3\n");
}

#[test]
fn substitution_write_flag_g_and_append_once_per_line() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("s/a/A/gw {} ; p", path.display()))
        .write_stdin("a a a\n");
    cmd.assert().success().stdout("A A A\n");
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "A A A\n");
}

#[test]
fn read_command_basic_and_range_end_only() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    fs::write(tmp.path(), "X\nY\n").unwrap();

    // Basic r: insert after current line
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("r {}", tmp.path().display()))
        .write_stdin("Z\n");
    cmd.assert().success().stdout("X\nY\n");

    // Addressed single-line r
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("2r {} ; p", tmp.path().display()))
        .write_stdin("1\n2\n3\n");
    cmd.assert().success().stdout("1\n2\nX\nY\n3\n");

    // Range r executes on EVERY line in range (lines 2 and 3). With '-n; ... ; p' we print all lines.
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("2,3r {} ; p", tmp.path().display()))
        .write_stdin("1\n2\n3\n4\n");
    cmd.assert().success().stdout("1\n2\nX\nY\n3\nX\nY\n4\n");
}

#[test]
fn read_command_with_regex_and_dollar_addresses() {
    use std::fs;
    let f = NamedTempFile::new().unwrap();
    fs::write(f.path(), "X\nY\n").unwrap();

    // Regex address: insert after line matching /^foo$/
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("/^foo$/r {} ; p", f.path().display()))
        .write_stdin("a\nfoo\nc\n");
    cmd.assert().success().stdout("a\nfoo\nX\nY\nc\n");

    // Dollar address: insert after last line
    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("$r {} ; p", f.path().display()))
        .write_stdin("1\n2\n3\n");
    cmd.assert().success().stdout("1\n2\n3\nX\nY\n");
}

#[test]
fn multiple_read_commands_sequence() {
    use std::fs;
    let f1 = NamedTempFile::new().unwrap();
    let f2 = NamedTempFile::new().unwrap();
    fs::write(f1.path(), "A1\nA2\n").unwrap();
    fs::write(f2.path(), "B1\n").unwrap();

    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!(
            "2r {} ; 3r {} ; p",
            f1.path().display(),
            f2.path().display()
        ))
        .write_stdin("l1\nl2\nl3\nl4\n");
    // After l2 insert A1,A2; after l3 insert B1
    cmd.assert()
        .success()
        .stdout("l1\nl2\nA1\nA2\nl3\nB1\nl4\n");
}

#[test]
fn substitution_n_with_g_priority() {
    let mut cmd = bin();
    // With occurrence N and g both present, replace from N-th occurrence onwards (GNU sed behavior)
    cmd.args(["-e", "s/a/X/2g"]).write_stdin("aaaa\n");
    // occurrences: a a a a -> replace from 2nd onwards → a X X X
    cmd.assert().success().stdout("aXXX\n");
}

#[test]
fn substitution_write_flag_no_match_creates_no_file() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    // Remove the temp file first, we only need the path
    std::fs::remove_file(&path).ok();

    let mut cmd = bin();
    cmd.args(["-e"])
        .arg(format!("s/zzz/XXX/w {}", path.display()))
        .write_stdin("no matches here\n");
    cmd.assert().success();
    assert!(
        !path.exists(),
        "file must not be created when no substitution occurred"
    );
}

#[test]
fn append_timing_with_range_and_order() {
    let mut cmd = bin();
    // Mimic FreeBSD test 4.2 semantics with a smaller input
    // -n
    // 2,4s/^/2-4/
    // s/^/before_a/p
    // /2-4/a\
    // X
    // s/^/after_a/p
    let script = "2,4s/^/2-4/\ns/^/before_a/p\n/2-4/a\\\nX\ns/^/after_a/p";
    cmd.args(["-n", "-e"])
        .arg(script)
        .write_stdin("l1_1\nl1_2\nl1_3\nl1_4\nl1_5\n");
    // Expected: for lines 2..4 the appended line 'X' prints after both p-prints
    let expected = [
        "before_al1_1",
        "after_abefore_al1_1",
        "before_a2-4l1_2",
        "after_abefore_a2-4l1_2",
        "X",
        "before_a2-4l1_3",
        "after_abefore_a2-4l1_3",
        "X",
        "before_a2-4l1_4",
        "after_abefore_a2-4l1_4",
        "X",
        "before_al1_5",
        "after_abefore_al1_5",
    ]
    .join("\n")
        + "\n";
    cmd.assert().success().stdout(expected);
}

#[test]
fn append_with_n_flushes_before_join_and_prints_in_order() {
    let mut cmd = bin();
    // Based on FreeBSD test 4.3 semantics with smaller range 2,2N
    // -n
    // s/^/^/p
    // /l1_/a\
    // app
    // 2,2N
    // s/$/$/p
    let script = "s/^/^/p\n/l1_/a\\\napp\n2,2N\ns/$/$/p";
    cmd.args(["-n", "-e"])
        .arg(script)
        .write_stdin("l1_1\nl1_2\nl1_3\n");
    // For line 1: '^l1_1', '^l1_1$', then 'app'
    // For line 2: '^l1_2', flush 'app', then join with line 3 and print with trailing $
    let expected = ["^l1_1", "^l1_1$", "app", "^l1_2", "app", "^l1_2\nl1_3$"].join("\n") + "\n";
    cmd.assert().success().stdout(expected);
}

#[test]
fn y_command_digits_transliteration() {
    let mut cmd = bin();
    cmd.args(["-e", "y/0123456789/9876543210/"])
        .write_stdin("2019\n");
    cmd.assert().success().stdout("7980\n");
    verify_against_sed!("y/0123456789/9876543210/", "2019\n", &[]);
}

#[test]
fn y_command_custom_delimiter() {
    let mut cmd = bin();
    cmd.args(["-e", "y#abc#XYZ#"]).write_stdin("cab\n");
    cmd.assert().success().stdout("ZXY\n");
    verify_against_sed!("y#abc#XYZ#", "cab\n", &[]);
}

#[test]
fn y_command_escapes_tab() {
    let mut cmd = bin();
    // Map 'a' to TAB using escape \t
    cmd.args(["-e", "y/a/\\t/"]).write_stdin("aXa\n");
    cmd.assert().success().stdout("\tX\t\n");
    verify_against_sed!("y/a/\\t/", "aXa\n", &[]);
}

#[test]
fn y_command_mismatched_sets_error() {
    let mut cmd = bin();
    cmd.args(["-e", "y/ab/XYZ/"]).write_stdin("ab\n");
    cmd.assert().failure().stderr(predicates::str::contains(
        "'y' command strings have different lengths",
    ));
}

#[test]
fn print_command_with_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2p"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("b\n");
}

#[test]
fn line_number_command_with_regex_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "/foo/="])
        .write_stdin("bar\nfoo\nbaz\n");
    cmd.assert().success().stdout("2\n");
}

#[test]
fn list_command_basic_escapes_and_dollar() {
    let mut cmd = bin();
    // Contains TAB, BEL (\x07), backslash, and normal letters
    let input = "a\tb\\c\x07\n"; // bytes: a, TAB, b, \\, c, BEL, LF
    cmd.args(["-n", "-e", "l"]).write_stdin(input);
    // Expect escaped tokens and trailing $ per line
    cmd.assert().success().stdout("a\\tb\\\\c\\a$\n");
}

#[test]
fn list_command_wraps_at_72_columns() {
    let mut cmd = bin();
    // Build a 100-char line of 'a'
    let long = "a".repeat(100) + "\n";
    cmd.args(["-n", "-e", "l"]).write_stdin(long);
    // First printed line should end with backslash due to wrap, second ends with $
    let out = cmd.assert().get_output().stdout.clone();
    let s = String::from_utf8_lossy(&out);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines.len() >= 2);
    assert!(lines[0].ends_with("\\"));
    assert!(lines.last().unwrap().ends_with("$"));
}

#[test]
fn append_and_insert_with_address() {
    let mut cmd = bin();
    // -n: control output; 2a appends after 2nd; 2i inserts before 2nd
    cmd.args([
        "-n", "-e", "2i\\
I", "-e", "2a\\
A", "-e", "p",
    ])
    .write_stdin("1\n2\n3\n");
    // Sequence with -n and final p prints current lines; i prints before current, a prints after current
    cmd.assert().success().stdout("1\nI\n2\nA\n3\n");
}

#[test]
fn delete_with_range_and_print_rest() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2,3d", "-e", "p"])
        .write_stdin("1\n2\n3\n4\n");
    cmd.assert().success().stdout("1\n4\n");
}

#[test]
fn change_single_line_address() {
    let mut cmd = bin();
    cmd.args([
        "-n", "-e", "2c\\
X", "-e", "p",
    ])
    .write_stdin("1\n2\n3\n");
    cmd.assert().success().stdout("1\nX\n3\n");
}

#[test]
fn change_range_end_print_once() {
    let mut cmd = bin();
    // Replace 2..4 with single X, printing it once
    cmd.args([
        "-n", "-e", "2,4c\\
X", "-e", "p",
    ])
    .write_stdin("1\n2\n3\n4\n5\n");
    cmd.assert().success().stdout("1\nX\n5\n");
}

#[test]
fn change_regex_singleton_range() {
    let mut cmd = bin();
    cmd.args([
        "-n",
        "-e",
        "/^x$/,/^x$/c\\
X",
        "-e",
        "p",
    ])
    .write_stdin("a\nx\ny\n");
    // GNU sed behavior: /^x$/,/^x$/ checks end from NEXT line, so range never ends.
    // The 'c' command outputs text only at the LAST line of range, but since range
    // continues to EOF, lines x and y are consumed by 'c' (which deletes pattern space
    // and starts new cycle, so 'p' doesn't run), and X is never output because range
    // doesn't end.
    cmd.assert().success().stdout("a\n");
}

#[test]
fn y_command_octal_escapes() {
    let mut cmd = bin();
    // GNU sed does NOT interpret octal escapes in y command
    // \141\142 stays literal, not mapped to 'a','b'
    cmd.args(["-e", "y/\\141\\142/\\102\\103/"])
        .write_stdin("ab\n");
    cmd.assert().success().stdout("ab\n");
}

#[test]
fn change_range_mixed_numeric_to_regex_end() {
    let mut cmd = bin();
    // replace from line 2 to line matching /^X$/ with single R
    cmd.args(["-n", "-e", "2,/^X$/c\\\nR", "-e", "p"])
        .write_stdin("1\n2\n3\nX\n5\n");
    cmd.assert().success().stdout("1\nR\n5\n");
}

#[test]
fn change_negated_range_applies_outside() {
    let mut cmd = bin();
    // Apply change to lines NOT in 2..4; print unmodified lines with p
    cmd.args(["-n", "-e", "2,4!c\\\nZ", "-e", "p"])
        .write_stdin("1\n2\n3\n4\n5\n");
    cmd.assert().success().stdout("Z\n2\n3\n4\nZ\n");
}

#[test]
fn ignore_case_with_address_range() {
    let mut cmd = bin();
    // Apply only between lines 2 and 4 inclusive
    cmd.args(["-n", "-e", "2,4s/foo/bar/Ig"])
        .write_stdin("Foo\nfOo\nfoo\nFOO\nfoo\n");
    // Expected prints only where substitution executed (we use -n and rely on 'p' flag via address? No, we explicitly print):
    // We'll add an addressed 'p' to show results
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2,4s/foo/bar/Ig;2,4p"]) // print addressed lines after substitution
        .write_stdin("Foo\nfOo\nfoo\nFOO\nfoo\n");
    cmd.assert().success().stdout("bar\nbar\nbar\n");
}

#[test]
fn next_command_prints_current_then_moves() {
    let mut cmd = bin();
    cmd.args(["-e", "n"]).write_stdin("a\nb\n");
    // default print prints first line, then n moves to next (which is printed by default in next cycle)
    cmd.assert().success().stdout("a\nb\n");
}

#[test]
fn big_n_appends_and_continues_commands() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "N;s/\n/;/;p"]).write_stdin("a\nb\n");
    cmd.assert().success().stdout("a;b\n");
}

#[test]
fn big_d_deletes_first_line_and_restarts_with_suffix() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "N;D;p"]).write_stdin("1\n2\n3\n");
    // After D restarts, 'p' is not reached; no output
    cmd.assert().success().stdout("");
}

#[test]
fn hold_and_get_copy() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "h;g;p"]).write_stdin("hello\n");
    cmd.assert().success().stdout("hello\n");
    verify_against_sed!("h;g;p", "hello\n", &["-n"]);
}

#[test]
fn hold_append_and_get_append() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "H;g;p"]).write_stdin("a\nb\n");
    // H always prepends newline, even when hold is empty (GNU sed behavior)
    // First cycle: hold='' -> H -> hold='\na', g -> pattern='\na', p -> print '\na'
    // Second cycle: hold='\na' -> H -> hold='\na\nb', g -> pattern='\na\nb', p -> print '\na\nb'
    cmd.assert().success().stdout("\na\n\na\nb\n");
    verify_against_sed!("H;g;p", "a\nb\n", &["-n"]);
}

#[test]
fn exchange_hold_and_pattern() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "h;x;p"]).write_stdin("x\n");
    cmd.assert().success().stdout("x\n");
    verify_against_sed!("h;x;p", "x\n", &["-n"]);
}

#[test]
fn test_branch_takes_on_successful_substitution() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/x/y/;t end;s/y/z/;:end;p"])
        .write_stdin("x\n");
    cmd.assert().success().stdout("y\n");
    verify_against_sed!("s/x/y/;t end;s/y/z/;:end;p", "x\n", &["-n"]);
}

#[test]
fn test_branch_t_skips_without_substitution() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "t end;s/x/y/;:end;p"])
        .write_stdin("x\n");
    cmd.assert().success().stdout("y\n");
    verify_against_sed!("t end;s/x/y/;:end;p", "x\n", &["-n"]);
}

#[test]
fn test_branch_t_resets_flag() {
    let mut cmd = bin();
    // After first t, the flag should reset so second t does nothing
    cmd.args(["-n", "-e", "s/a/b/;t L;:L;t L;p"])
        .write_stdin("a\n");
    cmd.assert().success().stdout("b\n");
}

#[test]
fn nested_group_with_negation_and_ranges() {
    let mut cmd = bin();
    // Reproduce the failing scenario with nested groups
    // 4,12 !{
    //   s/^/^/
    //   /6/,/10/ !{
    //       s/$/$/
    //       /8/ !s/_/T/
    //   }
    // }
    let script = r#"4,12 !{
s/^/^/
/6/,/10/ !{
    s/$/$/
    /8/ !s/_/T/
}
}"#;
    // Input lines labeled l1_..l12_ to make expectations obvious
    let input = (1..=12)
        .map(|i| format!("l{}_", i))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // Run with -n and append explicit p at end to print final pattern space per line
    cmd.args(["-n", "-e"])
        .arg(format!("{};p", script))
        .write_stdin(input.as_bytes());
    let out = cmd.assert().get_output().stdout.clone();
    let s = String::from_utf8_lossy(&out).to_string();

    // Spot-check key lines around the ranges:
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), 12);
    // For 4,12! group, commands apply outside 4..12 → lines 1..3 are transformed
    assert_eq!(lines[0], "^l1T$");
    assert_eq!(lines[1], "^l2T$");
    assert_eq!(lines[2], "^l3T$");
    // Lines 4..12 remain unchanged because group is negated for those lines
    assert_eq!(lines[3], "l4_");
    assert_eq!(lines[4], "l5_");
    assert_eq!(lines[5], "l6_");
    assert_eq!(lines[7], "l8_");
}

#[test]
fn test_branch_b_without_label_jumps_to_end() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "b;p"]).write_stdin("x\n");
    cmd.assert().success().stdout("");
}

#[test]
fn test_addressed_branch() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2b e;p;:e;="])
        .write_stdin("1\n2\n3\n");
    cmd.assert().success().stdout("1\n1\n2\n3\n3\n");
}

#[test]
fn quit_without_code_exits_0_and_no_output() {
    let mut cmd = bin();
    // Without -n, BSD sed prints current line before quitting
    cmd.args(["-e", "q"]).write_stdin("hello\n");
    cmd.assert().code(0).stdout("hello\n");
}

#[test]
fn quit_with_code_exits_with_that_code() {
    let mut cmd = bin();
    // Without -n, BSD sed prints current line before quitting with code
    cmd.args(["-e", "q 42"]).write_stdin("hello\n");
    cmd.assert().code(42).stdout("hello\n");
}

#[test]
fn quit_with_address_stops_before_printing() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2q", "-e", "p"])
        .write_stdin("1\n2\n3\n");
    cmd.assert().code(0).stdout("1\n");
}

#[test]
fn quit_with_negated_address_quits_immediately() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2!q", "-e", "p"])
        .write_stdin("1\n2\n");
    cmd.assert().code(0).stdout("");
}

// ============================================================================
// Phase 4.3: Comprehensive Substitution Flag Combination Tests
// ============================================================================

/// Test 'p' flag alone - should print substituted line
#[test]
fn subst_flag_p_alone() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/foo/bar/p"])
        .write_stdin("foo\nxxx\nfoo\n");
    cmd.assert().success().stdout("bar\nbar\n");
}

/// Test 'e' flag alone - should execute but not print (with -n)
#[test]
fn subst_flag_e_alone() {
    let mut cmd = bin();
    // Execute 'echo test' after substitution, but -n suppresses auto-print
    // So we get no output (execution happens but result is not printed)
    cmd.args(["-n", "-e", "s/.*/echo test/e"])
        .write_stdin("foo\n");
    cmd.assert().success().stdout("");
}

/// Test 'e' flag with auto-print - should execute and print result
#[test]
fn subst_flag_e_with_autoprint() {
    let mut cmd = bin();
    // Execute 'echo test' and auto-print the result
    cmd.args(["-e", "s/.*/echo test/e"]).write_stdin("foo\n");
    cmd.assert().success().stdout("test\n");
}

/// Test 'pe' flags - should print BEFORE executing (execution result not printed)
#[test]
fn subst_flag_pe_print_then_execute() {
    let mut cmd = bin();
    // Print "echo test" BEFORE executing it
    // The execution happens but its result is not printed
    cmd.args(["-n", "-e", "s/.*/echo test/pe"])
        .write_stdin("foo\n");
    cmd.assert().success().stdout("echo test\n");
}

/// Test 'ep' flags - should execute THEN print the result
#[test]
fn subst_flag_ep_execute_then_print() {
    let mut cmd = bin();
    // Execute "echo test" first (pattern space becomes "test")
    // Then print the pattern space ("test")
    cmd.args(["-n", "-e", "s/.*/echo test/ep"])
        .write_stdin("foo\n");
    cmd.assert().success().stdout("test\n");
}

/// Test 'pep' flags - multiple p should error (GNU sed compatibility)
#[test]
fn subst_flag_pep_error() {
    let mut cmd = bin();
    // GNU sed rejects multiple 'p' flags even with 'e' interspersed
    cmd.args(["-n", "-e", "s/.*/echo test/pep"])
        .write_stdin("foo\n");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("multiple 'p' options"));
}

/// Test 'epe' flags - multiple 'e' allowed, should execute then print
#[test]
fn subst_flag_epe_allowed() {
    let mut cmd = bin();
    // GNU sed allows multiple 'e' flags (unlike 'p')
    // With 'e' before 'p', should execute then print
    cmd.args(["-n", "-e", "s/.*/echo test/epe"])
        .write_stdin("foo\n");
    cmd.assert().success().stdout("test\n");
}

/// Test 'pp' flags - multiple p without intervening e should error
#[test]
fn subst_flag_pp_error() {
    let mut cmd = bin();
    cmd.args(["-e", "s/foo/bar/pp"]).write_stdin("foo\n");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("multiple 'p' options"));
}

/// Test 'g' flag alone - global substitution
#[test]
fn subst_flag_g_alone() {
    let mut cmd = bin();
    cmd.args(["-e", "s/o/X/g"]).write_stdin("foo\n");
    cmd.assert().success().stdout("fXX\n");
}

/// Test occurrence flag alone - replace only nth occurrence
#[test]
fn subst_flag_occurrence_alone() {
    let mut cmd = bin();
    cmd.args(["-e", "s/o/X/2"]).write_stdin("foo\n");
    cmd.assert().success().stdout("foX\n");
}

/// Test 'g2' flags - global + occurrence, occurrence takes precedence
#[test]
fn subst_flag_g_and_occurrence_allowed() {
    let mut cmd = bin();
    // GNU sed allows this - occurrence takes precedence, 'g' is ignored
    cmd.args(["-e", "s/o/X/g2"]).write_stdin("foo\n");
    cmd.assert().success().stdout("foX\n");
}

/// Test '2g' flags - occurrence + global, occurrence takes precedence
#[test]
fn subst_flag_occurrence_and_g_allowed() {
    let mut cmd = bin();
    // GNU sed allows this - occurrence takes precedence, 'g' is ignored
    cmd.args(["-e", "s/o/X/2g"]).write_stdin("foo\n");
    cmd.assert().success().stdout("foX\n");
}

/// Test 'gg' flags - multiple g should error
#[test]
fn subst_flag_gg_error() {
    let mut cmd = bin();
    cmd.args(["-e", "s/o/X/gg"]).write_stdin("foo\n");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("multiple 'g' options"));
}

/// Test 'ii' flags - multiple i (case-insensitive) allowed by GNU sed
#[test]
fn subst_flag_ii_allowed() {
    let mut cmd = bin();
    // GNU sed allows multiple 'i' flags - just treats as one
    cmd.args(["-e", "s/FOO/bar/ii"]).write_stdin("foo\n");
    cmd.assert().success().stdout("bar\n");
}

/// Test 'pg' flags - print and global together
#[test]
fn subst_flag_pg_combination() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/o/X/pg"]).write_stdin("foo\n");
    cmd.assert().success().stdout("fXX\n");
}

/// Test 'gp' flags - global and print together
#[test]
fn subst_flag_gp_combination() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/o/X/gp"]).write_stdin("foo\n");
    cmd.assert().success().stdout("fXX\n");
}

/// Test complex flag combination: 'gpi'
#[test]
fn subst_flag_gpi_combination() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/FOO/bar/gpi"])
        .write_stdin("FOO foo FOO\n");
    cmd.assert().success().stdout("bar bar bar\n");
}

// ============================================================================
// Phase 5: Coverage Tests - D (BigD) Command
// ============================================================================

/// D with multi-line pattern space - deletes first line
#[test]
fn big_d_multiline_deletes_first_line() {
    let mut cmd = bin();
    // N joins two lines, D deletes first, restarts cycle with remainder
    cmd.args(["-e", "N;D"])
        .write_stdin("first\nsecond\nthird\n");
    // After joining "first\nsecond", D deletes "first\n", leaving "second"
    // D restarts cycle, N joins "second\nthird", D deletes "second\n"
    // Then "third" is printed
    cmd.assert().success().stdout("third\n");
}

/// D with pattern space containing no newline - acts like d
#[test]
fn big_d_single_line_acts_like_d() {
    let mut cmd = bin();
    cmd.args(["-e", "D"]).write_stdin("only\n");
    // D on single line (no newline in pattern space) behaves like d - deletes and starts new cycle
    cmd.assert().success().stdout("");
}

/// D at end of file - D restarts cycle so p never executes
#[test]
fn big_d_at_eof() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "N;$D;p"]).write_stdin("a\nb\n");
    // N joins "a\nb", $ matches, D deletes "a\n" and restarts cycle
    // Since D restarts, 'p' never executes, output is empty (GNU sed behavior)
    cmd.assert().success().stdout("");
}

/// D in loop - processes all lines
#[test]
fn big_d_in_loop() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", ":a;N;$!ba;D"])
        .write_stdin("l1\nl2\nl3\n");
    // Collects all, then D deletes first line repeatedly until one remains
    cmd.assert().success().stdout("");
}

/// D after substitution
#[test]
fn big_d_after_substitution() {
    let mut cmd = bin();
    cmd.args(["-e", "N;s/:/-/g;D"]).write_stdin("a:b\nc:d\n");
    // N joins "a:b\nc:d", s changes to "a-b\nc-d", D removes "a-b\n", leaving "c-d"
    cmd.assert().success().stdout("c-d\n");
}

/// D with address
#[test]
fn big_d_with_address() {
    let mut cmd = bin();
    cmd.args(["-e", "2{N;D}"]).write_stdin("l1\nl2\nl3\nl4\n");
    // At line 2, N joins l2\nl3, D removes l2\n, leaving l3 which restarts cycle
    cmd.assert().success().stdout("l1\nl3\nl4\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Insert (i) Command
// ============================================================================

/// Insert at first line
#[test]
fn insert_at_first_line() {
    let mut cmd = bin();
    cmd.args(["-e", "1i\\\nINSERTED"])
        .write_stdin("first\nsecond\n");
    cmd.assert().success().stdout("INSERTED\nfirst\nsecond\n");
}

/// Insert at last line
#[test]
fn insert_at_last_line() {
    let mut cmd = bin();
    cmd.args(["-e", "$i\\\nINSERTED"])
        .write_stdin("first\nlast\n");
    cmd.assert().success().stdout("first\nINSERTED\nlast\n");
}

/// Insert with regex address
#[test]
fn insert_with_regex_address() {
    let mut cmd = bin();
    cmd.args(["-e", "/banana/i\\\nFRUIT"])
        .write_stdin("apple\nbanana\ncherry\n");
    cmd.assert()
        .success()
        .stdout("apple\nFRUIT\nbanana\ncherry\n");
}

/// Insert with negation
#[test]
fn insert_with_negation() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2!i\\\nNOT_2", "-e", "p"])
        .write_stdin("l1\nl2\nl3\n");
    cmd.assert().success().stdout("NOT_2\nl1\nl2\nNOT_2\nl3\n");
}

/// Insert multiple lines
#[test]
fn insert_multiple_lines() {
    let mut cmd = bin();
    cmd.args(["-e", "2i\\\nA\\\nB"]).write_stdin("1\n2\n3\n");
    cmd.assert().success().stdout("1\nA\nB\n2\n3\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Hold Space Operations
// ============================================================================

/// Multiple H commands accumulate with newlines
#[test]
fn hold_append_multiple_accumulates() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "H;$!d;g;p"]).write_stdin("a\nb\nc\n");
    // H appends with newline prefix each time
    cmd.assert().success().stdout("\na\nb\nc\n");
}

/// Exchange twice returns original
#[test]
fn exchange_twice_returns_original() {
    let mut cmd = bin();
    cmd.args(["-e", "x;x"]).write_stdin("content\n");
    cmd.assert().success().stdout("content\n");
}

/// Multiple G appends hold space multiple times
#[test]
fn get_append_multiple() {
    let mut cmd = bin();
    cmd.args(["-e", "h;G;G"]).write_stdin("line\n");
    // After h: hold="line", pattern="line"
    // After first G: pattern="line\nline"
    // After second G: pattern="line\nline\nline"
    cmd.assert().success().stdout("line\nline\nline\n");
}

/// Exchange with uninitialized (empty) hold space
#[test]
fn exchange_empty_hold() {
    let mut cmd = bin();
    cmd.args(["-e", "x"]).write_stdin("content\n");
    // Exchange puts empty hold into pattern, content into hold
    cmd.assert().success().stdout("\n");
}

/// Complex chain: h;s/.../;x;G
#[test]
fn hold_complex_chain() {
    let mut cmd = bin();
    cmd.args(["-e", "h;s/original/MODIFIED/;x;G"])
        .write_stdin("original\n");
    // h: hold="original", pattern="original"
    // s: pattern="MODIFIED"
    // x: pattern="original", hold="MODIFIED"
    // G: pattern="original\nMODIFIED"
    cmd.assert().success().stdout("original\nMODIFIED\n");
}

/// h followed by g on different lines
#[test]
fn hold_across_lines() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "1h;2g;p"])
        .write_stdin("first\nsecond\n");
    // Line 1: h saves "first"
    // Line 2: g replaces with "first"
    cmd.assert().success().stdout("first\nfirst\n");
}

/// H;H;g chain (double append)
#[test]
fn hold_append_double() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "1{H;H};$g;$p"])
        .write_stdin("one\ntwo\n");
    // Line 1: H twice appends "one" twice with newlines
    // Line 2: g replaces with hold content
    cmd.assert().success().stdout("\none\none\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Read (r/R) Command Edge Cases
// ============================================================================

/// Read with 0 address (prepend before first line)
#[test]
fn read_zero_address_prepend() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    fs::write(tmp.path(), "PREPENDED\n").unwrap();

    let mut cmd = bin();
    cmd.args(["-e"])
        .arg(format!("0r {}", tmp.path().display()))
        .write_stdin("first\nsecond\n");
    cmd.assert().success().stdout("PREPENDED\nfirst\nsecond\n");
}

/// R (ReadLine) command reads one line at a time
#[test]
fn read_line_command() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    fs::write(tmp.path(), "X\nY\nZ\n").unwrap();

    let mut cmd = bin();
    cmd.args(["-e"])
        .arg(format!("R {}", tmp.path().display()))
        .write_stdin("a\nb\nc\n");
    // R reads one line from file per input line
    cmd.assert().success().stdout("a\nX\nb\nY\nc\nZ\n");
}

/// R at EOF of file is silently ignored
#[test]
fn read_line_eof_ignored() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    fs::write(tmp.path(), "X\n").unwrap();

    let mut cmd = bin();
    cmd.args(["-e"])
        .arg(format!("R {}", tmp.path().display()))
        .write_stdin("a\nb\nc\n");
    // Only one line in file, subsequent R's are ignored
    cmd.assert().success().stdout("a\nX\nb\nc\n");
}

/// Read missing file is silently ignored
#[test]
fn read_missing_file_ignored() {
    let mut cmd = bin();
    cmd.args(["-e", "r /nonexistent/file/path.txt"])
        .write_stdin("hello\n");
    cmd.assert().success().stdout("hello\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Quit Commands with Exit Codes
// ============================================================================

/// Quit with specific exit code
#[test]
fn quit_with_exit_code_42() {
    let mut cmd = bin();
    cmd.args(["-e", "q42"]).write_stdin("test\n");
    cmd.assert().code(42).stdout("test\n");
}

/// Quit with exit code 0
#[test]
fn quit_with_exit_code_0() {
    let mut cmd = bin();
    cmd.args(["-e", "q0"]).write_stdin("test\n");
    cmd.assert().code(0).stdout("test\n");
}

/// QuitSilent (Q) with exit code
#[test]
fn quit_silent_with_exit_code_1() {
    let mut cmd = bin();
    cmd.args(["-e", "Q1"]).write_stdin("test\n");
    cmd.assert().code(1).stdout("");
}

/// Q at specific line
#[test]
fn quit_silent_at_line() {
    let mut cmd = bin();
    cmd.args(["-e", "2Q"]).write_stdin("l1\nl2\nl3\n");
    cmd.assert().code(0).stdout("l1\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Test Negative (T) Command
// ============================================================================

/// T branches when NO substitution was made
#[test]
fn test_neg_branches_without_substitution() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "T end;s/./X/;:end;p"])
        .write_stdin("a\n");
    // T branches immediately (no subst yet), skipping s command
    cmd.assert().success().stdout("a\n");
}

/// T does not branch after successful substitution
#[test]
fn test_neg_no_branch_after_subst() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/a/A/;T end;s/A/B/;:end;p"])
        .write_stdin("a\n");
    // First s succeeds, T does not branch, second s executes
    cmd.assert().success().stdout("B\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Append (a) Command Edge Cases
// ============================================================================

/// Append at last line
#[test]
fn append_at_last_line() {
    let mut cmd = bin();
    cmd.args(["-e", "$a\\\nAPPENDED"])
        .write_stdin("first\nlast\n");
    cmd.assert().success().stdout("first\nlast\nAPPENDED\n");
}

/// Append with regex address
#[test]
fn append_with_regex() {
    let mut cmd = bin();
    cmd.args(["-e", "/middle/a\\\nAFTER"])
        .write_stdin("first\nmiddle\nlast\n");
    cmd.assert()
        .success()
        .stdout("first\nmiddle\nAFTER\nlast\n");
}

/// Append empty text
#[test]
fn append_empty_text() {
    let mut cmd = bin();
    cmd.args(["-e", "1a\\\n"]).write_stdin("line\n");
    cmd.assert().success().stdout("line\n\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Print First Line (P) Command
// ============================================================================

/// P prints only first line of multiline pattern space
#[test]
fn print_first_line_multiline() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "N;P"]).write_stdin("first\nsecond\n");
    cmd.assert().success().stdout("first\n");
}

/// P on single line pattern space
#[test]
fn print_first_line_single() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "P"]).write_stdin("only\n");
    cmd.assert().success().stdout("only\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Write First Line (W) Command
// ============================================================================

/// W writes only first line of pattern space to file
#[test]
fn write_first_line_multiline() {
    use std::fs;
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    let mut cmd = bin();
    cmd.args(["-n", "-e"])
        .arg(format!("N;W {}", path.display()))
        .write_stdin("first\nsecond\n");
    cmd.assert().success();

    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "first\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Clear (z) Command
// ============================================================================

/// z command clears pattern space
#[test]
fn clear_command() {
    let mut cmd = bin();
    cmd.args(["-e", "z"]).write_stdin("content\n");
    cmd.assert().success().stdout("\n");
}

/// z then substitution
#[test]
fn clear_then_subst() {
    let mut cmd = bin();
    cmd.args(["-e", "z;s/^$/EMPTY/"]).write_stdin("content\n");
    cmd.assert().success().stdout("EMPTY\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Line Number (=) Command
// ============================================================================

/// = prints line number
#[test]
fn line_number_command() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "="]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("1\n2\n3\n");
}

/// = with address
#[test]
fn line_number_with_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2="]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("2\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Label and Branch
// ============================================================================

/// Label with no reference is allowed
#[test]
fn label_unreferenced() {
    let mut cmd = bin();
    cmd.args(["-e", ":unused;p"]).write_stdin("test\n");
    cmd.assert().success().stdout("test\ntest\n");
}

/// Multiple labels
#[test]
fn multiple_labels() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", ":a;p;b b;:b;p"]).write_stdin("x\n");
    // Prints twice: once at :a, then branches to :b which prints again
    cmd.assert().success().stdout("x\nx\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Filename Command (F)
// ============================================================================

/// F prints filename
#[test]
fn filename_command_with_file() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "content").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    cmd.args(["-n", "-e", "F"]).arg(tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    let s = String::from_utf8_lossy(&output);
    assert!(s.contains(tmp.path().to_str().unwrap()));
}

/// F with stdin prints -
#[test]
fn filename_command_stdin() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "F"]).write_stdin("test\n");
    cmd.assert().success().stdout("-\n");
}

// ============================================================================
// Phase 5: Coverage Tests - Substitution Edge Cases
// ============================================================================

/// Empty pattern uses last regex
#[test]
fn subst_empty_pattern_reuse() {
    let mut cmd = bin();
    cmd.args(["-e", "/foo/s//bar/"]).write_stdin("foo bar\n");
    cmd.assert().success().stdout("bar bar\n");
}

/// Case conversion in replacement: \u \l \U \L \E
#[test]
fn subst_case_conversion_upper() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\(.*\\)/\\U\\1/"])
        .write_stdin("hello\n");
    cmd.assert().success().stdout("HELLO\n");
}

#[test]
fn subst_case_conversion_lower() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\(.*\\)/\\L\\1/"])
        .write_stdin("HELLO\n");
    cmd.assert().success().stdout("hello\n");
}

#[test]
fn subst_case_conversion_first_char() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\(.*\\)/\\u\\1/"])
        .write_stdin("hello\n");
    cmd.assert().success().stdout("Hello\n");
}

#[test]
fn subst_case_conversion_end() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\(..\\)\\(.*\\)/\\U\\1\\E\\2/"])
        .write_stdin("hello\n");
    cmd.assert().success().stdout("HEllo\n");
}

// ============================================================================
// Phase 6: Additional Coverage Tests
// ============================================================================

/// Test list command with various escape sequences
#[test]
fn list_command_escapes() {
    let mut cmd = bin();
    // Input with tab, bell, backspace, form feed
    cmd.args(["-n", "-e", "l"])
        .write_stdin("a\tb\x07c\x08d\x0c\n");
    cmd.assert().success().stdout("a\\tb\\ac\\bd\\f$\n");
}

/// Test list command with high bytes
#[test]
fn list_command_high_bytes() {
    let mut cmd = bin();
    // Use bytes directly for high byte values
    cmd.args(["-n", "-e", "l"])
        .write_stdin(&[0x80u8, 0xff, b'\n'][..]);
    // High bytes should be output as octal escapes
    let output = cmd.assert().success().get_output().stdout.clone();
    let s = String::from_utf8_lossy(&output);
    assert!(s.contains("\\"));
}

/// Test version command
#[test]
fn version_command_passes() {
    let mut cmd = bin();
    cmd.args(["-e", "v 4.0"]).write_stdin("test\n");
    cmd.assert().success().stdout("test\n");
}

/// Test version command fails for higher version
#[test]
fn version_command_fails_higher() {
    let mut cmd = bin();
    cmd.args(["-e", "v 99.0"]).write_stdin("test\n");
    cmd.assert().failure();
}

/// Test hex escapes in replacement
#[test]
fn subst_hex_escape() {
    let mut cmd = bin();
    cmd.args(["-e", "s/b/\\x58/g"]).write_stdin("abc\n");
    cmd.assert().success().stdout("aXc\n");
}

/// Test octal escapes in replacement (GNU sed uses \oNNN syntax)
#[test]
fn subst_octal_escape() {
    let mut cmd = bin();
    // GNU sed uses \o130 for octal 130 = 'X' (ASCII 88)
    cmd.args(["-e", "s/b/\\o130/g"]).write_stdin("abc\n");
    cmd.assert().success().stdout("aXc\n");
}

/// Test newline escape in replacement
#[test]
fn subst_newline_escape() {
    let mut cmd = bin();
    cmd.args(["-e", "s/ /\\n/g"]).write_stdin("a b c\n");
    cmd.assert().success().stdout("a\nb\nc\n");
}

/// Test tab escape in replacement
#[test]
fn subst_tab_escape() {
    let mut cmd = bin();
    cmd.args(["-e", "s/ /\\t/g"]).write_stdin("a b\n");
    cmd.assert().success().stdout("a\tb\n");
}

/// Test n command (next) with quiet mode
#[test]
fn next_command_quiet() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "n;p"]).write_stdin("a\nb\nc\n");
    // n reads next line without printing, then p prints it
    cmd.assert().success().stdout("b\n");
}

/// Test N command (append next line)
#[test]
fn big_n_command() {
    let mut cmd = bin();
    cmd.args(["-e", "N;s/\\n/-/"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("a-b\nc\n");
}

/// Test N at end of file
#[test]
fn big_n_at_eof() {
    let mut cmd = bin();
    cmd.args(["-e", "$N"]).write_stdin("only\n");
    // N at EOF with one line should just print the line
    cmd.assert().success().stdout("only\n");
}

/// Test substitute with backreference \0 (whole match)
#[test]
fn subst_backref_zero() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[0-9][0-9]*/[\\0]/g"])
        .write_stdin("a1b23c\n");
    cmd.assert().success().stdout("a[1]b[23]c\n");
}

/// Test regex with word boundaries
#[test]
fn regex_word_boundary() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\bword\\b/WORD/g"])
        .write_stdin("word words sword\n");
    cmd.assert().success().stdout("WORD words sword\n");
}

/// Test regex with start word boundary
#[test]
fn regex_start_word_boundary() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\<word/WORD/g"])
        .write_stdin("word words sword\n");
    cmd.assert().success().stdout("WORD WORDs sword\n");
}

/// Test regex with end word boundary
#[test]
fn regex_end_word_boundary() {
    let mut cmd = bin();
    cmd.args(["-e", "s/word\\>/WORD/g"])
        .write_stdin("word words sword\n");
    cmd.assert().success().stdout("WORD words sWORD\n");
}

/// Test regex alternation in ERE mode
#[test]
fn regex_ere_alternation() {
    let mut cmd = bin();
    cmd.args(["-E", "-e", "s/cat|dog/animal/g"])
        .write_stdin("cat and dog\n");
    cmd.assert().success().stdout("animal and animal\n");
}

/// Test regex plus quantifier in ERE mode
#[test]
fn regex_ere_plus() {
    let mut cmd = bin();
    cmd.args(["-E", "-e", "s/a+/X/g"]).write_stdin("baaaaab\n");
    cmd.assert().success().stdout("bXb\n");
}

/// Test regex question quantifier in ERE mode
#[test]
fn regex_ere_question() {
    let mut cmd = bin();
    cmd.args(["-E", "-e", "s/colou?r/COLOR/g"])
        .write_stdin("color colour\n");
    cmd.assert().success().stdout("COLOR COLOR\n");
}

/// Test regex counted repetition
#[test]
fn regex_counted_repetition() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a\\{3\\}/X/g"])
        .write_stdin("aa aaa aaaa\n");
    cmd.assert().success().stdout("aa X Xa\n");
}

/// Test regex min-max repetition
#[test]
fn regex_minmax_repetition() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a\\{2,4\\}/X/g"])
        .write_stdin("a aa aaa aaaa aaaaa\n");
    cmd.assert().success().stdout("a X X X Xa\n");
}

/// Test address with first~step
#[test]
fn address_step() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "1~2p"])
        .write_stdin("1\n2\n3\n4\n5\n");
    cmd.assert().success().stdout("1\n3\n5\n");
}

/// Test address with 0,/pattern/
#[test]
fn address_zero_to_pattern() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "0,/x/p"]).write_stdin("a\nx\nb\n");
    cmd.assert().success().stdout("a\nx\n");
}

/// Test comment in script
#[test]
fn script_comment() {
    let mut cmd = bin();
    cmd.args(["-e", "# this is a comment", "-e", "s/a/b/"])
        .write_stdin("a\n");
    cmd.assert().success().stdout("b\n");
}

/// Test semicolon as command separator
#[test]
fn command_separator_semicolon() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/b/;s/b/c/"]).write_stdin("a\n");
    cmd.assert().success().stdout("c\n");
}

/// Test grouped commands with braces
#[test]
fn grouped_commands() {
    let mut cmd = bin();
    cmd.args(["-e", "2{s/a/A/;s/b/B/}"])
        .write_stdin("ab\nab\nab\n");
    cmd.assert().success().stdout("ab\nAB\nab\n");
}

/// Test nested grouped commands
#[test]
fn nested_grouped_commands() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2{/a/{s/a/X/;p}}"])
        .write_stdin("ab\nab\nab\n");
    cmd.assert().success().stdout("Xb\n");
}

/// Test multiple expressions
#[test]
fn multiple_expressions() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/1/", "-e", "s/b/2/", "-e", "s/c/3/"])
        .write_stdin("abc\n");
    cmd.assert().success().stdout("123\n");
}

/// Test POSIX character classes
#[test]
fn posix_char_class_digit() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[[:digit:]]/X/g"])
        .write_stdin("a1b2c3\n");
    cmd.assert().success().stdout("aXbXcX\n");
}

#[test]
fn posix_char_class_space() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[[:space:]]/_/g"])
        .write_stdin("a b\tc\n");
    // Note: newline at end is line terminator, not matched
    cmd.assert().success().stdout("a_b_c\n");
}

#[test]
fn posix_char_class_upper() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[[:upper:]]/x/g"]).write_stdin("AbCdE\n");
    cmd.assert().success().stdout("xbxdx\n");
}

#[test]
fn posix_char_class_lower() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[[:lower:]]/X/g"]).write_stdin("AbCdE\n");
    cmd.assert().success().stdout("AXCXE\n");
}

/// Test negated character class
#[test]
fn negated_char_class() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[^a-z]/_/g"]).write_stdin("aB1c\n");
    // B and 1 are not lowercase letters, so they get replaced
    cmd.assert().success().stdout("a__c\n");
}

/// Test dot matches any character
#[test]
fn regex_dot_any() {
    let mut cmd = bin();
    cmd.args(["-e", "s/./X/g"]).write_stdin("abc\n");
    cmd.assert().success().stdout("XXX\n");
}

/// Test caret anchor
#[test]
fn regex_caret_anchor() {
    let mut cmd = bin();
    cmd.args(["-e", "s/^a/X/"]).write_stdin("abc\nabc\n");
    cmd.assert().success().stdout("Xbc\nXbc\n");
}

/// Test dollar anchor
#[test]
fn regex_dollar_anchor() {
    let mut cmd = bin();
    cmd.args(["-e", "s/c$/X/"]).write_stdin("abc\nabc\n");
    cmd.assert().success().stdout("abX\nabX\n");
}

/// Test both anchors
#[test]
fn regex_both_anchors() {
    let mut cmd = bin();
    cmd.args(["-e", "s/^abc$/X/"])
        .write_stdin("abc\nxabc\nabcx\n");
    cmd.assert().success().stdout("X\nxabc\nabcx\n");
}

/// Test star quantifier
#[test]
fn regex_star() {
    let mut cmd = bin();
    cmd.args(["-e", "s/ab*/X/g"]).write_stdin("a ab abb abbb\n");
    cmd.assert().success().stdout("X X X X\n");
}

/// Test empty match handling
#[test]
fn regex_empty_match() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a*/X/g"]).write_stdin("bab\n");
    // Empty matches at boundaries should be handled
    cmd.assert().success();
}

/// Test character range in class
#[test]
fn char_class_range() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[a-z]/X/g"]).write_stdin("aBcDeF\n");
    cmd.assert().success().stdout("XBXDXF\n");
}

/// Test special characters in character class
#[test]
fn char_class_special() {
    let mut cmd = bin();
    // Closing bracket at start, hyphen at end
    cmd.args(["-e", "s/[]a-]/X/g"]).write_stdin("a]b-c\n");
    cmd.assert().success().stdout("XXbXc\n");
}

// ============================================================================
// Phase 6: Additional Coverage Tests
// ============================================================================

/// Test substitution with execute flag (e)
#[test]
fn subst_execute_flag() {
    let mut cmd = bin();
    cmd.args(["-e", "s/.*/echo HELLO/e"])
        .write_stdin("ignore\n");
    cmd.assert().success().stdout("HELLO\n");
}

/// Test negated address with regex
#[test]
fn negated_regex_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "/foo/!p"])
        .write_stdin("foo\nbar\nbaz\n");
    cmd.assert().success().stdout("bar\nbaz\n");
    verify_against_sed!("/foo/!p", "foo\nbar\nbaz\n", &["-n"]);
}

/// Test negated address with line number
#[test]
fn negated_line_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2!p"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("a\nc\n");
    verify_against_sed!("2!p", "a\nb\nc\n", &["-n"]);
}

/// Test negated address range
#[test]
fn negated_range_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "2,3!p"]).write_stdin("a\nb\nc\nd\n");
    cmd.assert().success().stdout("a\nd\n");
    verify_against_sed!("2,3!p", "a\nb\nc\nd\n", &["-n"]);
}

/// Test branch to end of script
#[test]
fn branch_to_end() {
    let mut cmd = bin();
    cmd.args(["-e", "b end;s/a/X/;:end"]).write_stdin("abc\n");
    cmd.assert().success().stdout("abc\n");
    verify_against_sed!("b end;s/a/X/;:end", "abc\n", &[]);
}

/// Test change command with range
#[test]
fn change_with_range() {
    let mut cmd = bin();
    cmd.args(["-e", "2,3c\\\nNEW"]).write_stdin("a\nb\nc\nd\n");
    cmd.assert().success().stdout("a\nNEW\nd\n");
}

/// Test substitution multiline mode (m flag)
#[test]
fn subst_multiline_flag() {
    let mut cmd = bin();
    cmd.args(["-e", "N;s/^/START:/gm"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("START:a\nSTART:b\nc\n");
}

/// Test substitution with occurrence number
#[test]
fn subst_occurrence_4() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/4"]).write_stdin("aaaaa\n");
    cmd.assert().success().stdout("aaaXa\n");
    verify_against_sed!("s/a/X/4", "aaaaa\n", &[]);
}

/// Test Q (quit without printing)
#[test]
fn quit_silent() {
    let mut cmd = bin();
    cmd.args(["-e", "2Q"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("a\n");
}

/// Test quit with exit code
#[test]
fn quit_with_exit_code() {
    let mut cmd = bin();
    cmd.args(["-e", "q5"]).write_stdin("hello\n");
    cmd.assert().code(5);
}

/// Test address with tilde step (0~2)
#[test]
fn address_zero_tilde_step() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "0~3p"])
        .write_stdin("1\n2\n3\n4\n5\n6\n");
    cmd.assert().success().stdout("3\n6\n");
    verify_against_sed!("0~3p", "1\n2\n3\n4\n5\n6\n", &["-n"]);
}

/// Test F command (print filename)
#[test]
fn print_filename() {
    use std::fs;
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    fs::write(tmp.path(), "hello\n").unwrap();

    let mut cmd = bin();
    cmd.args(["-n", "-e", "F"]).arg(tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    assert!(!output.is_empty());
}

/// Test P with single line pattern space - explicit check
#[test]
fn print_first_line_single_explicit() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "P"]).write_stdin("single line\n");
    cmd.assert().success().stdout("single line\n");
    verify_against_sed!("P", "single line\n", &["-n"]);
}

/// Test multiline pattern with P
#[test]
fn print_first_line_multiline_pattern() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "N;N;P"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("a\n");
    verify_against_sed!("N;N;P", "a\nb\nc\n", &["-n"]);
}

/// Test empty pattern reuse in substitution
#[test]
fn empty_pattern_reuse() {
    let mut cmd = bin();
    cmd.args(["-e", "/hello/s//world/"]).write_stdin("hello\n");
    cmd.assert().success().stdout("world\n");
    verify_against_sed!("/hello/s//world/", "hello\n", &[]);
}

/// Test word boundary in regex (\b)
#[test]
fn word_boundary() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\bfoo\\b/BAR/g"])
        .write_stdin("foo foobar barfoo\n");
    cmd.assert().success().stdout("BAR foobar barfoo\n");
}

/// Test backreference with more than 9 groups
#[test]
fn backreference_nine_groups() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)/\\9\\8\\7\\6\\5\\4\\3\\2\\1/"])
        .write_stdin("123456789\n");
    cmd.assert().success().stdout("987654321\n");
}

/// Test substitution with 2g flag
#[test]
fn subst_2g_flag() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/2g"]).write_stdin("aaaaa\n");
    cmd.assert().success().stdout("aXXXX\n");
    verify_against_sed!("s/a/X/2g", "aaaaa\n", &[]);
}

/// Test comment at end of script
#[test]
fn comment_at_end() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/g #comment"]).write_stdin("aaa\n");
    cmd.assert().success().stdout("XXX\n");
}

/// Test nested braces
#[test]
fn nested_braces() {
    let mut cmd = bin();
    cmd.args(["-e", "1{2,3{s/a/X/}}"])
        .write_stdin("abc\nabc\nabc\n");
    cmd.assert().success().stdout("abc\nabc\nabc\n");
}

/// Test case conversion \l
#[test]
fn case_conversion_lowercase_next() {
    let mut cmd = bin();
    cmd.args(["-e", "s/.*/\\l&/"]).write_stdin("HELLO\n");
    cmd.assert().success().stdout("hELLO\n");
}

/// Test multiple -e expressions
#[test]
fn multiple_e_expressions() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/1/", "-e", "s/b/2/", "-e", "s/c/3/"])
        .write_stdin("abc\n");
    cmd.assert().success().stdout("123\n");
    verify_against_sed!("s/a/1/;s/b/2/;s/c/3/", "abc\n", &[]);
}

/// Test hex escape in replacement
#[test]
fn hex_escape_in_replacement() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/\\x41/g"]).write_stdin("aaa\n");
    cmd.assert().success().stdout("AAA\n");
}

/// Test octal escape in replacement
#[test]
fn octal_escape_in_replacement() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/\\o101/g"]).write_stdin("aaa\n");
    cmd.assert().success().stdout("AAA\n");
}

/// Test decimal escape in replacement
#[test]
fn decimal_escape_in_replacement() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/\\d65/g"]).write_stdin("aaa\n");
    cmd.assert().success().stdout("AAA\n");
}

/// Test control character in replacement
#[test]
fn control_char_in_replacement() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/\\cI/g"]).write_stdin("aaa\n");
    // \cI is tab (control-I = 0x09)
    cmd.assert().success().stdout("\t\t\t\n");
}

/// Test print and execute flags together (pe order)
#[test]
fn subst_pe_flags() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/.*/echo DONE/pe"])
        .write_stdin("x\n");
    // pe: print then execute - should print the substituted text "echo DONE" first
    // then execute it and print result "DONE"
    cmd.assert().success();
}

/// Test print and execute flags together (ep order)
#[test]
fn subst_ep_flags() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/.*/echo DONE/ep"])
        .write_stdin("x\n");
    // ep: execute then print - should execute first, then print
    cmd.assert().success();
}

/// Test substitution with w flag (write to file)
#[test]
fn subst_write_flag() {
    use std::fs;
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.args(["-e", &format!("s/foo/bar/w {}", tmp_path)])
        .write_stdin("foo\nbaz\n");
    cmd.assert().success().stdout("bar\nbaz\n");

    let written = fs::read_to_string(tmp.path()).unwrap();
    assert_eq!(written, "bar\n");
}

/// Test n command (next line)
#[test]
fn next_line_command() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "n;p"]).write_stdin("a\nb\nc\nd\n");
    cmd.assert().success().stdout("b\nd\n");
    verify_against_sed!("n;p", "a\nb\nc\nd\n", &["-n"]);
}

/// Test step address with substitution
#[test]
fn step_address_subst() {
    let mut cmd = bin();
    cmd.args(["-e", "1~2s/a/X/"]).write_stdin("a\na\na\na\na\n");
    cmd.assert().success().stdout("X\na\nX\na\nX\n");
    verify_against_sed!("1~2s/a/X/", "a\na\na\na\na\n", &[]);
}

// =============================================
// Additional tests for exec.rs coverage improvement
// =============================================

/// Test case transform \u (uppercase next) with group backreference
#[test]
fn case_uppercase_next_with_backref() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\([a-z]\\+\\)/\\u\\1/"])
        .write_stdin("hello world\n");
    cmd.assert().success().stdout("Hello world\n");
    verify_against_sed!("s/\\([a-z]\\+\\)/\\u\\1/", "hello world\n", &[]);
}

/// Test case transform \l (lowercase next) with group backreference
#[test]
fn case_lowercase_next_with_backref() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\([A-Z]\\+\\)/\\l\\1/"])
        .write_stdin("HELLO WORLD\n");
    cmd.assert().success().stdout("hELLO WORLD\n");
    verify_against_sed!("s/\\([A-Z]\\+\\)/\\l\\1/", "HELLO WORLD\n", &[]);
}

/// Test case transform \U (uppercase all) with group backreference
#[test]
fn case_uppercase_all_with_backref() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\([a-z]\\+\\)/\\U\\1\\E/"])
        .write_stdin("hello\n");
    cmd.assert().success().stdout("HELLO\n");
    verify_against_sed!("s/\\([a-z]\\+\\)/\\U\\1\\E/", "hello\n", &[]);
}

/// Test case transform \L (lowercase all) with group backreference
#[test]
fn case_lowercase_all_with_backref() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\([A-Z]\\+\\)/\\L\\1\\E/"])
        .write_stdin("HELLO\n");
    cmd.assert().success().stdout("hello\n");
    verify_against_sed!("s/\\([A-Z]\\+\\)/\\L\\1\\E/", "HELLO\n", &[]);
}

/// Test substitution with occurrence 3
#[test]
fn subst_occurrence_3() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/3"]).write_stdin("a a a a a\n");
    cmd.assert().success().stdout("a a X a a\n");
    verify_against_sed!("s/a/X/3", "a a a a a\n", &[]);
}

/// Test substitution with occurrence 5
#[test]
fn subst_occurrence_5() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/5"]).write_stdin("a a a a a\n");
    cmd.assert().success().stdout("a a a a X\n");
    verify_against_sed!("s/a/X/5", "a a a a a\n", &[]);
}

/// Test substitution with occurrence beyond total matches
#[test]
fn subst_occurrence_beyond_matches() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/9"]).write_stdin("a a a\n");
    cmd.assert().success().stdout("a a a\n");
    verify_against_sed!("s/a/X/9", "a a a\n", &[]);
}

/// Test substitution with 3g (from 3rd occurrence, replace all)
#[test]
fn subst_occurrence_3g() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/3g"]).write_stdin("a a a a a\n");
    cmd.assert().success().stdout("a a X X X\n");
    verify_against_sed!("s/a/X/3g", "a a a a a\n", &[]);
}

/// Test zero-length match with star quantifier
#[test]
fn zero_length_match_star() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a*/X/g"]).write_stdin("bab\n");
    cmd.assert().success().stdout("XbXbX\n");
    verify_against_sed!("s/a*/X/g", "bab\n", &[]);
}

/// Test zero-length match at word boundaries
#[test]
fn zero_length_word_boundary() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\b/|/g"]).write_stdin("ab cd\n");
    cmd.assert().success().stdout("|ab| |cd|\n");
    verify_against_sed!("s/\\b/|/g", "ab cd\n", &[]);
}

/// Test empty pattern reuse after substitution
#[test]
fn empty_pattern_reuse_after_subst() {
    let mut cmd = bin();
    // After s/foo//, empty pattern // reuses 'foo', but 'foo' is gone
    cmd.args(["-e", "s/foo//;s//X/2"])
        .write_stdin("foo bar baz bar qux bar\n");
    cmd.assert().success().stdout(" bar baz bar qux bar\n");
    // Note: no verify_against_sed since behavior depends on last regex tracking
}

/// Test translate (y) with mixed case
#[test]
fn translate_mixed_case() {
    let mut cmd = bin();
    cmd.args(["-e", "y/abc/XYZ/"]).write_stdin("abcABC\n");
    cmd.assert().success().stdout("XYZABC\n");
    verify_against_sed!("y/abc/XYZ/", "abcABC\n", &[]);
}

/// Test translate with numbers
#[test]
fn translate_numbers() {
    let mut cmd = bin();
    cmd.args(["-e", "y/0123456789/abcdefghij/"])
        .write_stdin("2024\n");
    cmd.assert().success().stdout("cace\n");
    verify_against_sed!("y/0123456789/abcdefghij/", "2024\n", &[]);
}

/// Test translate with special characters
#[test]
fn translate_special_chars() {
    let mut cmd = bin();
    cmd.args(["-e", "y/./!/"]).write_stdin("a.b.c\n");
    cmd.assert().success().stdout("a!b!c\n");
    verify_against_sed!("y/./!/", "a.b.c\n", &[]);
}

/// Test hold space with raw bytes preservation
#[test]
fn hold_space_preserves_content() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "1h;2{H;g;p}"])
        .write_stdin("line1\nline2\n");
    cmd.assert().success().stdout("line1\nline2\n");
    verify_against_sed!("1h;2{H;g;p}", "line1\nline2\n", &["-n"]);
}

/// Test exchange (x) multiple times
#[test]
fn exchange_multiple_times() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "h;s/.*/NEW/;x;p;x;p"])
        .write_stdin("OLD\n");
    cmd.assert().success().stdout("OLD\nNEW\n");
    verify_against_sed!("h;s/.*/NEW/;x;p;x;p", "OLD\n", &["-n"]);
}

/// Test get append (G) with empty hold space
#[test]
fn get_append_empty_hold() {
    let mut cmd = bin();
    cmd.args(["-e", "G"]).write_stdin("line\n");
    cmd.assert().success().stdout("line\n\n");
    verify_against_sed!("G", "line\n", &[]);
}

/// Test BigD deletes first line and continues
#[test]
fn big_d_continues_to_end() {
    let mut cmd = bin();
    // N;D on "a\nb\nc\n" should output just "c"
    cmd.args(["-e", "N;D"]).write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("c\n");
    verify_against_sed!("N;D", "a\nb\nc\n", &[]);
}

/// Test branch with empty label jumps to end
#[test]
fn branch_empty_label_to_end() {
    let mut cmd = bin();
    cmd.args(["-e", "b;s/./X/"]).write_stdin("abc\n");
    cmd.assert().success().stdout("abc\n");
    verify_against_sed!("b;s/./X/", "abc\n", &[]);
}

/// Test test branch (t) resets flag after branch
#[test]
fn test_branch_resets_after_jump() {
    let mut cmd = bin();
    cmd.args(["-e", ":loop;s/a/X/;t loop"]).write_stdin("aaa\n");
    cmd.assert().success().stdout("XXX\n");
    verify_against_sed!(":loop;s/a/X/;t loop", "aaa\n", &[]);
}

/// Test T (inverse test) branch
#[test]
fn test_neg_branch_basic() {
    let mut cmd = bin();
    cmd.args(["-e", "s/x/y/;T end;s/./Z/g;:end"])
        .write_stdin("abc\n");
    cmd.assert().success().stdout("abc\n");
    verify_against_sed!("s/x/y/;T end;s/./Z/g;:end", "abc\n", &[]);
}

/// Test T branch with successful substitution
#[test]
fn test_neg_no_branch_on_success() {
    let mut cmd = bin();
    cmd.args(["-e", "s/a/X/;T end;s/b/Y/;:end"])
        .write_stdin("ab\n");
    cmd.assert().success().stdout("XY\n");
    verify_against_sed!("s/a/X/;T end;s/b/Y/;:end", "ab\n", &[]);
}

/// Test ReadLine (R) command progressive read
#[test]
fn read_line_progressive() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "inserted1").unwrap();
    writeln!(tmp, "inserted2").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    cmd.args(["-e", &format!("R {}", tmp.path().display())])
        .write_stdin("line1\nline2\n");
    cmd.assert()
        .success()
        .stdout("line1\ninserted1\nline2\ninserted2\n");
}

/// Test Write first line (W) command
#[test]
fn write_first_line_only() {
    use std::fs;
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.args(["-n", "-e", &format!("N;W {}", tmp_path)])
        .write_stdin("line1\nline2\n");
    cmd.assert().success();

    let written = fs::read_to_string(tmp.path()).unwrap();
    assert_eq!(written, "line1\n");
}

/// Test list command with very short line
#[test]
fn list_command_short_line() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "l"]).write_stdin("ab\n");
    cmd.assert().success().stdout("ab$\n");
    verify_against_sed!("l", "ab\n", &["-n"]);
}

/// Test list command with null byte
#[test]
fn list_command_null_byte() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "l"]).write_stdin("a\x00b\n");
    cmd.assert().success().stdout("a\\000b$\n");
}

/// Test print filename (F) with stdin
#[test]
fn print_filename_stdin() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "F"]).write_stdin("test\n");
    cmd.assert().success(); // Output should be stdin path or "-"
}

/// Test print filename (F) with file
#[test]
fn print_filename_with_file() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "content").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.args(["-n", "-e", "F"]).arg(tmp.path());
    // Output should contain the filename
    cmd.assert().success().stdout(format!("{}\n", path));
}

/// Test clear (z) command
#[test]
fn clear_command_explicit() {
    let mut cmd = bin();
    cmd.args(["-e", "z;s/^$/EMPTY/"]).write_stdin("content\n");
    cmd.assert().success().stdout("EMPTY\n");
    verify_against_sed!("z;s/^$/EMPTY/", "content\n", &[]);
}

/// Test execute (e) with explicit command argument
#[test]
fn execute_explicit_command() {
    let mut cmd = bin();
    cmd.args(["-e", "e echo hello"]).write_stdin("input\n");
    cmd.assert().success().stdout("hello\ninput\n");
    verify_against_sed!("e echo hello", "input\n", &[]);
}

/// Test substitution with \0 backreference
#[test]
fn subst_backref_zero_same_as_ampersand() {
    let mut cmd = bin();
    cmd.args(["-e", "s/[a-z]\\+/[\\0]/g"])
        .write_stdin("hello world\n");
    cmd.assert().success().stdout("[hello] [world]\n");
    verify_against_sed!("s/[a-z]\\+/[\\0]/g", "hello world\n", &[]);
}

/// Test multiple nested case transforms
#[test]
fn case_transforms_nested() {
    let mut cmd = bin();
    cmd.args(["-e", "s/\\([a-z]\\)\\([a-z]*\\)/\\u\\1\\L\\2/g"])
        .write_stdin("hello WORLD test\n");
    cmd.assert().success().stdout("Hello WORLD Test\n");
    verify_against_sed!(
        "s/\\([a-z]\\)\\([a-z]*\\)/\\u\\1\\L\\2/g",
        "hello WORLD test\n",
        &[]
    );
}

/// Test substitution with print timing pe (print then execute)
#[test]
fn subst_pe_print_then_execute() {
    let mut cmd = bin();
    cmd.args(["-e", "s/.*/echo REPLACED/pe"])
        .write_stdin("original\n");
    // pe = print then execute: prints "echo REPLACED" then executes it
    cmd.assert().success();
}

/// Test line number command (=) with address
#[test]
fn line_number_with_regex_address() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "/foo/="])
        .write_stdin("bar\nfoo\nbaz\nfoo\n");
    cmd.assert().success().stdout("2\n4\n");
    verify_against_sed!("/foo/=", "bar\nfoo\nbaz\nfoo\n", &["-n"]);
}

/// Test insert command with regex address
#[test]
fn insert_with_regex_match() {
    let mut cmd = bin();
    cmd.args(["-e", "/^target$/i\\BEFORE"])
        .write_stdin("line1\ntarget\nline3\n");
    cmd.assert()
        .success()
        .stdout("line1\nBEFORE\ntarget\nline3\n");
    verify_against_sed!("/^target$/i\\BEFORE", "line1\ntarget\nline3\n", &[]);
}

/// Test append command at EOF
#[test]
fn append_at_eof() {
    let mut cmd = bin();
    cmd.args(["-e", "$a\\END"]).write_stdin("line1\nline2\n");
    cmd.assert().success().stdout("line1\nline2\nEND\n");
    verify_against_sed!("$a\\END", "line1\nline2\n", &[]);
}

/// Test change (c) command with negated address
#[test]
fn change_negated_single_address() {
    let mut cmd = bin();
    cmd.args(["-e", "2!c\\CHANGED"]).write_stdin("l1\nl2\nl3\n");
    cmd.assert().success().stdout("CHANGED\nl2\nCHANGED\n");
    verify_against_sed!("2!c\\CHANGED", "l1\nl2\nl3\n", &[]);
}

/// Test N command with quiet mode
#[test]
fn n_command_quiet_mode() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "N;p"]).write_stdin("a\nb\nc\nd\n");
    cmd.assert().success().stdout("a\nb\nc\nd\n");
    verify_against_sed!("N;p", "a\nb\nc\nd\n", &["-n"]);
}

/// Test next (n) at EOF
#[test]
fn next_at_eof() {
    let mut cmd = bin();
    cmd.args(["-e", "n;d"]).write_stdin("a\nb\n");
    cmd.assert().success().stdout("a\n");
    verify_against_sed!("n;d", "a\nb\n", &[]);
}

/// Test substitution with hex escape in replacement
#[test]
fn subst_hex_escape_high_byte() {
    let mut cmd = bin();
    cmd.args(["-e", "s/X/\\xC0/"]).write_stdin("X\n");
    cmd.assert().success();
    // Should contain the byte 0xC0
}

/// Test substitution with octal escape
#[test]
fn subst_octal_escape_nul() {
    let mut cmd = bin();
    cmd.args(["-e", "s/X/\\o000/"]).write_stdin("X\n");
    cmd.assert().success();
    // Should produce a NUL byte
}

/// Test translate with pipe delimiter
#[test]
fn translate_pipe_delimiter() {
    let mut cmd = bin();
    cmd.args(["-e", "y|abc|XYZ|"]).write_stdin("abc\n");
    cmd.assert().success().stdout("XYZ\n");
    verify_against_sed!("y|abc|XYZ|", "abc\n", &[]);
}

/// Test substitution global with print (gp)
#[test]
fn subst_global_print() {
    let mut cmd = bin();
    cmd.args(["-n", "-e", "s/a/X/gp"]).write_stdin("aaa\nbbb\n");
    cmd.assert().success().stdout("XXX\n");
    verify_against_sed!("s/a/X/gp", "aaa\nbbb\n", &["-n"]);
}

/// Test substitution with write file global
#[test]
fn subst_write_global() {
    use std::fs;
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_str().unwrap();

    let mut cmd = bin();
    cmd.args(["-e", &format!("s/foo/bar/gw {}", tmp_path)])
        .write_stdin("foo foo\nbaz\nfoo\n");
    cmd.assert().success().stdout("bar bar\nbaz\nbar\n");

    let written = fs::read_to_string(tmp.path()).unwrap();
    assert_eq!(written, "bar bar\nbar\n");
}

/// Test zero address with read prepend
#[test]
fn zero_address_read_prepend() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "prepended").unwrap();
    tmp.flush().unwrap();

    let mut cmd = bin();
    cmd.args(["-e", &format!("0r {}", tmp.path().display())])
        .write_stdin("first\nsecond\n");
    cmd.assert().success().stdout("prepended\nfirst\nsecond\n");
}

/// Test zero address with tilde step
#[test]
fn zero_tilde_step() {
    let mut cmd = bin();
    cmd.args(["-e", "0~3s/^/X/"])
        .write_stdin("a\nb\nc\nd\ne\nf\n");
    cmd.assert().success().stdout("a\nb\nXc\nd\ne\nXf\n");
    verify_against_sed!("0~3s/^/X/", "a\nb\nc\nd\ne\nf\n", &[]);
}

/// Test version command (v) with current version
#[test]
fn version_command_current() {
    let mut cmd = bin();
    cmd.args(["-e", "v 4.0"]).write_stdin("test\n");
    cmd.assert().success().stdout("test\n");
}

/// Test version command failure
#[test]
fn version_command_future_fails() {
    let mut cmd = bin();
    cmd.args(["-e", "v 99.0"]).write_stdin("test\n");
    cmd.assert().failure();
}

/// Test N command followed by substitution across lines
#[test]
fn n_then_subst_multiline() {
    let mut cmd = bin();
    cmd.args(["-e", "N;s/\\n/ /"]).write_stdin("a\nb\nc\nd\n");
    cmd.assert().success().stdout("a b\nc d\n");
    verify_against_sed!("N;s/\\n/ /", "a\nb\nc\nd\n", &[]);
}

/// Test grouped commands with multiple branches
#[test]
fn grouped_commands_complex() {
    let mut cmd = bin();
    cmd.args(["-e", "/a/{s/a/A/;b end};/b/s/b/B/;:end"])
        .write_stdin("a\nb\nc\n");
    cmd.assert().success().stdout("A\nB\nc\n");
    verify_against_sed!("/a/{s/a/A/;b end};/b/s/b/B/;:end", "a\nb\nc\n", &[]);
}
