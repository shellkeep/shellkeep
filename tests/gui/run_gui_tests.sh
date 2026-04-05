#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Master runner for shellkeep GUI E2E tests.
#
# This script runs from the dev container. It:
#   1. Builds shellkeep (cargo build --release)
#   2. Uploads the binary and test scripts to the droplet
#   3. Runs setup_droplet.sh if needed
#   4. Starts Xvfb on the droplet
#   5. Runs each test_*.sh on the droplet via SSH
#   6. Collects results and prints summary
#
# Usage:
#   ./tests/gui/run_gui_tests.sh              # Run all GUI tests
#   ./tests/gui/run_gui_tests.sh --skip-build # Skip cargo build
#   ./tests/gui/run_gui_tests.sh --help       # Show help
#
# Prerequisites:
#   - SSH key at /home/node/.ssh/id_shellkeep
#   - Droplet accessible at 209.38.150.61
#   - Rust toolchain available

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# ---- Color output ----------------------------------------------------------- #

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# ---- Configuration ---------------------------------------------------------- #

SSH_KEY="/home/node/.ssh/id_shellkeep"
DROPLET="root@209.38.150.61"
SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"
TEST_DIR="/opt/shellkeep-gui-test"

SKIP_BUILD=false

# ---- Functions --------------------------------------------------------------- #

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Run shellkeep GUI E2E tests on the droplet.

Options:
  --skip-build    Skip cargo build (use existing binary)
  --help, -h      Show this help message

The script builds shellkeep, uploads it to the droplet, and runs
GUI tests under Xvfb with xdotool automation.
EOF
}

droplet_ssh() {
  ssh -i "$SSH_KEY" $SSH_OPTS "$DROPLET" "$@"
}

droplet_scp() {
  scp -i "$SSH_KEY" $SSH_OPTS "$@"
}

log() {
  echo -e "${CYAN}[runner]${NC} $*"
}

die() {
  echo -e "${RED}ERROR:${NC} $*" >&2
  exit 1
}

# ---- Parse arguments -------------------------------------------------------- #

for arg in "$@"; do
  case "$arg" in
    --skip-build)
      SKIP_BUILD=true
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      die "Unknown argument: $arg"
      ;;
  esac
done

# ---- Pre-flight checks ------------------------------------------------------ #

log "Pre-flight checks..."

if [[ ! -f "$SSH_KEY" ]]; then
  die "SSH key not found at $SSH_KEY"
fi

if ! ssh -i "$SSH_KEY" $SSH_OPTS -o ConnectTimeout=5 "$DROPLET" "true" 2>/dev/null; then
  die "Cannot connect to droplet at 209.38.150.61"
fi

log "Droplet is reachable"

# ---- Step 1: Build ---------------------------------------------------------- #

if [[ "$SKIP_BUILD" == "false" ]]; then
  log "Building shellkeep (cargo build --release)..."

  cd "$REPO_DIR"
  cargo build --release 2>&1 | tail -5

  if [[ ! -f "$REPO_DIR/target/release/shellkeep" ]]; then
    die "Build failed: target/release/shellkeep not found"
  fi

  log "Build complete"
else
  log "Skipping build (--skip-build)"

  if [[ ! -f "$REPO_DIR/target/release/shellkeep" ]]; then
    die "No binary found at target/release/shellkeep (build first or remove --skip-build)"
  fi
fi

# ---- Step 2: Upload binary and test scripts --------------------------------- #

log "Uploading binary to droplet..."
droplet_ssh "mkdir -p $TEST_DIR"
droplet_scp "$REPO_DIR/target/release/shellkeep" "$DROPLET:$TEST_DIR/shellkeep"
droplet_ssh "chmod +x $TEST_DIR/shellkeep"

log "Uploading test scripts..."
droplet_ssh "mkdir -p $TEST_DIR/tests"
droplet_scp -r "$SCRIPT_DIR/"* "$DROPLET:$TEST_DIR/tests/"
droplet_ssh "chmod +x $TEST_DIR/tests/*.sh 2>/dev/null || true"

