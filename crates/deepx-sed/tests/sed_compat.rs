// Copyright (c) 2026 Red Authors
// License: MIT
//

// Integration tests comparing red vs GNU sed
// Run with: cargo test --test sed_compat
//
// These tests run sed and red with the same script/input and compare output.
// The common module provides shared utilities that can be reused by other tests.

mod common;

use common::{compare_sed_red, compare_with_locale, locale_available};

/// Assert comparison matches, with helpful error message
macro_rules! assert_match {
    ($r:expr, $desc:expr) => {
        assert!(
            $r.matches,
            "{}\nsed stdout: {:?}\nred stdout: {:?}\nsed status: {}\nred status: {}",
            $desc,
            String::from_utf8_lossy(&$r.sed_stdout),
            String::from_utf8_lossy(&$r.red_stdout),
            $r.sed_status,
            $r.red_status
        );
    };
}

// ============================================================================
// CLI Flag Pair Tests
// ============================================================================

#[test]
fn flag_quiet_basic() {
    let r = compare_sed_red("s/a/X/p", "aaa\nbbb\n", &["-n"]);
    assert_match!(r, "quiet with print flag");
}

#[test]
fn flag_quiet_with_extended() {
    let r = compare_sed_red("s/(a+)/[\\1]/p", "aaa\nbbb\n", &["-n", "-E"]);
    assert_match!(r, "quiet + extended regex");
}

#[test]
fn flag_quiet_with_null_data() {
    let r = compare_sed_red("s/a/X/p", "a\0b\0c\0", &["-n", "-z"]);
    assert_match!(r, "quiet + null data");
}

#[test]
fn flag_extended_basic() {
    let r = compare_sed_red("s/(a+)/[\\1]/g", "aaa bbb aaa\n", &["-E"]);
    assert_match!(r, "extended regex basic");
}

#[test]
fn flag_extended_alternation() {
    let r = compare_sed_red("s/cat|dog/pet/g", "I have a cat and a dog\n", &["-E"]);
    assert_match!(r, "extended alternation");
}

#[test]
fn flag_extended_question() {
    let r = compare_sed_red("s/colou?r/COLOR/g", "color colour\n", &["-E"]);
    assert_match!(r, "extended question mark");
}

#[test]
fn flag_null_data_basic() {
    let r = compare_sed_red("s/a/X/g", "aaa\0bbb\0", &["-z"]);
    assert_match!(r, "null data basic");
}

// Skip on Windows: MSYS2 sed argument processing interferes with \n escape
#[cfg(not(windows))]
#[test]
fn flag_null_data_newlines_in_record() {
    let r = compare_sed_red("s/\\n/NEWLINE/g", "line1\nline2\0line3\nline4\0", &["-z"]);
    assert_match!(r, "null data with embedded newlines");
}

#[test]
fn flag_address_range_quiet() {
    let r = compare_sed_red("2,4s/a/X/p", "aaa\naaa\naaa\naaa\naaa\n", &["-n"]);
    assert_match!(r, "address range with quiet");
}

#[test]
fn flag_regex_address_extended() {
    let r = compare_sed_red("/^a+$/s/a/X/g", "aaa\nbbb\naaaa\n", &["-E"]);
    assert_match!(r, "regex address with extended");
}

// ============================================================================
// Substitution Flag Tests
// ============================================================================

#[test]
fn subst_global_basic() {
    let r = compare_sed_red("s/a/X/g", "banana\n", &[]);
    assert_match!(r, "global basic");
}

#[test]
fn subst_global_with_print() {
    let r = compare_sed_red("s/a/X/gp", "banana\n", &["-n"]);
    assert_match!(r, "global with print");
}

#[test]
fn subst_global_ignore_case() {
    let r = compare_sed_red("s/a/X/gi", "AaAaA\n", &[]);
    assert_match!(r, "global ignore case");
}

#[test]
fn subst_occurrence_2g() {
    let r = compare_sed_red("s/a/X/2g", "aaaaa\n", &[]);
    assert_match!(r, "occurrence 2g");
}

#[test]
fn subst_print_on_match() {
    let r = compare_sed_red("s/a/X/p", "abc\nxyz\nabc\n", &["-n"]);
    assert_match!(r, "print on match");
}

