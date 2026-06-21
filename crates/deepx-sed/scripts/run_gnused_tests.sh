#!/usr/bin/env bash
set -Eeuo pipefail

# Copyright (c) 2026 Red Authors
# License: MIT
#

# Run GNU sed tests against the local 'red' binary.
# Note: GNU sed tests use init.sh from gnulib framework.

usage() {
  cat <<'USAGE'
Run GNU sed tests against the local 'red' binary.

Usage:
  run_gnused_tests.sh [--tests-dir /path/to/gnused/testsuite] [--debug] [--fail-on-error] [--timeout-sec N] [--no-build] [--compact] [--fast] [--test-pattern PATTERN]

Options:
  --tests-dir DIR      Path to GNU sed testsuite directory
                       (default: <crate_root>/tests/gnused-tests/testsuite).
  --debug              Build 'red' in debug mode (default: release).
  --fail-on-error      Exit with failure if any test fails
  --timeout-sec N      Per-test timeout in seconds (0 disables; default: 30)
  --no-build           Do not rebuild the binary (use the existing target output).
  --compact            Print compact output (only test results and errors).
  --fast               Convenience flag: implies --debug --compact --timeout-sec 10.
  --expensive          Run expensive/very-expensive tests (e.g., >2GB file tests)
  --test-pattern PAT   Only run tests matching the pattern (e.g., "execute-tests.sh")

Notes:
  - GNU sed tests require gnulib's init.sh framework
  - If init.sh is not available, tests that depend on it will be skipped
  - This runner prepares PATH so that 'sed' resolves to the local 'red' binary.
  - Some tests may require specific locales or tools (perl, etc.)
USAGE
}

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
DEFAULT_TESTS_DIR="${CRATE_ROOT}/red/tests/gnused-tests/testsuite"
TESTS_DIR="${DEFAULT_TESTS_DIR}"
BUILD_MODE="release"
FAIL_ON_ERROR="false"
TIMEOUT_SEC=30
NO_BUILD="false"
COMPACT="false"
EXPENSIVE="false"
TEST_PATTERN="*.sh"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tests-dir)
      TESTS_DIR="${2:-}"; shift 2 ;;
    --debug)
      BUILD_MODE="debug"; shift ;;
    --fail-on-error)
      FAIL_ON_ERROR="true"; shift ;;
    --timeout-sec)
      TIMEOUT_SEC="${2:-}"; shift 2 ;;
    --no-build)
      NO_BUILD="true"; shift ;;
    --compact)
      COMPACT="true"; shift ;;
    --fast)
      BUILD_MODE="debug"; COMPACT="true"; TIMEOUT_SEC=10; shift ;;
    --expensive)
      EXPENSIVE="true"; shift ;;
    --test-pattern)
      TEST_PATTERN="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[error] Unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

echo "[info] Tests dir: ${TESTS_DIR}"

if [[ ! -d "${TESTS_DIR}" ]]; then
  echo "[error] Tests directory not found: ${TESTS_DIR}" >&2
  echo "[info] Run 'scripts/fetch_gnused_tests.sh' first to download the tests" >&2
  exit 1
fi

if [[ "${NO_BUILD}" != "true" ]]; then
  echo "[info] Building red (${BUILD_MODE})"
  pushd "${CRATE_ROOT}/red" >/dev/null
  if [[ "${BUILD_MODE}" == "release" ]]; then
    cargo build --release
  else
    cargo build
  fi
  popd >/dev/null
else
  echo "[info] Skipping build (using existing binary)"
fi

if [[ "${BUILD_MODE}" == "release" ]]; then
  RED_BIN="${CRATE_ROOT}/red/target/release/red"
else
  RED_BIN="${CRATE_ROOT}/red/target/debug/red"
fi

if [[ ! -x "${RED_BIN}" ]]; then
  echo "[error] red binary not found at ${RED_BIN}" >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
cleanup() { rm -rf "${WORKDIR}" || true; }
trap cleanup EXIT

# Create a bin directory with sed -> red symlink
mkdir -p "${WORKDIR}/bin"
ln -s "${RED_BIN}" "${WORKDIR}/bin/sed"

