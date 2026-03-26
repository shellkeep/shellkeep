#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Run all shellkeep multi-client isolation tests.
#
# Usage:
#   ./tests/multi-client/run_all.sh                    # Run all tests
#   ./tests/multi-client/run_all.sh simultaneous       # Run a specific test
#   ./tests/multi-client/run_all.sh --list             # List available tests
#
# Prerequisites:
#   - Docker daemon running
#   - sshpass installed (apt install sshpass)
#   - Sufficient permissions for Docker operations
#
# Environment variables:
#   E2E_IMAGE        Docker image name (default: shellkeep-test-sshd)
#   E2E_SSH_HOST     SSH host (default: 127.0.0.1)
#   E2E_SSH_USER     SSH user (default: testuser)
#   E2E_SSH_PASS     SSH password (default: testpass)
#   MC_KEEPALIVE_TIMEOUT  Keepalive timeout in seconds (default: 45)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# All available test scripts in execution order.
declare -a ALL_TESTS=(
  "test_01_simultaneous_clients.sh"
  "test_02_client_id_conflict.sh"
  "test_03_orphan_lock_takeover.sh"
  "test_04_environment_isolation.sh"
)

# Map short names to filenames.
declare -A TEST_MAP=(
  [simultaneous]="test_01_simultaneous_clients.sh"
  [conflict]="test_02_client_id_conflict.sh"
  [orphan]="test_03_orphan_lock_takeover.sh"
  [environment]="test_04_environment_isolation.sh"
)

# ---- Functions ------------------------------------------------------------ #

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS] [TEST...]

Run shellkeep multi-client isolation tests.

Options:
  --list, -l    List available tests
  --help, -h    Show this help message

Tests (run all if none specified):
  simultaneous   Two client-IDs simultaneously (desktop + laptop)
  conflict       Client-ID conflict detection and force-takeover
  orphan         Orphan lock auto-takeover after kill -9
  environment    Environment isolation between clients

Examples:
  $(basename "$0")                       # Run all tests
  $(basename "$0") simultaneous          # Run one test
  $(basename "$0") conflict orphan       # Run two tests
EOF
}

list_tests() {
  echo "Available multi-client tests:"
  echo ""
  echo "  simultaneous  - Two client-IDs (desktop + laptop) with separate sessions, state, locks"
  echo "  conflict      - Client-ID conflict detection with force-takeover"
  echo "  orphan        - Orphan lock auto-takeover (simulated kill -9, heartbeat expiry)"
  echo "  environment   - Independent environments per client-id"
}

check_prereqs() {
  local missing=0
  for tool in docker sshpass ssh; do
    if ! command -v "$tool" &>/dev/null; then
      echo -e "${RED}Missing required tool: $tool${NC}"
      ((missing++)) || true
    fi
  done

  if ! docker info &>/dev/null 2>&1; then
    echo -e "${RED}Docker daemon not running or insufficient permissions${NC}"
    ((missing++)) || true
  fi

  if [[ "$missing" -gt 0 ]]; then
    echo ""
    echo "Install missing tools and ensure Docker is running."
    exit 1
  fi
}

run_test() {
  local script="$1"
  local name
  name=$(basename "$script" .sh | sed 's/test_[0-9]*_//')

  echo ""
  echo -e "${BOLD}${CYAN}======================================================${NC}"
  echo -e "${BOLD}${CYAN}  Multi-Client Test: ${name}${NC}"
  echo -e "${BOLD}${CYAN}======================================================${NC}"
  echo ""

  local start_time
  start_time=$(date +%s)

  local exit_code=0
  bash "$SCRIPT_DIR/$script" || exit_code=$?

  local end_time
  end_time=$(date +%s)
  local duration=$((end_time - start_time))

  echo ""
  if [[ "$exit_code" -eq 0 ]]; then
    echo -e "${GREEN}  Test ${name}: PASSED (${duration}s)${NC}"
  else
    echo -e "${RED}  Test ${name}: FAILED (${duration}s)${NC}"
  fi

  return "$exit_code"
}

# ---- Main ----------------------------------------------------------------- #

# Parse arguments.
TESTS_TO_RUN=()

for arg in "$@"; do
  case "$arg" in
    --list|-l)
      list_tests
      exit 0
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      if [[ -n "${TEST_MAP[$arg]+x}" ]]; then
        TESTS_TO_RUN+=("${TEST_MAP[$arg]}")
      else
        echo -e "${RED}Unknown test: $arg${NC}"
        echo "Use --list to see available tests."
        exit 1
      fi
      ;;
  esac
done

# Default: run all tests.
if [[ ${#TESTS_TO_RUN[@]} -eq 0 ]]; then
  TESTS_TO_RUN=("${ALL_TESTS[@]}")
fi

# Pre-flight checks.
check_prereqs

echo -e "${BOLD}${CYAN}shellkeep multi-client isolation test suite${NC}"
echo -e "Running ${#TESTS_TO_RUN[@]} test(s)..."
echo ""

# Build the Docker image once (shared across tests).
DOCKERFILE_DIR="$SCRIPT_DIR/../integration"
if [[ -f "$DOCKERFILE_DIR/Dockerfile" ]]; then
  IMAGE="${E2E_IMAGE:-shellkeep-test-sshd}"
  if ! docker image inspect "$IMAGE" &>/dev/null; then
    echo -e "${CYAN}Building Docker image ${IMAGE}...${NC}"
    docker build -t "$IMAGE" "$DOCKERFILE_DIR"
  else
    echo -e "${CYAN}Docker image ${IMAGE} already exists.${NC}"
  fi
fi

# Run tests.
TOTAL=0
PASSED=0
FAILED=0
FAILED_NAMES=()

suite_start=$(date +%s)

for test_script in "${TESTS_TO_RUN[@]}"; do
  ((TOTAL++)) || true
  if run_test "$test_script"; then
    ((PASSED++)) || true
  else
    ((FAILED++)) || true
    name=$(basename "$test_script" .sh | sed 's/test_[0-9]*_//')
    FAILED_NAMES+=("$name")
  fi
done

suite_end=$(date +%s)
suite_duration=$((suite_end - suite_start))

# ---- Final summary -------------------------------------------------------- #

echo ""
echo -e "${BOLD}${CYAN}======================================================${NC}"
echo -e "${BOLD}${CYAN}  Multi-Client Suite Summary${NC}"
echo -e "${BOLD}${CYAN}======================================================${NC}"
echo ""
echo -e "  Total:    $TOTAL"
echo -e "  ${GREEN}Passed:${NC}   $PASSED"
echo -e "  ${RED}Failed:${NC}   $FAILED"
echo -e "  Duration: ${suite_duration}s"

if [[ "$FAILED" -gt 0 ]]; then
  echo ""
  echo -e "  ${RED}Failed tests:${NC}"
  for name in "${FAILED_NAMES[@]}"; do
    echo -e "    - $name"
  done
  echo ""
  exit 1
else
  echo ""
  echo -e "  ${GREEN}All multi-client tests passed.${NC}"
  echo ""
  exit 0
fi