#[test]
fn subst_print_without_quiet() {
    let r = compare_sed_red("s/a/X/p", "abc\n", &[]);
    assert_match!(r, "print without quiet (prints twice)");
}

#[test]
fn subst_ignore_case_basic() {
    let r = compare_sed_red("s/hello/HELLO/i", "Hello HELLO hello\n", &[]);
    assert_match!(r, "ignore case basic");
}

#[test]
fn subst_ignore_case_char_class() {
    let r = compare_sed_red("s/[a-z]/X/gi", "AbCdE\n", &[]);
    assert_match!(r, "ignore case with char class");
}

#[test]
fn subst_multiline_caret() {
    let r = compare_sed_red("N;s/^/START:/gm", "line1\nline2\nline3\n", &[]);
    assert_match!(r, "multiline caret");
}

#[test]
fn subst_multiline_dollar() {
    let r = compare_sed_red("N;s/$/:END/gm", "line1\nline2\nline3\n", &[]);
    assert_match!(r, "multiline dollar");
}

#[test]
fn subst_occurrence_1() {
    let r = compare_sed_red("s/a/X/", "aaaaa\n", &[]);
    assert_match!(r, "first occurrence (default)");
}

#[test]
fn subst_occurrence_2() {
    let r = compare_sed_red("s/a/X/2", "aaaaa\n", &[]);
    assert_match!(r, "second occurrence");
}

#[test]
fn subst_occurrence_3() {
    let r = compare_sed_red("s/a/X/3", "aaaaa\n", &[]);
    assert_match!(r, "third occurrence");
}

#[test]
fn subst_occurrence_beyond() {
    let r = compare_sed_red("s/a/X/9", "aaaaa\n", &[]);
    assert_match!(r, "occurrence beyond matches");
}

#[test]
fn subst_backref_basic() {
    let r = compare_sed_red("s/\\(a\\)\\1/X/", "aa ab aa\n", &[]);
    assert_match!(r, "backreference basic");
}

#[test]
fn subst_backref_extended() {
    let r = compare_sed_red("s/(.)\\1/X/g", "aabbcc\n", &["-E"]);
    assert_match!(r, "backreference extended");
}

#[test]
fn subst_backref_in_replacement() {
    let r = compare_sed_red("s/\\(a*\\)/[\\1]/g", "aaa b aa\n", &[]);
    assert_match!(r, "backreference in replacement");
}

#[test]
fn subst_ampersand() {
    let r = compare_sed_red("s/[0-9]*/[&]/g", "abc123def456\n", &[]);
    assert_match!(r, "ampersand replacement");
}

// Skip on Windows: MSYS2 sed argument processing interferes with \& escape
#[cfg(not(windows))]
#[test]
fn subst_escaped_ampersand() {
    let r = compare_sed_red("s/a/\\&/g", "aaa\n", &[]);
    assert_match!(r, "escaped ampersand");
}

// ============================================================================
// Empty Match Tests (regression for infinite loop)
// ============================================================================

#[test]
fn empty_match_star() {
    let r = compare_sed_red("s/a*/X/g", "bbb\n", &[]);
    assert_match!(r, "empty match with star");
}

#[test]
fn empty_match_question() {
    let r = compare_sed_red("s/a?/X/g", "bbb\n", &["-E"]);
    assert_match!(r, "empty match with question");
}

#[test]
fn empty_match_caret() {
    let r = compare_sed_red("s/^/START:/", "hello\n", &[]);
    assert_match!(r, "empty match at start");
}

#[test]
fn empty_match_dollar() {
    let r = compare_sed_red("s/$/:END/", "hello\n", &[]);
    assert_match!(r, "empty match at end");
}

