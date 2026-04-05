#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Run all GUI E2E tests sequentially.
# Usage: bash run_all.sh [test_number]
#   No args: run all 15 tests
#   With arg: run only that test (e.g., bash run_all.sh 03)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

TOTAL=0
PASSED=0
FAILED=0
FAILED_TESTS=""

run_test() {
  local script="$1"
  local name
  name=$(basename "$script" .sh)
  echo ""
  echo -e "${CYAN}================================================================${NC}"
  echo -e "${CYAN}  Running: $name${NC}"
  echo -e "${CYAN}================================================================${NC}"

  ((TOTAL++)) || true

  if bash "$script"; then
    echo -e "${GREEN}  >> $name PASSED${NC}"
    ((PASSED++)) || true
  else
    echo -e "${RED}  >> $name FAILED${NC}"
    ((FAILED++)) || true
    FAILED_TESTS="${FAILED_TESTS}  - ${name}\n"
  fi
}

# Determine which tests to run
if [[ $# -gt 0 ]]; then
  # Run specific test by number
  test_num="$1"
  script="${SCRIPT_DIR}/test_${test_num}_*.sh"
  # shellcheck disable=SC2086
  if ls $script &>/dev/null; then
    for f in $script; do
      run_test "$f"
    done
  else
    echo "No test found matching: test_${test_num}_*.sh"
    exit 1
  fi
else
  # Run all tests in order
  for script in "$SCRIPT_DIR"/test_*.sh; do
    run_test "$script"
  done
fi

# Summary
echo ""
echo -e "${CYAN}================================================================${NC}"
echo -e "${CYAN}  GUI E2E Test Suite Summary${NC}"
echo -e "${CYAN}================================================================${NC}"
echo -e "  Total:   $TOTAL"
echo -e "  ${GREEN}Passed:${NC}  $PASSED"
echo -e "  ${RED}Failed:${NC}  $FAILED"

if [[ -n "$FAILED_TESTS" ]]; then
  echo ""
  echo -e "  ${RED}Failed tests:${NC}"
  echo -e "$FAILED_TESTS"
fi

echo -e "${CYAN}================================================================${NC}"

if [[ "$FAILED" -gt 0 ]]; then
  exit 1
fi
exit 0
