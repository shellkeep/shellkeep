#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Run all shellkeep e2e test scenarios.
#
# Usage:
#   ./tests/e2e/run_all.sh              # Run all scenarios
#   ./tests/e2e/run_all.sh first_use    # Run a specific scenario
#   ./tests/e2e/run_all.sh --list       # List available scenarios
#
# Prerequisites:
#   - Docker daemon running
#   - sshpass installed (apt install sshpass)
#   - Sufficient permissions for Docker and network operations
#
# Environment variables:
#   E2E_IMAGE        Docker image name (default: shellkeep-test-sshd)
#   E2E_SSH_HOST     SSH host (default: 127.0.0.1)
#   E2E_SSH_USER     SSH user (default: testuser)
#   E2E_SSH_PASS     SSH password (default: testpass)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# All available test scenarios in execution order.
declare -a ALL_SCENARIOS=(
  "test_e2e_first_use.sh"
  "test_e2e_reconnection.sh"
  "test_e2e_dead_session.sh"
  "test_e2e_conflict.sh"
  "test_e2e_environments.sh"
  "test_e2e_multi_client.sh"
)

# Map short names to filenames.
declare -A SCENARIO_MAP=(
  [first_use]="test_e2e_first_use.sh"
  [reconnection]="test_e2e_reconnection.sh"
  [dead_session]="test_e2e_dead_session.sh"
  [conflict]="test_e2e_conflict.sh"
  [environments]="test_e2e_environments.sh"
  [multi_client]="test_e2e_multi_client.sh"
)

# ---- Functions ------------------------------------------------------------ #

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS] [SCENARIO...]

Run shellkeep end-to-end test scenarios.

Options:
  --list, -l    List available scenarios
  --help, -h    Show this help message

Scenarios (run all if none specified):
  first_use      First connection flow, state creation, session management
  reconnection   Disconnect/reconnect with network interruption
  dead_session   Dead session detection after tmux kill
  conflict       Client-ID conflict with two concurrent connections
  environments   Multiple environment management
  multi_client   Multiple client isolation

Examples:
  $(basename "$0")                    # Run all scenarios
  $(basename "$0") first_use          # Run one scenario
  $(basename "$0") conflict dead_session  # Run two scenarios
EOF
}

list_scenarios() {
  echo "Available e2e scenarios:"
  echo ""
  echo "  first_use      - First connection flow, state creation, session management"
  echo "  reconnection   - Disconnect/reconnect with network interruption simulation"
  echo "  dead_session   - Dead session detection after external tmux kill"
  echo "  conflict       - Client-ID conflict detection with concurrent connections"
  echo "  environments   - Multiple environment management and isolation"
  echo "  multi_client   - Multiple client isolation with separate state files"
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

run_scenario() {
  local script="$1"
  local name
  name=$(basename "$script" .sh | sed 's/test_e2e_//')

  echo ""
  echo -e "${BOLD}${CYAN}======================================================${NC}"
  echo -e "${BOLD}${CYAN}  Scenario: ${name}${NC}"
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
    echo -e "${GREEN}  Scenario ${name}: PASSED (${duration}s)${NC}"
  else
    echo -e "${RED}  Scenario ${name}: FAILED (${duration}s)${NC}"
  fi

  return "$exit_code"
}

# ---- Main ----------------------------------------------------------------- #

# Parse arguments.
SCENARIOS_TO_RUN=()

for arg in "$@"; do
  case "$arg" in
    --list|-l)
      list_scenarios
      exit 0
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      if [[ -n "${SCENARIO_MAP[$arg]+x}" ]]; then
        SCENARIOS_TO_RUN+=("${SCENARIO_MAP[$arg]}")
      else
        echo -e "${RED}Unknown scenario: $arg${NC}"
        echo "Use --list to see available scenarios."
        exit 1
      fi
      ;;
  esac
done

# Default: run all scenarios.
if [[ ${#SCENARIOS_TO_RUN[@]} -eq 0 ]]; then
  SCENARIOS_TO_RUN=("${ALL_SCENARIOS[@]}")
fi

# Pre-flight checks.
check_prereqs

echo -e "${BOLD}${CYAN}shellkeep e2e test suite${NC}"
echo -e "Running ${#SCENARIOS_TO_RUN[@]} scenario(s)..."
echo ""

# Build the Docker image once (shared across scenarios).
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

# Run scenarios.
TOTAL=0
PASSED=0
FAILED=0
FAILED_NAMES=()

suite_start=$(date +%s)

for scenario in "${SCENARIOS_TO_RUN[@]}"; do
  ((TOTAL++)) || true
  if run_scenario "$scenario"; then
    ((PASSED++)) || true
  else
    ((FAILED++)) || true
    name=$(basename "$scenario" .sh | sed 's/test_e2e_//')
    FAILED_NAMES+=("$name")
  fi
done

suite_end=$(date +%s)
suite_duration=$((suite_end - suite_start))

# ---- Final summary -------------------------------------------------------- #

echo ""
echo -e "${BOLD}${CYAN}======================================================${NC}"
echo -e "${BOLD}${CYAN}  E2E Suite Summary${NC}"
echo -e "${BOLD}${CYAN}======================================================${NC}"
echo ""
echo -e "  Total:    $TOTAL"
echo -e "  ${GREEN}Passed:${NC}   $PASSED"
echo -e "  ${RED}Failed:${NC}   $FAILED"
echo -e "  Duration: ${suite_duration}s"

if [[ "$FAILED" -gt 0 ]]; then
  echo ""
  echo -e "  ${RED}Failed scenarios:${NC}"
  for name in "${FAILED_NAMES[@]}"; do
    echo -e "    - $name"
  done
  echo ""
  exit 1
else
  echo ""
  echo -e "  ${GREEN}All scenarios passed.${NC}"
  echo ""
  exit 0
fi