# Set up test framework using gnulib's init.sh
# GNU sed tests expect ./testsuite/init.sh and init.cfg relative to test location
TESTS_PARENT="$(dirname "${TESTS_DIR}")"

# Also create sed/ directory in tests parent (tests use path_prepend_ ./sed)
mkdir -p "${TESTS_PARENT}/sed"
ln -sf "${RED_BIN}" "${TESTS_PARENT}/sed/sed"
GNULIB_INIT_SH="${TESTS_PARENT}/gnulib/tests/init.sh"
INIT_CFG="${TESTS_PARENT}/init.cfg"
TESTSUITE_INIT_SH="${TESTS_PARENT}/testsuite/init.sh"

# Check if gnulib is available
if [[ ! -f "${GNULIB_INIT_SH}" ]]; then
  echo "[error] gnulib/tests/init.sh not found at: ${GNULIB_INIT_SH}" >&2
  echo "[error] Run 'scripts/fetch_gnused_tests.sh' to download tests and gnulib" >&2
  exit 1
fi

# Create symlink to gnulib's init.sh in testsuite directory
echo "[info] Using gnulib test framework from: ${GNULIB_INIT_SH}"
mkdir -p "$(dirname "${TESTSUITE_INIT_SH}")"
ln -sf "../gnulib/tests/init.sh" "${TESTSUITE_INIT_SH}" 2>/dev/null || {
  # If symlink fails (e.g., on some systems), copy the file
  cp "${GNULIB_INIT_SH}" "${TESTSUITE_INIT_SH}"
}

# Ensure init.cfg exists (it's sourced by init.sh)
if [[ -f "${INIT_CFG}" ]]; then
  echo "[info] Using existing init.cfg"
else
  echo "[info] init.cfg not found - tests will use defaults"
fi

# Update PATH to use our sed wrapper and testsuite helper scripts
export PATH="${WORKDIR}/bin:${TESTS_DIR}:${PATH}"
echo "[info] Using sed: $(command -v sed) ($(${WORKDIR}/bin/sed --version 2>/dev/null | head -1 || echo 'unknown version'))"

# Set up environment variables expected by GNU sed tests
# srcdir=. because tests run from gnused-tests parent directory
export srcdir="."
export abs_top_srcdir="${TESTS_PARENT}"
export abs_srcdir="${TESTS_DIR}"
export LC_ALL=C

# Locale variables for tests (normally set by configure)
export LOCALE_JA="ja_JP.eucjp"

# Variables for help-version.sh test
# Extract version from our red binary
RED_VERSION=$(${WORKDIR}/bin/sed --version 2>/dev/null | head -1 | sed 's/.* //' || echo "unknown")
export VERSION="${RED_VERSION}"
export built_programs="sed"

# Enable expensive tests if requested
if [[ "${EXPENSIVE}" == "true" ]]; then
  export RUN_EXPENSIVE_TESTS=yes
  export RUN_VERY_EXPENSIVE_TESTS=yes
fi

# Store logs
LOGDIR="${WORKDIR}/logs"
mkdir -p "${LOGDIR}"

# Change to parent of tests directory (gnulib expects srcdir=. and testsuite/ subdirectory)
pushd "${TESTS_PARENT}" >/dev/null

# Find all test scripts matching pattern (in testsuite subdirectory)
# Note: using while read loop instead of mapfile for bash 3.2 compatibility (macOS)
TEST_SCRIPTS=()
while IFS= read -r script; do
  TEST_SCRIPTS+=("$script")
done < <(find testsuite -maxdepth 1 -name "${TEST_PATTERN}" -type f | sort)