#[test]
fn empty_match_alpha_star() {
    let r = compare_sed_red("s/[[:alpha:]]*/WORD/g", "test123\n", &[]);
    assert_match!(r, "empty match alpha star");
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn edge_empty_input() {
    let r = compare_sed_red("s/a/X/g", "", &[]);
    assert_match!(r, "empty input");
}

#[test]
fn edge_no_match() {
    let r = compare_sed_red("s/xyz/ABC/g", "hello world\n", &[]);
    assert_match!(r, "no match");
}

#[test]
fn edge_empty_replacement() {
    let r = compare_sed_red("s/a//g", "banana\n", &[]);
    assert_match!(r, "empty replacement");
}

#[test]
fn edge_empty_pattern_reuse() {
    let r = compare_sed_red("/foo/s//bar/", "foo baz foo\n", &[]);
    assert_match!(r, "empty pattern reuses last");
}

// ============================================================================
// Locale Tests
// ============================================================================

#[test]
fn locale_utf8_basic() {
    if !locale_available("en_US.utf8") {
        return;
    }
    let r = compare_with_locale("en_US.utf8", "s/hello/world/g", b"hello hello\n");
    assert_match!(r, "UTF-8 basic");
}

#[test]
fn locale_utf8_cyrillic() {
    if !locale_available("ru_RU.utf8") {
        return;
    }
    let r = compare_with_locale("ru_RU.utf8", "s/привет/мир/g", "привет привет\n".as_bytes());
    assert_match!(r, "UTF-8 cyrillic");
}

#[test]
fn locale_utf8_dot_multibyte() {
    if !locale_available("en_US.utf8") {
        return;
    }
    let r = compare_with_locale("en_US.utf8", "s/./X/g", "日本\n".as_bytes());
    assert_match!(r, "UTF-8 dot matches multibyte char");
}

#[test]
fn locale_utf8_char_class() {
    if !locale_available("en_US.utf8") {
        return;
    }
    let r = compare_with_locale("en_US.utf8", "s/[[:alpha:]]/X/g", "abc123\n".as_bytes());
    assert_match!(r, "UTF-8 char class");
}

#[test]
fn locale_c_basic() {
    let r = compare_with_locale("C", "s/a/X/g", b"aaa\n");
    assert_match!(r, "C locale basic");
}

#[test]
fn locale_c_high_bytes() {
    let r = compare_with_locale("C", "s/./X/g", &[0x80, 0x81, 0x82, b'\n']);
    assert_match!(r, "C locale high bytes");
}

#[test]
fn locale_empty_match_c() {
    let r = compare_with_locale("C", "s/a*/X/g", b"bbb\n");
    assert_match!(r, "C locale empty match");
}

#[test]
fn locale_empty_match_utf8() {
    if !locale_available("en_US.utf8") {
        return;
    }
    let r = compare_with_locale("en_US.utf8", "s/a*/X/g", b"bbb\n");
    assert_match!(r, "UTF-8 locale empty match");
}

#[test]
fn locale_shiftjis_basic() {
    if !locale_available("ja_JP.shiftjis") {
        return;
    }
    let r = compare_with_locale("ja_JP.shiftjis", "s/a/X/g", b"aaa\n");
    assert_match!(r, "Shift-JIS basic");
}

#[test]
fn locale_eucjp_basic() {
    if !locale_available("ja_JP.eucjp") {
        return;
    }
    let r = compare_with_locale("ja_JP.eucjp", "s/a/X/g", b"aaa\n");
    assert_match!(r, "EUC-JP basic");
}

#[test]
fn locale_transliterate_c() {
    let r = compare_with_locale("C", "y/abc/xyz/", b"aabbcc\n");
    assert_match!(r, "transliterate C locale");
}

#[test]
fn locale_transliterate_utf8() {
    if !locale_available("en_US.utf8") {
        return;
    }
    let r = compare_with_locale("en_US.utf8", "y/abc/xyz/", b"aabbcc\n");
    assert_match!(r, "transliterate UTF-8");
}

// ============================================================================
// C Locale Regex Backtracking Tests
// ============================================================================

#[test]
fn locale_c_greedy_backtrack() {
    // Bug: in C locale, .*sk pattern wasn't matching correctly when 's' appeared before 'sk'
    // Expected: greedy .* should match "obinary.sh: " leaving "skipped"
    let r = compare_with_locale("C", "s/.*sk/X/", b"obinary.sh: skipped\n");
    assert_match!(r, "C locale greedy backtrack with repeated char");
}

#[test]
fn locale_c_greedy_backtrack_extended() {
    let r = compare_with_locale(
        "C",
        "s/.*skipped test: //",
        b"obinary.sh: skipped test: platform\n",
    );
    assert_match!(r, "C locale greedy backtrack extended");
}
