#!/bin/bash

# Copyright (c) 2026 Red Authors
# License: MIT
#

# Benchmark red vs GNU sed
# Requires: hyperfine (cargo install hyperfine)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RED_BIN="${RED_BIN:-$SCRIPT_DIR/../target/release/red}"
SED_BIN="${SED_BIN:-sed}"
RUNS="${RUNS:-20}"
WARMUP="${WARMUP:-3}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Red vs GNU sed Benchmark ===${NC}"
echo ""

# Check dependencies
if ! command -v hyperfine &> /dev/null; then
    echo -e "${RED}Error: hyperfine not found${NC}"
    echo "Install with: cargo install hyperfine"
    exit 1
fi

if [[ ! -f "$RED_BIN" ]]; then
    echo -e "${YELLOW}Building red in release mode...${NC}"
    (cd "$SCRIPT_DIR/.." && cargo build --release)
fi

echo "Red binary: $RED_BIN"
echo "Sed binary: $SED_BIN"
echo "Runs: $RUNS, Warmup: $WARMUP"
echo ""

# Create temp directory
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Generate test files
echo -e "${YELLOW}Generating test files...${NC}"

# Small file (1K lines)
yes "hello world foo bar baz" | head -1000 > "$TMPDIR/small.txt"

# Medium file (100K lines)
yes "The quick brown fox jumps over the lazy dog. foo bar baz 12345" | head -100000 > "$TMPDIR/medium.txt"

# Large file (1M lines)
yes "Lorem ipsum dolor sit amet, consectetur adipiscing elit. foo bar" | head -1000000 > "$TMPDIR/large.txt"

# File with many matches
seq 1 100000 | while read n; do echo "line $n: foo foo foo bar bar baz"; done > "$TMPDIR/many_matches.txt"

echo "  small.txt:        $(wc -l < "$TMPDIR/small.txt") lines ($(du -h "$TMPDIR/small.txt" | cut -f1))"
echo "  medium.txt:       $(wc -l < "$TMPDIR/medium.txt") lines ($(du -h "$TMPDIR/medium.txt" | cut -f1))"
echo "  large.txt:        $(wc -l < "$TMPDIR/large.txt") lines ($(du -h "$TMPDIR/large.txt" | cut -f1))"
echo "  many_matches.txt: $(wc -l < "$TMPDIR/many_matches.txt") lines ($(du -h "$TMPDIR/many_matches.txt" | cut -f1))"
echo ""

# Benchmark functions
run_bench() {
    local name="$1"
    local sed_cmd="$2"
    local red_cmd="$3"

    echo -e "${GREEN}--- $name ---${NC}"
    hyperfine --warmup "$WARMUP" --runs "$RUNS" \
        --command-name "sed" "$sed_cmd" \
        --command-name "red" "$red_cmd"
    echo ""
}

# ============================================
# Benchmarks
# ============================================

echo -e "${GREEN}=== Simple Substitution ===${NC}"
echo ""

run_bench "Small file - simple s///" \
    "$SED_BIN 's/foo/FOO/' $TMPDIR/small.txt > /dev/null" \
    "$RED_BIN 's/foo/FOO/' $TMPDIR/small.txt > /dev/null"

run_bench "Medium file - simple s///" \
    "$SED_BIN 's/foo/FOO/' $TMPDIR/medium.txt > /dev/null" \
    "$RED_BIN 's/foo/FOO/' $TMPDIR/medium.txt > /dev/null"

run_bench "Large file - simple s///" \
    "$SED_BIN 's/foo/FOO/' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN 's/foo/FOO/' $TMPDIR/large.txt > /dev/null"

echo -e "${GREEN}=== Global Substitution ===${NC}"
echo ""

run_bench "Many matches - s///g" \
    "$SED_BIN 's/foo/FOO/g' $TMPDIR/many_matches.txt > /dev/null" \
    "$RED_BIN 's/foo/FOO/g' $TMPDIR/many_matches.txt > /dev/null"

run_bench "Large file - s///g" \
    "$SED_BIN 's/foo/FOO/g' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN 's/foo/FOO/g' $TMPDIR/large.txt > /dev/null"

echo -e "${GREEN}=== Regex Patterns ===${NC}"
echo ""

run_bench "Medium file - word boundary regex" \
    "$SED_BIN 's/\\bfoo\\b/FOO/g' $TMPDIR/medium.txt > /dev/null" \
    "$RED_BIN 's/\\bfoo\\b/FOO/g' $TMPDIR/medium.txt > /dev/null"

run_bench "Medium file - capture groups" \
    "$SED_BIN 's/\\(foo\\) \\(bar\\)/\\2 \\1/g' $TMPDIR/medium.txt > /dev/null" \
    "$RED_BIN 's/\\(foo\\) \\(bar\\)/\\2 \\1/g' $TMPDIR/medium.txt > /dev/null"

run_bench "Large file - extended regex" \
    "$SED_BIN -E 's/(foo|bar|baz)/WORD/g' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN -E 's/(foo|bar|baz)/WORD/g' $TMPDIR/large.txt > /dev/null"

echo -e "${GREEN}=== Delete Lines ===${NC}"
echo ""

run_bench "Large file - delete matching lines" \
    "$SED_BIN '/foo/d' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN '/foo/d' $TMPDIR/large.txt > /dev/null"

run_bench "Large file - delete line range" \
    "$SED_BIN '1000,2000d' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN '1000,2000d' $TMPDIR/large.txt > /dev/null"

echo -e "${GREEN}=== Print Lines ===${NC}"
echo ""

run_bench "Large file - print matching (-n /p/p)" \
    "$SED_BIN -n '/foo/p' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN -n '/foo/p' $TMPDIR/large.txt > /dev/null"

run_bench "Large file - print range" \
    "$SED_BIN -n '50000,60000p' $TMPDIR/large.txt > /dev/null" \
    "$RED_BIN -n '50000,60000p' $TMPDIR/large.txt > /dev/null"

echo -e "${GREEN}=== Multiple Commands ===${NC}"
echo ""

run_bench "Medium file - multiple -e" \
    "$SED_BIN -e 's/foo/FOO/g' -e 's/bar/BAR/g' -e 's/baz/BAZ/g' $TMPDIR/medium.txt > /dev/null" \
    "$RED_BIN -e 's/foo/FOO/g' -e 's/bar/BAR/g' -e 's/baz/BAZ/g' $TMPDIR/medium.txt > /dev/null"

echo -e "${GREEN}=== Complex Script ===${NC}"
echo ""

run_bench "Medium file - complex script" \
    "$SED_BIN '/^$/d; s/foo/FOO/g; s/bar/BAR/g; /Lorem/s/ipsum/IPSUM/' $TMPDIR/medium.txt > /dev/null" \
    "$RED_BIN '/^$/d; s/foo/FOO/g; s/bar/BAR/g; /Lorem/s/ipsum/IPSUM/' $TMPDIR/medium.txt > /dev/null"

# ============================================
# Summary
# ============================================

echo -e "${GREEN}=== Benchmark Complete ===${NC}"
