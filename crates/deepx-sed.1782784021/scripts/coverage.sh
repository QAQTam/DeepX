#!/usr/bin/env bash
set -Eeuo pipefail

# Copyright (c) 2026 Red Authors
# License: MIT
#

# This script generates Rust code coverage for the whole project using cargo-llvm-cov.
# It installs cargo-llvm-cov if missing, runs tests, and produces lcov and HTML reports.

usage() {
  cat <<'USAGE'
Generate coverage report for the whole Rust project using cargo-llvm-cov.

Usage:
  coverage.sh [--html] [--open] [--fail-under PCT] [--profile dev|release] [--no-install]

Options:
  --html           Generate HTML report (in addition to lcov.info)
  --open           Open the HTML report in a browser (implies --html)
  --fail-under PCT Fail if line coverage is below PCT (integer, e.g., 80)
  --profile P      Build profile to use: dev or release (default: dev)
  --no-install     Do not auto-install cargo-llvm-cov

Outputs:
  - coverage/lcov.info
  - coverage/html/ (when --html)
USAGE
}

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROFILE="dev"
GENERATE_HTML="false"
OPEN_HTML="false"
FAIL_UNDER=""
NO_INSTALL="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --html) GENERATE_HTML="true"; shift ;;
    --open) GENERATE_HTML="true"; OPEN_HTML="true"; shift ;;
    --fail-under) FAIL_UNDER="${2:-}"; shift 2 ;;
    --profile) PROFILE="${2:-dev}"; shift 2 ;;
    --no-install) NO_INSTALL="true"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[error] Unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ "${NO_INSTALL}" != "true" ]]; then
  if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "[info] Installing cargo-llvm-cov"
    cargo install cargo-llvm-cov --locked
  fi
fi

pushd "${CRATE_ROOT}" >/dev/null

# Clean previous coverage data to avoid contamination
echo "[info] Resetting coverage counters"
cargo llvm-cov clean --workspace

RUN_FLAGS=(
  --workspace
  --lcov --output-path coverage/lcov.info
  --ignore-filename-regex ".*/\.cargo/registry/.*|.*/rustc/.*|.*/target/.*"
)

# Apply cargo profile flag only for release; dev is default and does not accept --dev
if [[ "${PROFILE}" == "release" ]]; then
  RUN_FLAGS+=(--release)
fi

if [[ -n "${FAIL_UNDER}" ]]; then
  RUN_FLAGS+=(--fail-under-lines "${FAIL_UNDER}")
fi

echo "[info] Running tests with coverage"
# Ensure output directories exist
mkdir -p coverage
cargo llvm-cov ${RUN_FLAGS[@]}

if [[ "${GENERATE_HTML}" == "true" ]]; then
  echo "[info] Generating HTML report"
  mkdir -p coverage/html
  REPORT_FLAGS=(--html --output-dir coverage/html)
  if [[ "${OPEN_HTML}" == "true" ]]; then
    REPORT_FLAGS+=(--open)
  fi
  cargo llvm-cov report ${REPORT_FLAGS[@]}
fi

echo "[info] Coverage artifacts:"
echo " - $(realpath coverage/lcov.info)"
if [[ -d coverage/html ]]; then
  echo " - $(realpath coverage/html)"
fi

popd >/dev/null


