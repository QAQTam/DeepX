#!/bin/bash

# Copyright (c) 2026 Red Authors
# License: MIT
#

# Compare cargo test expectations against GNU sed behavior

RED_BIN="${RED_BIN:-target/release/red}"

# Build red first
cargo build --release 2>/dev/null

PASS=0
FAIL=0
DIFF_FOUND=()

compare() {
    local name="$1"
    local script="$2"
    local input="$3"
    local args="${4:-}"

    # Run both sed and red
    local sed_out red_out sed_status red_status

    if [ -n "$args" ]; then
        sed_out=$(printf '%s' "$input" | sed $args -e "$script" 2>/dev/null) || sed_status=$?
        red_out=$(printf '%s' "$input" | $RED_BIN $args -e "$script" 2>/dev/null) || red_status=$?
    else
        sed_out=$(printf '%s' "$input" | sed -e "$script" 2>/dev/null) || sed_status=$?
        red_out=$(printf '%s' "$input" | $RED_BIN -e "$script" 2>/dev/null) || red_status=$?
    fi

    if [ "$sed_out" = "$red_out" ]; then
        echo "[PASS] $name"
        ((PASS++))
    else
        echo "[FAIL] $name"
        echo "  script: $script"
        echo "  input: $(printf '%s' "$input" | head -c 50 | cat -v)..."
        echo "  sed: $(printf '%s' "$sed_out" | head -c 80 | cat -v)"
        echo "  red: $(printf '%s' "$red_out" | head -c 80 | cat -v)"
        ((FAIL++))
        DIFF_FOUND+=("$name")
    fi
}

echo "=== Comparing cargo test expectations with GNU sed ==="
echo ""

# Basic substitution tests
compare "basic_substitution_once" 's/foo/bar/' $'foo\nfoo\n'
compare "basic_substitution_global" 's/foo/bar/g' 'foo foo'
compare "replacement_ampersand" 's/[0-9][0-9]*/&X/g' 'a1 b22'
compare "backreferences" 's/\([a-z][a-z]*\)-\([0-9][0-9]*\)/\2-\1/' 'abc-123'
compare "delimiter_custom" 's#foo/bar#baz#' 'foo/bar'
compare "quiet_mode" 's/x/y/' 'x' '-n'
compare "multiple_scripts" 's/a/A/;s/b/B/' 'ab'
compare "delimiter_bracket" 's[abc[X[g' 'abc abc'

# Address tests
compare "address_numeric" '2p' $'a\nb\nc' '-n'
compare "address_last_line" '$p' $'x\ny\nz' '-n'
compare "address_regex" '/^foo$/p' $'bar\nfoo\nbaz' '-n'
compare "range_numeric" '1,2p' $'l1\nl2\nl3' '-n'
compare "range_to_dollar" '2,$p' $'l1\nl2\nl3' '-n'
compare "range_regex_single" '/^x$/,/^x$/p' $'a\nx\ny' '-n'
compare "negation" '2!p' $'a\nb\nc' '-n'
compare "step_address" '0~2p' $'1\n2\n3\n4\n5' '-n'

# BRE tests
compare "bre_counted_exact" 's/\(ab\)\{2\}/X/' $'abab\naba'
compare "bre_counted_range" 's/a\{2,3\}/X/g' 'a aa aaa aaaa'
compare "bre_posix_alpha" 's/[[:alpha:]]\{3\}/X/' $'abc-123\n12abc34'
compare "bre_escape_bracket" 's/[]a]/_/g' '] a b ]a'
compare "bre_ignore_case" 's/foo/bar/Ig' 'Foo fOo foo'
compare "escape_ampersand_backslash" 's/[0-9][0-9]*/\&-\\/g' 'a1 b22 c333'

# Nth occurrence
compare "nth_occurrence" 's/./X/4' 'abcd'

# Substitution flags
compare "subst_2g" 's/a/X/2g' 'aaaa'
compare "subst_g2" 's/o/X/g2' 'foo'
compare "subst_p_flag" 's/foo/bar/p' $'foo\nxxx\nfoo' '-n'

# y command
compare "y_digits" 'y/0123456789/9876543210/' '2019'
compare "y_custom_delim" 'y#abc#XYZ#' 'cab'
compare "y_escape_tab" 'y/a/\t/' 'aXa'
compare "y_octal" 'y/\141\142/\102\103/' 'ab'

# Print and line number
compare "print_address" '2p' $'a\nb\nc' '-n'
compare "line_number_regex" '/foo/=' $'bar\nfoo\nbaz' '-n'

# List command
compare "list_escapes" 'l' $'a\tb\\c\x07' '-n'

# a/i/c commands
compare "append_insert" '2i\
I
2a\
A
p' $'1\n2\n3' '-n'

compare "delete_range" '2,3d;p' $'1\n2\n3\n4' '-n'

compare "change_single" '2c\
X
p' $'1\n2\n3' '-n'

# n/N/D commands
compare "next_cmd" 'n' $'a\nb'
compare "big_n" 'N;s/\n/;/;p' $'a\nb' '-n'
compare "big_d" 'N;D;p' $'1\n2\n3' '-n'

# Hold space
compare "hold_get" 'h;g;p' 'hello' '-n'
compare "hold_append" 'H;g;p' $'a\nb' '-n'
compare "exchange" 'h;x;p' 'x' '-n'

# Branching
compare "t_branch_success" 's/x/y/;t end;s/y/z/;:end;p' 'x' '-n'
compare "t_branch_skip" 't end;s/x/y/;:end;p' 'x' '-n'
compare "b_to_end" 'b;p' 'x' '-n'
compare "addressed_branch" '2b e;p;:e;=' $'1\n2\n3' '-n'

# Quit
compare "quit_basic" 'q' 'hello'
compare "quit_address" '2q;p' $'1\n2\n3' '-n'

# e flag (execute)
compare "e_with_autoprint" 's/.*/echo test/e' 'foo'
compare "ep_execute_print" 's/.*/echo test/ep' 'foo' '-n'
compare "pe_print_execute" 's/.*/echo test/pe' 'foo' '-n'

# Combined flags
compare "pg_flags" 's/o/X/pg' 'foo' '-n'
compare "gp_flags" 's/o/X/gp' 'foo' '-n'
compare "gpi_flags" 's/FOO/bar/gpi' 'FOO foo FOO' '-n'

# Empty match tests
compare "empty_match_star" 's/a*/X/g' 'bbb'
compare "empty_match_question" 's/a\{0,1\}/X/g' 'bbb'
compare "empty_match_alpha_star" 's/[[:alpha:]]*/WORD/g' 'test123'

echo ""
echo "=== Summary ==="
echo "Passed: $PASS"
echo "Failed: $FAIL"

if [ $FAIL -gt 0 ]; then
    echo ""
    echo "Tests with differences:"
    for t in "${DIFF_FOUND[@]}"; do
        echo "  - $t"
    done
    exit 1
fi
