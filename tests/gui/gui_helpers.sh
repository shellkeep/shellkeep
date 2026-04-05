#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Shared helper functions for shellkeep GUI E2E tests.
# Source this file from individual test scripts.
#
# These tests run ON the droplet. The shellkeep binary runs under Xvfb
# and is automated via xdotool.

# Strict mode (sourcing scripts inherit this).
set -euo pipefail

# ---- Color output ----------------------------------------------------------- #

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# ---- Configuration ---------------------------------------------------------- #

# SSH credentials for uploading from the dev container to the droplet.
SSH_KEY="/home/node/.ssh/id_shellkeep"
DROPLET="root@209.38.150.61"
DROPLET_IP="209.38.150.61"

# Paths on the droplet where tests run.
TEST_DIR="/opt/shellkeep-gui-test"
SHELLKEEP_BIN="${TEST_DIR}/shellkeep"
TEST_SCRIPTS_DIR="${TEST_DIR}/tests"
TEST_CONFIG="${TEST_DIR}/tests/test_config.toml"
TEST_SERVERS_JSON="${TEST_DIR}/tests/servers.json"

# Xvfb display.
export DISPLAY="${DISPLAY:-:99}"

# shellkeep process state.
SHELLKEEP_PID=""

# Screenshot output directory.
SCREENSHOT_DIR="${TEST_DIR}/screenshots"

# ---- Test framework --------------------------------------------------------- #

PASS=0
FAIL=0
TOTAL=0

gui_log() {
  echo -e "${CYAN}[gui]${NC} $*"
}

gui_pass() {
  echo -e "  ${GREEN}PASS${NC}: $*"
  ((PASS++)) || true
  ((TOTAL++)) || true
}

gui_fail() {
  echo -e "  ${RED}FAIL${NC}: $*"
  ((FAIL++)) || true
  ((TOTAL++)) || true
}

gui_section() {
  echo ""
  echo -e "${BOLD}${CYAN}--- $* ---${NC}"
}

# ---- Assertions ------------------------------------------------------------- #

# Assert two values are equal.
# Usage: assert_eq <actual> <expected> <message>
assert_eq() {
  local actual="$1"
  local expected="$2"
  local msg="$3"
  if [[ "$actual" == "$expected" ]]; then
    gui_pass "$msg"
  else
    gui_fail "$msg (expected '$expected', got '$actual')"
  fi
}

# Assert haystack contains needle.
# Usage: assert_contains <haystack> <needle> <message>
assert_contains() {
  local haystack="$1"
  local needle="$2"
  local msg="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    gui_pass "$msg"
  else
    gui_fail "$msg (expected '$needle' in output)"
  fi
}

# Assert numeric value is greater than threshold.
# Usage: assert_gt <actual> <threshold> <message>
assert_gt() {
  local actual="$1"
  local threshold="$2"
  local msg="$3"
  if [[ "$actual" -gt "$threshold" ]]; then
    gui_pass "$msg"
  else
    gui_fail "$msg (expected > $threshold, got $actual)"
  fi
}

# Assert a condition (exit code 0 = true).
# Usage: assert_true <exit_code> <message>
assert_true() {
  local result="$1"
  local msg="$2"
  if [[ "$result" -eq 0 ]]; then
    gui_pass "$msg"
  else
    gui_fail "$msg (condition was false)"
  fi
}

