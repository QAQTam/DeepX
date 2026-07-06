#!/bin/bash

# Copyright (c) 2026 Red Authors
# License: MIT
#

# MB Regex Tests for Shift-JIS and EUC-JP
# Tests regex matching in non-UTF-8 multibyte locales

RED_BIN="${RED_BIN:-/home/builder/red/red/target/release/red}"

PASS=0
FAIL=0
SKIP=0

GREEN='\033[0;32m'
RED_COLOR='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

run_test() {
    local name="$1"
    local locale="$2"
    local cmd="$3"
    local input_hex="$4"

    # Check if locale is available
    if ! locale -a 2>/dev/null | grep -q "^${locale}$"; then
        echo -e "${YELLOW}[SKIP]${NC}: $name (locale $locale not available)"
        SKIP=$((SKIP + 1))
        return
    fi

    # Run both sed and red
    sed_out=$(printf "$input_hex" | xxd -r -p | LC_ALL=$locale sed "$cmd" 2>/dev/null | xxd -p | tr -d '\n')
    sed_exit=$?
    red_out=$(printf "$input_hex" | xxd -r -p | LC_ALL=$locale "$RED_BIN" "$cmd" 2>/dev/null | xxd -p | tr -d '\n')
    red_exit=$?

    if [ "$sed_out" = "$red_out" ] && [ "$sed_exit" = "$red_exit" ]; then
        echo -e "${GREEN}[PASS]${NC}: $name"
        PASS=$((PASS + 1))
    else
        echo -e "${RED_COLOR}[FAIL]${NC}: $name"
        echo "  Command: $cmd"
        echo "  Input:   $input_hex"
        echo "  sed:     $sed_out (exit $sed_exit)"
        echo "  red:     $red_out (exit $red_exit)"
        FAIL=$((FAIL + 1))
    fi
}

echo "========================================"
echo "MB Regex Tests"
echo "========================================"
echo ""

# Check if red binary exists
if [ ! -x "$RED_BIN" ]; then
    echo "Error: red binary not found at $RED_BIN"
    exit 1
fi

echo "=== Shift-JIS Dot (.) Tests ==="
# Input: 83 5B 83 5D = two Shift-JIS chars
run_test "sjis_dot_single" "ja_JP.shiftjis" "s/./X/" "835b835d"
run_test "sjis_dot_global" "ja_JP.shiftjis" "s/./X/g" "835b835d"
run_test "sjis_dot_star" "ja_JP.shiftjis" "s/.*/X/" "835b835d"
run_test "sjis_two_dots" "ja_JP.shiftjis" "s/../Y/" "835b835d"

echo ""
echo "=== EUC-JP Dot (.) Tests ==="
# Input: A4 A2 A4 A4 = two EUC-JP chars (hiragana a, i)
run_test "eucjp_dot_single" "ja_JP.eucjp" "s/./X/" "a4a2a4a4"
run_test "eucjp_dot_global" "ja_JP.eucjp" "s/./X/g" "a4a2a4a4"

echo ""
echo "=== Mixed ASCII/MB Tests ==="
# Input: 61 83 5B 62 = 'a' + MB char + 'b'
run_test "sjis_mixed_single" "ja_JP.shiftjis" "s/./X/" "61835b62"
run_test "sjis_mixed_global" "ja_JP.shiftjis" "s/./X/g" "61835b62"
run_test "sjis_mixed_dot_star" "ja_JP.shiftjis" "s/.*/X/" "61835b62"

echo ""
echo "=== Character Class Tests ==="
# ASCII class should only match ASCII chars
run_test "sjis_ascii_class" "ja_JP.shiftjis" "s/[a-z]/X/g" "61835b62"
run_test "sjis_digit_class" "ja_JP.shiftjis" "s/[0-9]/X/g" "31835b32"

echo ""
echo "=== Negated Class Tests ==="
# Negated class - [^a] should match MB chars
run_test "sjis_neg_class" "ja_JP.shiftjis" "s/[^a]/X/g" "61835b62"

echo ""
echo "=== Quantifier Tests ==="
run_test "sjis_dot_plus" "ja_JP.shiftjis" "s/.+/X/" "835b835d"
run_test "sjis_dot_question" "ja_JP.shiftjis" "s/.\\?/X/" "835b835d"

echo ""
echo "=== Backreference Tests ==="
# Two identical MB chars
run_test "sjis_backref_same" "ja_JP.shiftjis" 's/\(.\)\1/X/' "835b835b"
# Different MB chars (should not match)
run_test "sjis_backref_diff" "ja_JP.shiftjis" 's/\(.\)\1/X/' "835b835d"

echo ""
echo "=== Capture Group Tests ==="
# Reverse 3 chars
run_test "sjis_capture_reverse" "ja_JP.shiftjis" 's/\(.\)\(.\)\(.\)/\3\2\1/' "61835b62"

echo ""
echo "=== Edge Cases ==="
# Empty input
run_test "sjis_empty" "ja_JP.shiftjis" "s/./X/" ""
# Single byte
run_test "sjis_single_byte" "ja_JP.shiftjis" "s/./X/" "61"
# Incomplete MB at end
run_test "sjis_incomplete" "ja_JP.shiftjis" "s/./X/g" "61835b83"

echo ""
echo "========================================"
echo "Summary"
echo "========================================"
echo -e "Passed:  ${GREEN}$PASS${NC}"
echo -e "Failed:  ${RED_COLOR}$FAIL${NC}"
echo -e "Skipped: ${YELLOW}$SKIP${NC}"
echo ""

if [ $FAIL -gt 0 ]; then
    echo "Some tests failed!"
    exit 1
else
    echo "All tests passed!"
    exit 0
fi
