#!/bin/bash

# Copyright (c) 2026 Red Authors
# License: MIT
#

# Compare GNU sed and red output for given input
#
# Usage: compare_sed_red.sh "sed_command" [input_file]
# Or:    echo "input" | compare_sed_red.sh "sed_command"
#
# Examples:
#   ./compare_sed_red.sh 's/a/X/g' <<< "abba"
#   ./compare_sed_red.sh 'y/abc/xyz/' input.txt
#   printf 'a\x80b' | ./compare_sed_red.sh 's/a/X/'

set -e

RED_BIN="${RED_BIN:-/home/builder/red/red/target/release/red}"

if [ -z "$1" ]; then
    echo "Usage: $0 'sed_command' [input_file]"
    echo "Or:    echo 'input' | $0 'sed_command'"
    exit 1
fi

SED_CMD="$1"
INPUT_FILE="$2"

# Create temp files for output
SED_OUT=$(mktemp)
RED_OUT=$(mktemp)
trap "rm -f $SED_OUT $RED_OUT" EXIT

# Run both commands
if [ -n "$INPUT_FILE" ]; then
    sed "$SED_CMD" < "$INPUT_FILE" > "$SED_OUT" 2>&1
    SED_EXIT=$?
    "$RED_BIN" "$SED_CMD" < "$INPUT_FILE" > "$RED_OUT" 2>&1
    RED_EXIT=$?
else
    # Read from stdin
    INPUT=$(cat)
    echo -n "$INPUT" | sed "$SED_CMD" > "$SED_OUT" 2>&1
    SED_EXIT=$?
    echo -n "$INPUT" | "$RED_BIN" "$SED_CMD" > "$RED_OUT" 2>&1
    RED_EXIT=$?
fi

# Compare outputs
if diff -q "$SED_OUT" "$RED_OUT" > /dev/null 2>&1 && [ "$SED_EXIT" = "$RED_EXIT" ]; then
    echo "[PASS]: $SED_CMD"
    exit 0
else
    echo "[FAIL]: $SED_CMD"
    echo "  Exit codes: sed=$SED_EXIT red=$RED_EXIT"
    echo "  sed output (hex):"
    xxd "$SED_OUT" | head -5 | sed 's/^/    /'
    echo "  red output (hex):"
    xxd "$RED_OUT" | head -5 | sed 's/^/    /'
    exit 1
fi
