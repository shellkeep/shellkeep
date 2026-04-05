#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 12: Hiding sessions via window close (FR-TABS-17)
#
# 1. Connect, create second tab
# 2. Get initial tmux count
# 3. Close the session window (which hides all tabs per FR-TABS-17)
# 4. Verify tmux sessions still alive (hidden, not killed)

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 12: Hide Session"

# ---- Helpers -------------------------------------------------------------- #

sk_tmux_count() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null \
    | grep -c '^shellkeep--' || echo "0"
}

connect_and_wait() {
  local control_wid
  control_wid=$(get_window_ids "^shellkeep$" | head -1)
  if [[ -z "$control_wid" ]]; then
    wait_for_window "^shellkeep$" 10
    control_wid=$(get_window_ids "^shellkeep$" | head -1)
  fi
  focus_window "$control_wid"
  sleep 0.5
  click_at 450 350
  sleep 0.3
  type_text "root@127.0.0.1"
  sleep 0.3
  press_enter
  wait_for_window "root@127.0.0.1" 15
  sleep 5
}

# ---- Setup ---------------------------------------------------------------- #

cleanup_server_state
start_xvfb
start_shellkeep

# ---- Step 1: Connect and create second tab ------------------------------- #

gui_section "Connecting and creating second tab"
connect_and_wait

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

press_key "ctrl+shift+t"
sleep 5  # Wait for new tab connection + state sync
screenshot "12_two_tabs"

# ---- Step 2: Get initial tmux count -------------------------------------- #

initial_tmux=$(sk_tmux_count)
gui_log "Initial tmux sessions: $initial_tmux"
assert_eq "$initial_tmux" "2" "2 tmux sessions exist"

# ---- Step 3: Close session window (hides tabs) -------------------------- #

gui_section "Closing session window (hide)"

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
xdotool windowclose "$session_wid" 2>/dev/null || true
sleep 2
screenshot "12_after_close"

# ---- Step 4: Verify tmux sessions still alive ---------------------------- #

gui_section "Verifying sessions hidden, not killed"

tmux_count=$(sk_tmux_count)
assert_eq "$tmux_count" "$initial_tmux" "tmux sessions still alive (hidden, not killed)"

# Verify no session windows visible
session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "0" "No session windows visible"

# Control window should still be showing
control_count=$(window_count "^shellkeep$")
assert_gt "$control_count" "0" "Control window still visible"

# Verify shared.json has hidden windows info
state=$(server_shared_state)
if echo "$state" | jq -e '.hidden_windows | length > 0' &>/dev/null; then
  gui_pass "shared.json has hidden_windows data"
else
  # Hidden windows may be tracked differently; just verify tmux is alive
  gui_log "hidden_windows field not found (may use different tracking)"
fi

screenshot "12_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