# Print test summary and exit with appropriate code.
test_summary() {
  echo ""
  echo -e "${CYAN}========================================${NC}"
  echo -e "  ${GREEN}Passed:${NC}  $PASS"
  echo -e "  ${RED}Failed:${NC}  $FAIL"
  echo -e "  Total:   $TOTAL"
  echo -e "${CYAN}========================================${NC}"

  if [[ "$FAIL" -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

# ---- Remote execution (from dev container to droplet) ----------------------- #

# Run a command on the droplet via SSH.
# Usage: droplet_run <command...>
droplet_run() {
  ssh -i "$SSH_KEY" \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR \
    "$DROPLET" "$@"
}

# Run a command on the droplet in the background.
# Usage: droplet_run_bg <command...>
droplet_run_bg() {
  ssh -i "$SSH_KEY" \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR \
    "$DROPLET" "$@" &
}

# ---- Xvfb management ------------------------------------------------------- #

# Start Xvfb on the configured display if not already running.
start_xvfb() {
  gui_log "Starting Xvfb on display $DISPLAY..."

  # Check if Xvfb is already running on this display.
  if xdpyinfo -display "$DISPLAY" &>/dev/null 2>&1; then
    gui_log "Xvfb already running on $DISPLAY"
    return 0
  fi

  # Start Xvfb with a reasonable screen size.
  Xvfb "$DISPLAY" -screen 0 1280x1024x24 -ac +extension GLX &>/dev/null &
  local xvfb_pid=$!

  # Wait for the display to become available.
  local retries=30
  while ! xdpyinfo -display "$DISPLAY" &>/dev/null 2>&1; do
    ((retries--)) || {
      gui_fail "Xvfb failed to start on $DISPLAY"
      return 1
    }
    sleep 0.3
  done

  gui_log "Xvfb started (PID $xvfb_pid) on $DISPLAY"
}

# Stop Xvfb on the configured display.
stop_xvfb() {
  gui_log "Stopping Xvfb..."
  pkill -f "Xvfb $DISPLAY" 2>/dev/null || true
  sleep 0.5
}

# ---- shellkeep process management ------------------------------------------- #

# Start the shellkeep binary under the Xvfb display.
# Uses the test config and servers.json.
start_shellkeep() {
  gui_log "Starting shellkeep..."

  # Ensure config directory exists and populate it.
  local config_dir="$HOME/.config/shellkeep"
  mkdir -p "$config_dir"

  # Copy test config and servers list.
  if [[ -f "$TEST_CONFIG" ]]; then
    cp "$TEST_CONFIG" "$config_dir/config.toml"
  fi
  if [[ -f "$TEST_SERVERS_JSON" ]]; then
    cp "$TEST_SERVERS_JSON" "$config_dir/servers.json"
  fi

  # Ensure screenshot directory exists.
  mkdir -p "$SCREENSHOT_DIR"

  # Start a dbus session if not already running.
  if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
    eval "$(dbus-launch --sh-syntax)" 2>/dev/null || true
  fi

  # Launch shellkeep in the background.
  DISPLAY="$DISPLAY" "$SHELLKEEP_BIN" &>/tmp/shellkeep-gui-test.log &
  SHELLKEEP_PID=$!

  # Wait for the process to be running.
  local retries=50
  while ! kill -0 "$SHELLKEEP_PID" 2>/dev/null; do
    ((retries--)) || {
      gui_fail "shellkeep failed to start"
      cat /tmp/shellkeep-gui-test.log 2>/dev/null || true
      return 1
    }
    sleep 0.2
  done

  gui_log "shellkeep started (PID $SHELLKEEP_PID)"

  # Wait for a window to appear (up to 10s).
  wait_for_window "." 10 || {
    gui_fail "No shellkeep window appeared within 10s"
    cat /tmp/shellkeep-gui-test.log 2>/dev/null || true
    return 1
  }

  gui_log "shellkeep window detected"
}

# Stop the shellkeep process.
stop_shellkeep() {
  if [[ -n "$SHELLKEEP_PID" ]] && kill -0 "$SHELLKEEP_PID" 2>/dev/null; then
    gui_log "Stopping shellkeep (PID $SHELLKEEP_PID)..."
    kill "$SHELLKEEP_PID" 2>/dev/null || true

    # Wait for graceful exit (up to 5s).
    local retries=25
    while kill -0 "$SHELLKEEP_PID" 2>/dev/null; do
      ((retries--)) || {
        gui_log "Force killing shellkeep..."
        kill -9 "$SHELLKEEP_PID" 2>/dev/null || true
        break
      }
      sleep 0.2
    done

    SHELLKEEP_PID=""
    gui_log "shellkeep stopped"
  fi

  # Clean up any stray shellkeep processes.
  pkill -f "$SHELLKEEP_BIN" 2>/dev/null || true
}

# ---- Window management (xdotool) ------------------------------------------- #

# Wait for a window matching the name pattern to appear.
# Usage: wait_for_window <name_pattern> <timeout_seconds>
wait_for_window() {
  local pattern="${1:-.}"
  local timeout="${2:-10}"
  local deadline=$((SECONDS + timeout))

  while [[ $SECONDS -lt $deadline ]]; do
    if xdotool search --name "$pattern" 2>/dev/null | head -1 | grep -q .; then
      return 0
    fi
    sleep 0.3
  done

  return 1
}

# Get window IDs matching a pattern.
# Usage: get_window_ids <pattern>
get_window_ids() {
  local pattern="${1:-.}"
  xdotool search --name "$pattern" 2>/dev/null || true
}

# Count windows matching a pattern.
# Usage: window_count <pattern>
window_count() {
  local pattern="${1:-.}"
  local ids
  ids=$(xdotool search --name "$pattern" 2>/dev/null || true)
  if [[ -z "$ids" ]]; then
    echo "0"
  else
    echo "$ids" | wc -l | tr -d ' '
  fi
}

# Wait until the number of windows matching a pattern reaches the expected count.
# Usage: wait_window_count <pattern> <expected_count> <timeout_seconds>
wait_window_count() {
  local pattern="$1"
  local expected="$2"
  local timeout="${3:-10}"
  local deadline=$((SECONDS + timeout))

  while [[ $SECONDS -lt $deadline ]]; do
    local count
    count=$(window_count "$pattern")
    if [[ "$count" -eq "$expected" ]]; then
      return 0
    fi
    sleep 0.3
  done

  return 1
}

# ---- GUI automation (xdotool) ----------------------------------------------- #

# Focus a window by its window ID.
# Usage: focus_window <wid>
focus_window() {
  local wid="${1:?window id required}"
  xdotool windowactivate --sync "$wid" 2>/dev/null || true
  xdotool windowfocus --sync "$wid" 2>/dev/null || true
  sleep 0.2
}

# Type text with a small inter-key delay.
# Usage: type_text <text>
type_text() {
  local text="${1:?text required}"
  xdotool type --delay 30 "$text"
  sleep 0.1
}

# Press a key combination.
# Usage: press_key <key_combo>  (e.g., "ctrl+shift+t", "Return", "F2")
press_key() {
  local key="${1:?key required}"
  xdotool key "$key"
  sleep 0.2
}

# Press Enter.
press_enter() {
  press_key "Return"
}

# Click at screen coordinates.
# Usage: click_at <x> <y>
click_at() {
  local x="${1:?x required}"
  local y="${2:?y required}"
  xdotool mousemove "$x" "$y"
  sleep 0.1
  xdotool click 1
  sleep 0.2
}

# Right-click at screen coordinates.
# Usage: right_click_at <x> <y>
right_click_at() {
  local x="${1:?x required}"
  local y="${2:?y required}"
  xdotool mousemove "$x" "$y"
  sleep 0.1
  xdotool click 3
  sleep 0.2
}

# ---- Server state verification ---------------------------------------------- #

# Count shellkeep tmux sessions on the server (excluding lock sessions).
server_tmux_count() {
  local count
  count=$(tmux list-sessions -F '#{session_name}' 2>/dev/null \
    | grep -c '^sk-' || echo "0")
  echo "$count"
}

# List shellkeep tmux session names.
server_tmux_list() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null \
    | grep '^sk-' || true
}

# Read the shared.json state file.
server_shared_state() {
  local state_dir="$HOME/.shellkeep"
  if [[ -f "$state_dir/shared.json" ]]; then
    cat "$state_dir/shared.json"
  else
    echo "{}"
  fi
}

# Count tabs in the current workspace from shared.json.
server_tab_count() {
  local state
  state=$(server_shared_state)
  echo "$state" | jq '[.workspaces[]?.tabs[]?] | length' 2>/dev/null || echo "0"
}

# List unique server_window_id values from shared.json.
server_window_ids() {
  local state
  state=$(server_shared_state)
  echo "$state" | jq -r '[.workspaces[]?.server_window_id] | unique | .[]' 2>/dev/null || true
}

# ---- Cleanup ---------------------------------------------------------------- #

# Clean up all shellkeep state on the server.
# Kills tmux sessions, removes state directory, kills stray processes.
cleanup_server_state() {
  gui_log "Cleaning up server state..."

  # Kill all shellkeep-related tmux sessions (including locks).
  local sessions
  sessions=$(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
  if [[ -n "$sessions" ]]; then
    while IFS= read -r session; do
      case "$session" in
        sk-*|shellkeep-lock-*)
          tmux kill-session -t "$session" 2>/dev/null || true
          ;;
      esac
    done <<< "$sessions"
  fi

  # Remove shellkeep state directory.
  rm -rf "$HOME/.shellkeep"

  # Remove shellkeep config (test config will be re-created on next start).
  rm -rf "$HOME/.config/shellkeep"

  # Kill any running shellkeep processes.
  pkill -f "$SHELLKEEP_BIN" 2>/dev/null || true

  gui_log "Server state cleaned"
}

# ---- Screenshots ------------------------------------------------------------ #

# Capture a screenshot of the Xvfb display for debugging.
# Usage: screenshot <name>
screenshot() {
  local name="${1:-screenshot}"
  local timestamp
  timestamp=$(date +%Y%m%d_%H%M%S)
  local path="${SCREENSHOT_DIR}/${name}_${timestamp}.png"

  mkdir -p "$SCREENSHOT_DIR"
  import -window root -display "$DISPLAY" "$path" 2>/dev/null || {
    gui_log "WARNING: screenshot failed (is Xvfb running?)"
    return 1
  }

  gui_log "Screenshot saved: $path"
}

# ---- Lifecycle -------------------------------------------------------------- #

# Standard test setup: clean state, start Xvfb, start shellkeep.
gui_test_setup() {
  cleanup_server_state
  start_xvfb
  start_shellkeep
}

# Standard test teardown: stop shellkeep, clean state.
gui_test_teardown() {
  # Take a screenshot before teardown for debugging failures.
  screenshot "teardown" 2>/dev/null || true
  stop_shellkeep
  cleanup_server_state
}

# Register teardown on EXIT.
gui_register_cleanup() {
  trap 'gui_test_teardown' EXIT
}
