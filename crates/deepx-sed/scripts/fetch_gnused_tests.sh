#!/usr/bin/env bash
set -Eeuo pipefail

# Copyright (c) 2026 Red Authors
# License: MIT
#

usage() {
  cat <<'USAGE'
Fetch GNU sed tests via 'git clone'.

Usage:
  fetch_gnused_tests.sh [--dest /path/to/dir] [--git-url URL] [--branch BRANCH]

Options:
  --dest DIR       Destination directory to place cloned tests (must NOT exist).
                   Default: <crate_root>/tests/gnused-tests
  --git-url URL    Override Git URL (default: git://git.sv.gnu.org/sed)
  --branch BRANCH  Git branch to clone (default: master)

Notes:
  - Requires 'git' to be installed.
  - The script only downloads the full GNU sed repository; the tests are in testsuite/.
  - Running tests is handled by a separate script.
USAGE
}

GIT_URL_DEFAULT="git://git.sv.gnu.org/sed"
GIT_BRANCH="master"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEST_DIR="${CRATE_ROOT}/tests/gnused-tests"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dest)
      DEST_DIR="${2:-}"; shift 2 ;;
    --git-url)
      GIT_URL_DEFAULT="${2:-}"; shift 2 ;;
    --branch)
      GIT_BRANCH="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[error] Unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

if ! command -v git >/dev/null 2>&1; then
  echo "[error] 'git' not found. Install git." >&2
  exit 1
fi

if [[ -e "${DEST_DIR}" ]]; then
  echo "[error] Destination already exists: ${DEST_DIR}. Remove it or provide a different --dest." >&2
  exit 2
fi
mkdir -p "$(dirname "${DEST_DIR}")"

echo "[info] Cloning GNU sed repository"
echo "[info]   URL: ${GIT_URL_DEFAULT}"
echo "[info]   Branch: ${GIT_BRANCH}"
echo "[info]   Dest: ${DEST_DIR}"

# Clone with depth 1 to save space and time
git clone --depth 1 --branch "${GIT_BRANCH}" "${GIT_URL_DEFAULT}" "${DEST_DIR}" 2>&1 | \
  grep -v "^remote:" || true

if [[ ! -d "${DEST_DIR}/testsuite" ]]; then
  echo "[error] Expected testsuite/ directory not found in cloned repository" >&2
  exit 1
fi

# Initialize gnulib submodule (required for proper test framework)
echo "[info] Initializing gnulib submodule (test framework)"
cd "${DEST_DIR}"
git submodule update --init --depth 1 gnulib 2>&1 | grep -v "^remote:" || true

if [[ ! -f "${DEST_DIR}/gnulib/tests/init.sh" ]]; then
  echo "[error] gnulib/tests/init.sh not found after submodule initialization" >&2
  exit 1
fi

# Compile test helpers (used by locale detection in tests)
# Original files require autotools-generated headers, so we provide stubs
# On Windows/systems without gcc, skip compilation (some locale tests will be skipped)
if command -v gcc >/dev/null 2>&1; then
  echo "[info] Compiling test helpers"
  touch "${DEST_DIR}/testsuite/config.h"
  cat > "${DEST_DIR}/testsuite/progname.h" << 'EOF'
#define set_program_name(x) (void)0
#define program_name "test-helper"
EOF
  # error() is a GNU extension, provide a stub for Windows/MinGW
  cat > "${DEST_DIR}/testsuite/error.h" << 'EOF'
#if defined(_WIN32) || defined(__MINGW32__) || defined(__CYGWIN__)
#include <stdio.h>
#include <stdlib.h>
#define error(status, errnum, ...) do { fprintf(stderr, __VA_ARGS__); if (status) exit(status); } while(0)
#else
#include_next <error.h>
#endif
EOF
  gcc -I"${DEST_DIR}/testsuite" -o "${DEST_DIR}/testsuite/get-mb-cur-max" \
      "${DEST_DIR}/testsuite/get-mb-cur-max.c" 2>/dev/null || \
      echo "[warn] Failed to compile get-mb-cur-max (some locale tests will be skipped)"
  gcc -I"${DEST_DIR}/testsuite" -include "${DEST_DIR}/testsuite/error.h" \
      -o "${DEST_DIR}/testsuite/test-mbrtowc" \
      "${DEST_DIR}/testsuite/test-mbrtowc.c" 2>/dev/null || \
      echo "[warn] Failed to compile test-mbrtowc (some locale tests will be skipped)"
else
  echo "[warn] gcc not found - skipping test helper compilation (some locale tests will be skipped)"
fi

echo "[info] Done. GNU sed repository cloned to: ${DEST_DIR}"
echo "[info] Tests are located in: ${DEST_DIR}/testsuite"
echo "[info] Test framework (gnulib) initialized: ${DEST_DIR}/gnulib"
