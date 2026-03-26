#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Run all first-run-experience tests (Agent 28, Wave 9).
#
# Tests the 6 first-use combinations from the WAVE-09 spec:
#
#   Scenario 1: Happy path — SSH key + agent + tmux + empty known_hosts
#   Scenario 2: No tmux — SSH key + agent + NO tmux + empty known_hosts
#   Scenario 3: No key  — NO SSH key + NO agent + tmux + empty known_hosts
#   Scenario 4: Old tmux — SSH key + agent + tmux 2.x + empty known_hosts
#   Scenario 5: Nothing — NO SSH key + NO agent + NO tmux + empty known_hosts
#   Scenario 6: Returning — SSH key + agent + tmux + populated known_hosts + state
#
# Usage:
#   ./tests/first-run/run_all.sh            # Run all scenarios
#   ./tests/first-run/run_all.sh 1 3 6      # Run specific scenarios
#
# Prerequisites:
#   - Docker
#   - sshpass
#   - ssh, ssh-keygen, ssh-keyscan, ssh-agent, ssh-add
#
# Exit code:
#   0 if all scenarios pass, 1 if any scenario fails.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---- Color output --------------------------------------------------------- #

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ---- Configuration ------------------------------------------------------- #

ALL_SCENARIOS=(1 2 3 4 5 6)
SCENARIOS_TO_RUN=()

if [[ $# -gt 0 ]]; then
  SCENARIOS_TO_RUN=("$@")
else
  SCENARIOS_TO_RUN=("${ALL_SCENARIOS[@]}")
fi

TOTAL_PASS=0
TOTAL_FAIL=0
TOTAL_SKIP=0
FAILED_SCENARIOS=()

# ---- Banner -------------------------------------------------------------- #

echo ""
echo -e "${CYAN}================================================================${NC}"
echo -e "${CYAN}  shellkeep — First-Run Experience Tests (Agent 28, Wave 9)${NC}"
echo -e "${CYAN}================================================================${NC}"
echo ""
echo "Scenarios to run: ${SCENARIOS_TO_RUN[*]}"
echo ""

# ---- Prerequisite check -------------------------------------------------- #

echo -e "${CYAN}Checking prerequisites...${NC}"

missing=0
for tool in docker sshpass ssh ssh-keygen ssh-keyscan ssh-agent ssh-add; do
  if ! command -v "$tool" &>/dev/null; then
    echo -e "  ${RED}MISSING${NC}: $tool"
    ((missing++)) || true
  fi
done

if [[ "$missing" -gt 0 ]]; then
  echo ""
  echo -e "${RED}Missing $missing required tools. Install them and retry.${NC}"
  exit 1
fi

echo -e "  ${GREEN}All prerequisites met.${NC}"
echo ""

# ---- Run scenarios -------------------------------------------------------- #

for scenario in "${SCENARIOS_TO_RUN[@]}"; do
  script="$SCRIPT_DIR/test_scenario_${scenario}.sh"

  if [[ ! -f "$script" ]]; then
    echo -e "${RED}Scenario $scenario: script not found ($script)${NC}"
    FAILED_SCENARIOS+=("$scenario")
    ((TOTAL_FAIL++)) || true
    continue
  fi

  echo -e "${CYAN}================================================================${NC}"
  echo -e "${CYAN}  Running Scenario $scenario${NC}"
  echo -e "${CYAN}================================================================${NC}"

  set +e
  bash "$script"
  rc=$?
  set -e

  if [[ $rc -eq 0 ]]; then
    echo -e "${GREEN}  Scenario $scenario: PASSED${NC}"
    ((TOTAL_PASS++)) || true
  else
    echo -e "${RED}  Scenario $scenario: FAILED${NC}"
    ((TOTAL_FAIL++)) || true
    FAILED_SCENARIOS+=("$scenario")
  fi

  echo ""
done

# ---- Summary -------------------------------------------------------------- #

echo -e "${CYAN}================================================================${NC}"
echo -e "${CYAN}  First-Run Experience Test Summary${NC}"
echo -e "${CYAN}================================================================${NC}"
echo ""
echo -e "  Scenarios run:    ${#SCENARIOS_TO_RUN[@]}"
echo -e "  ${GREEN}Passed:${NC}           $TOTAL_PASS"
echo -e "  ${RED}Failed:${NC}           $TOTAL_FAIL"

if [[ ${#FAILED_SCENARIOS[@]} -gt 0 ]]; then
  echo ""
  echo -e "  ${RED}Failed scenarios: ${FAILED_SCENARIOS[*]}${NC}"
fi

echo ""
echo -e "${CYAN}--- Items requiring manual verification ---${NC}"
echo ""
echo "  The following behaviors require a display server (GTK) and"
echo "  cannot be verified in headless mode:"
echo ""
echo "  Scenario 1 (happy path):"
echo "    - TOFU dialog shows fingerprint and accept/reject buttons"
echo "    - Connection progress feedback shows all phases"
echo "    - Default environment created automatically"
echo ""
echo "  Scenario 2 (no tmux):"
echo "    - Dialog shows 'tmux is required' with install instructions"
echo "    - Instructions cover apt, dnf, pacman, brew"
echo ""
echo "  Scenario 3 (no key):"
echo "    - Auth failure dialog with guidance about SSH keys"
echo "    - Retry button is present"
echo ""
echo "  Scenario 4 (old tmux):"
echo "    - Warning shows found vs. minimum tmux version"
echo "    - User can proceed or cancel"
echo ""
echo "  Scenario 5 (nothing works):"
echo "    - Auth failure is clear and actionable"
echo "    - No blank screen or hang"
echo ""
echo "  Scenario 6 (returning user):"
echo "    - No TOFU dialog (host already known)"
echo "    - No environment selection dialog (single env)"
echo "    - Tabs restored with correct names"
echo "    - Window restored to saved geometry"
echo ""
echo -e "${CYAN}================================================================${NC}"

if [[ "$TOTAL_FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