log "Upload complete"

# ---- Step 3: Run setup if needed -------------------------------------------- #

log "Checking droplet setup..."

if ! droplet_ssh "command -v xdotool &>/dev/null && command -v Xvfb &>/dev/null"; then
  log "Running setup_droplet.sh..."
  droplet_ssh "bash $TEST_DIR/tests/setup_droplet.sh"
  log "Setup complete"
else
  log "Droplet already set up"
fi

# ---- Step 4: Start Xvfb on droplet ----------------------------------------- #

log "Starting Xvfb on droplet..."

droplet_ssh "
  export DISPLAY=:99
  if xdpyinfo -display :99 &>/dev/null 2>&1; then
    echo 'Xvfb already running on :99'
  else
    Xvfb :99 -screen 0 1280x1024x24 -ac +extension GLX &>/dev/null &
    sleep 1
    if xdpyinfo -display :99 &>/dev/null 2>&1; then
      echo 'Xvfb started on :99'
    else
      echo 'ERROR: Xvfb failed to start' >&2
      exit 1
    fi
  fi
"

# ---- Step 5: Discover and run test scripts ---------------------------------- #

log "Discovering test scripts..."

# Find all test_*.sh scripts on the droplet.
TEST_SCRIPTS=$(droplet_ssh "ls $TEST_DIR/tests/test_*.sh 2>/dev/null" || true)

if [[ -z "$TEST_SCRIPTS" ]]; then
  log "No test scripts found (test_*.sh). Nothing to run."
  echo ""
  echo -e "${YELLOW}No GUI test scripts found. Create test_*.sh files in tests/gui/.${NC}"
  exit 0
fi

TOTAL=0
PASSED=0
FAILED=0
FAILED_NAMES=()

suite_start=$(date +%s)

while IFS= read -r test_script; do
  test_name=$(basename "$test_script" .sh | sed 's/test_//')
  ((TOTAL++)) || true

  echo ""
  echo -e "${BOLD}${CYAN}======================================================${NC}"
  echo -e "${BOLD}${CYAN}  GUI Test: ${test_name}${NC}"
  echo -e "${BOLD}${CYAN}======================================================${NC}"
  echo ""

  local_start=$(date +%s)
  exit_code=0

  # Run the test script on the droplet.
  droplet_ssh "
    export DISPLAY=:99
    export PATH=$TEST_DIR:\$PATH
    cd $TEST_DIR/tests
    bash $test_script
  " || exit_code=$?

  local_end=$(date +%s)
  duration=$((local_end - local_start))

  echo ""
  if [[ "$exit_code" -eq 0 ]]; then
    echo -e "${GREEN}  Test ${test_name}: PASSED (${duration}s)${NC}"
    ((PASSED++)) || true
  else
    echo -e "${RED}  Test ${test_name}: FAILED (${duration}s)${NC}"
    ((FAILED++)) || true
    FAILED_NAMES+=("$test_name")

    # Try to fetch the screenshot for debugging.
    log "Fetching screenshots for failed test..."
    droplet_scp "$DROPLET:$TEST_DIR/screenshots/*" "$REPO_DIR/tests/gui/" 2>/dev/null || true
  fi
done <<< "$TEST_SCRIPTS"

suite_end=$(date +%s)
suite_duration=$((suite_end - suite_start))

# ---- Step 6: Stop Xvfb ------------------------------------------------------ #

log "Stopping Xvfb on droplet..."
droplet_ssh "pkill -f 'Xvfb :99' 2>/dev/null || true"

# ---- Step 7: Summary -------------------------------------------------------- #

echo ""
echo -e "${BOLD}${CYAN}======================================================${NC}"
echo -e "${BOLD}${CYAN}  GUI Test Suite Summary${NC}"
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
  echo -e "  ${GREEN}All GUI tests passed.${NC}"
  echo ""
  exit 0
fi
