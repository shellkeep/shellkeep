#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Run all shellkeep network chaos test scenarios.
#
# Usage:
#   ./tests/chaos/run_all.sh                    # Run all scenarios
#   ./tests/chaos/run_all.sh high_latency       # Run a specific scenario
#   ./tests/chaos/run_all.sh --list             # List available scenarios
#   ./tests/chaos/run_all.sh --skip-long        # Skip 5-minute disconnect test
#
# Prerequisites:
#   - Docker daemon running with NET_ADMIN-capable containers
#   - sshpass installed (apt install sshpass)
#   - Root or docker group membership for iptables/tc inside containers
#
# Environment variables:
#   CHAOS_IMAGE        Docker image name (default: shellkeep-chaos-sshd)
#   CHAOS_SSH_HOST     SSH host (default: 127.0.0.1)
#   CHAOS_SSH_USER     SSH user (default: testuser)
#   CHAOS_SSH_PASS     SSH password (default: testpass)
#
# NOTE: Scenario 06 (5-minute disconnect) takes ~5.5 minutes. Use --skip-long
# to exclude it from the suite run.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# All test scenarios in execution order.
declare -a ALL_SCENARIOS=(
  "test_01_high_latency.sh"
  "test_02_packet_loss_10.sh"
  "test_03_packet_loss_30.sh"
  "test_04_abrupt_disconnect.sh"
  "test_05_disconnect_30s.sh"
  "test_06_disconnect_5min.sh"
  "test_07_bandwidth_56k.sh"
  "test_08_ip_change.sh"
  "test_09_gradual_latency.sh"
  "test_10_intermittent.sh"
)

# Long-running scenarios (excluded with --skip-long).
declare -A LONG_SCENARIOS=(
  ["test_06_disconnect_5min.sh"]=1
)

# Map short names to filenames.
declare -A SCENARIO_MAP=(
  [high_latency]="test_01_high_latency.sh"
  [loss_10]="test_02_packet_loss_10.sh"
  [loss_30]="test_03_packet_loss_30.sh"
  [abrupt_disconnect]="test_04_abrupt_disconnect.sh"
  [disconnect_30s]="test_05_disconnect_30s.sh"
  [disconnect_5min]="test_06_disconnect_5min.sh"
  [bandwidth_56k]="test_07_bandwidth_56k.sh"
  [ip_change]="test_08_ip_change.sh"
  [gradual_latency]="test_09_gradual_latency.sh"
  [intermittent]="test_10_intermittent.sh"
)

# ---- Functions ------------------------------------------------------------ #

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS] [SCENARIO...]

Run shellkeep network chaos test scenarios.

Options:
  --list, -l        List available scenarios
  --skip-long       Skip scenarios that take > 2 minutes (currently: disconnect_5min)
  --help, -h        Show this help message

Scenarios (run all if none specified):
  high_latency      300ms delay + 50ms jitter
  loss_10           10% packet loss
  loss_30           30% packet loss
  abrupt_disconnect iptables DROP on SSH port
  disconnect_30s    30-second network outage
  disconnect_5min   5-minute network outage (~5.5 min runtime)
  bandwidth_56k     56kbit/s bandwidth cap
  ip_change         IP address change simulation
  gradual_latency   Latency ramp from 50ms to 500ms
  intermittent      5s up / 2s down flapping loop

Examples:
  $(basename "$0")                         # Run all scenarios
  $(basename "$0") high_latency loss_10    # Run two specific scenarios
  $(basename "$0") --skip-long             # Run all except 5-min disconnect
EOF
}

list_scenarios() {
  echo "Available chaos test scenarios:"
  echo ""
  echo "  01. high_latency       - 300ms delay + 50ms jitter (~30s)"
  echo "  02. loss_10            - 10% packet loss (~30s)"
  echo "  03. loss_30            - 30% packet loss (~45s)"
  echo "  04. abrupt_disconnect  - iptables DROP on SSH (~20s)"
  echo "  05. disconnect_30s     - 30-second outage (~50s)"
  echo "  06. disconnect_5min    - 5-minute outage (~5.5 min)"
  echo "  07. bandwidth_56k      - 56kbit/s bandwidth cap (~30s)"
  echo "  08. ip_change          - IP address change simulation (~20s)"
  echo "  09. gradual_latency    - Latency ramp 50->500ms (~90s)"
  echo "  10. intermittent       - 5s up / 2s down flapping (~80s)"
  echo ""
  echo "Total estimated runtime (all): ~12 minutes"
  echo "Total estimated runtime (--skip-long): ~6.5 minutes"
}

check_prereqs() {
  local missing=0
  for tool in docker sshpass ssh timeout; do
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
  name=$(basename "$script" .sh | sed 's/test_[0-9]*_//')

  echo ""
  echo -e "${BOLD}${CYAN}======================================================${NC}"
  echo -e "${BOLD}${CYAN}  Chaos Scenario: ${name}${NC}"
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

SCENARIOS_TO_RUN=()
SKIP_LONG=false

for arg in "$@"; do
  case "$arg" in
    --list|-l)
      list_scenarios
      exit 0
      ;;
    --skip-long)
      SKIP_LONG=true
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
  for scenario in "${ALL_SCENARIOS[@]}"; do
    if $SKIP_LONG && [[ -n "${LONG_SCENARIOS[$scenario]+x}" ]]; then
      echo -e "${YELLOW}Skipping long scenario: $(basename "$scenario" .sh)${NC}"
      continue
    fi
    SCENARIOS_TO_RUN+=("$scenario")
  done
fi

# Pre-flight checks.
check_prereqs

echo -e "${BOLD}${CYAN}shellkeep network chaos test suite${NC}"
echo -e "Running ${#SCENARIOS_TO_RUN[@]} scenario(s)..."
echo ""

# Build the Docker image once.
IMAGE="${CHAOS_IMAGE:-shellkeep-chaos-sshd}"
if [[ -f "$SCRIPT_DIR/Dockerfile" ]]; then
  if ! docker image inspect "$IMAGE" &>/dev/null; then
    echo -e "${CYAN}Building Docker image ${IMAGE}...${NC}"
    docker build -t "$IMAGE" "$SCRIPT_DIR"
  else
    echo -e "${CYAN}Docker image ${IMAGE} already exists.${NC}"
  fi
fi

# Run scenarios.
TOTAL=0
PASSED=0
FAILED=0
SKIPPED=0
FAILED_NAMES=()

suite_start=$(date +%s)

for scenario in "${SCENARIOS_TO_RUN[@]}"; do
  ((TOTAL++)) || true
  if run_scenario "$scenario"; then
    ((PASSED++)) || true
  else
    ((FAILED++)) || true
    name=$(basename "$scenario" .sh | sed 's/test_[0-9]*_//')
    FAILED_NAMES+=("$name")
  fi
done

suite_end=$(date +%s)
suite_duration=$((suite_end - suite_start))

# ---- Final summary -------------------------------------------------------- #

echo ""
echo -e "${BOLD}${CYAN}======================================================${NC}"
echo -e "${BOLD}${CYAN}  Chaos Suite Summary${NC}"
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
  echo -e "  ${GREEN}All chaos scenarios passed.${NC}"
  echo ""
  exit 0
fi