if [[ ${#TEST_SCRIPTS[@]} -eq 0 ]]; then
  echo "[error] No test scripts found matching pattern: ${TEST_PATTERN}" >&2
  exit 1
fi

echo "[info] Found ${#TEST_SCRIPTS[@]} test scripts matching '${TEST_PATTERN}'"
[[ "${COMPACT}" != "true" ]] && echo ""

# Run tests
PASSED=0
FAILED=0
SKIPPED=0
TIMEOUT=0

for test_script in "${TEST_SCRIPTS[@]}"; do
  test_name="$(basename "${test_script}")"
  test_log="${LOGDIR}/${test_name}.log"

  # panic-tests.sh requires a filesystem that enforces directory permissions
  # VirtioFS (used in some container/VM setups) doesn't enforce dir write permissions
  # So we copy the test directory to /tmp (which uses overlay fs) and run from there
  if [[ "${test_name}" == "panic-tests.sh" ]]; then
    [[ "${COMPACT}" != "true" ]] && echo "[test] Running ${test_name} (from /tmp for permission enforcement)..."
    TMP_TEST_DIR=$(mktemp -d)
    cp -r "${TESTS_PARENT}"/* "${TMP_TEST_DIR}/"
    mkdir -p "${TMP_TEST_DIR}/sed"
    ln -sf "${RED_BIN}" "${TMP_TEST_DIR}/sed/sed"

    set +e
    pushd "${TMP_TEST_DIR}" >/dev/null
    if [[ "${TIMEOUT_SEC}" != "0" && -n "$(command -v timeout)" ]]; then
      timeout "${TIMEOUT_SEC}"s bash "testsuite/${test_name}" >"${test_log}" 2>&1
      rc=$?
    else
      bash "testsuite/${test_name}" >"${test_log}" 2>&1
      rc=$?
    fi
    popd >/dev/null
    set -e
    rm -rf "${TMP_TEST_DIR}"

    if [[ ${rc} -eq 0 ]]; then
      PASSED=$((PASSED + 1))
      [[ "${COMPACT}" == "true" ]] && echo "  [PASS]"
      [[ "${COMPACT}" != "true" ]] && echo "  [PASS]"
    elif [[ ${rc} -eq 77 ]]; then
      SKIPPED=$((SKIPPED + 1))
      skip_msg=$(grep -E "(^SKIP:|skipped test:)" "${test_log}" | head -1 || true)
      if [[ -z "${skip_msg}" ]]; then
        skip_msg="(no message)"
      else
        # Extract just the reason part using bash string manipulation (avoid sed/locale issues)
        if [[ "${skip_msg}" == *"skipped test: "* ]]; then
          skip_msg="${skip_msg##*skipped test: }"
        elif [[ "${skip_msg}" == "SKIP: "* ]]; then
          skip_msg="${skip_msg#SKIP: }"
        fi
      fi
      [[ "${COMPACT}" == "true" ]] && echo "  [SKIP]: ${skip_msg}"
      [[ "${COMPACT}" != "true" ]] && echo "  [SKIP]: ${skip_msg}"
    else
      FAILED=$((FAILED + 1))
      [[ "${COMPACT}" == "true" ]] && echo "  [FAIL] (rc=${rc})"
      [[ "${COMPACT}" != "true" ]] && { echo "  [FAIL] (rc=${rc})"; echo "  Log: ${test_log}"; head -50 "${test_log}"; }
    fi
    continue
  fi

  # Skip tests requiring special handling (documented limitations)
  # Note: 8bit.sh, newjis.sh, 8to7.sh, and mac-mf.sh now pass after byte-level implementation
  case "${test_name}" in
    # mac-mf.sh now passes after fixing raw bytes tracking and address regex parsing
    binary.sh)
      # binary.sh passes in release but too slow in debug mode
      if [[ "${BUILD_MODE}" == "debug" ]]; then
        SKIPPED=$((SKIPPED + 1))
        [[ "${COMPACT}" == "true" ]] && echo "SKIP ${test_name}: Too slow in debug mode - use release build"
        [[ "${COMPACT}" != "true" ]] && echo "[test] Skipping ${test_name} (too slow in debug mode)"
        continue
      fi
      ;;
    # dc.sh now works after fixing greedy matching and ^ literal handling
    # Keeping commented out for reference:
    # dc.sh)
    #   SKIPPED=$((SKIPPED + 1))
    #   [[ "${COMPACT}" == "true" ]] && echo "SKIP ${test_name}: Complex sed script (calculator)"
    #   [[ "${COMPACT}" != "true" ]] && echo "[test] Skipping ${test_name} (complex calculator script)"
    #   continue
    #   ;;
    # bsd-wrapper.sh now passes - all BSD tests working
    #bsd-wrapper.sh)
    #  SKIPPED=$((SKIPPED + 1))
    #  [[ "${COMPACT}" == "true" ]] && echo "SKIP ${test_name}: BSD sed compatibility layer N/A"
    #  [[ "${COMPACT}" != "true" ]] && echo "[test] Skipping ${test_name} (BSD wrapper not applicable)"
    #  continue
    #  ;;
  esac

  [[ "${COMPACT}" != "true" ]] && echo "[test] Running ${test_name}..."

  set +e
  if [[ "${TIMEOUT_SEC}" != "0" && -n "$(command -v timeout)" ]]; then
    timeout "${TIMEOUT_SEC}"s bash "${test_script}" >"${test_log}" 2>&1
    rc=$?
  else
    bash "${test_script}" >"${test_log}" 2>&1
    rc=$?
  fi
  set -e
  
  # Analyze result
  # 0 = success, 77 = skipped, 99 = framework failure, 124 = timeout
  if [[ ${rc} -eq 0 ]]; then
    PASSED=$((PASSED + 1))
    [[ "${COMPACT}" != "true" ]] && echo "  [PASS]"
  elif [[ ${rc} -eq 77 ]]; then
    SKIPPED=$((SKIPPED + 1))
    # Handle both "SKIP: ..." and "testname: skipped test: ..." formats
    skip_msg=$(grep -E "(^SKIP:|skipped test:)" "${test_log}" | head -1 || true)
    if [[ -z "${skip_msg}" ]]; then
      skip_msg="(no message)"
    else
      # Extract just the reason part using bash string manipulation (avoid sed/locale issues)
      if [[ "${skip_msg}" == *"skipped test: "* ]]; then
        skip_msg="${skip_msg##*skipped test: }"
      elif [[ "${skip_msg}" == "SKIP: "* ]]; then
        skip_msg="${skip_msg#SKIP: }"
      fi
    fi
    if [[ "${COMPACT}" == "true" ]]; then
      echo "SKIP ${test_name}: ${skip_msg}"
    else
      echo "  [SKIP]: ${skip_msg}"
    fi
  elif [[ ${rc} -eq 124 ]]; then
    TIMEOUT=$((TIMEOUT + 1))
    FAILED=$((FAILED + 1))
    if [[ "${COMPACT}" == "true" ]]; then
      echo "TIMEOUT ${test_name}"
    else
      echo "  [TIMEOUT] (${TIMEOUT_SEC}s)"
    fi
  else
    FAILED=$((FAILED + 1))
    if [[ "${COMPACT}" == "true" ]]; then
      echo "FAIL ${test_name} (exit ${rc})"
      # Show last few lines of output
      tail -10 "${test_log}" | sed 's/^/  | /'
    else
      echo "  [FAIL] (exit ${rc})"
      echo "  Log: ${test_log}"
      # Show error output
      tail -20 "${test_log}" | sed 's/^/  | /'
    fi
  fi
  
  [[ "${COMPACT}" != "true" ]] && echo ""
done

popd >/dev/null

# Summary
echo ""
echo "=========================================="
echo "Test Results Summary"
echo "=========================================="
echo "Total:   $((PASSED + FAILED + SKIPPED))"
echo "Passed:  ${PASSED}"
echo "Failed:  ${FAILED}"
echo "Skipped: ${SKIPPED}"
if [[ ${TIMEOUT} -gt 0 ]]; then
  echo "Timeout: ${TIMEOUT}"
fi
echo ""
echo "Logs saved at: ${LOGDIR}"
echo "=========================================="

# Exit status
if [[ "${FAIL_ON_ERROR}" == "true" && ${FAILED} -gt 0 ]]; then
  echo "[error] ${FAILED} test(s) failed"
  exit 1
fi

if [[ ${FAILED} -gt 0 ]]; then
  echo "[warn] ${FAILED} test(s) failed (use --fail-on-error to make this fatal)"
  exit 0
else
  echo "[info] All tests passed!"
  exit 0
fi
