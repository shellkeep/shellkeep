#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 08: Closing session window hides sessions (FR-TABS-17)
#
# 1. Connect to server
# 2. Get session window ID
# 3. Close session window via xdotool windowclose
# 4. Verify tmux session still alive on server
# 5. Verify no session windows visible (only control window)

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 08: Window Close Hides"

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

# ---- Step 1: Connect ----------------------------------------------------- #

gui_section "Connecting to server"
connect_and_wait

initial_tmux=$(sk_tmux_count)
gui_log "Initial tmux sessions: $initial_tmux"
assert_gt "$initial_tmux" "0" "tmux sessions exist after connect"

screenshot "08_connected"

# ---- Step 2: Close session window ---------------------------------------- #

gui_section "Closing session window via windowclose"

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
gui_log "Closing session window wid=$session_wid"
xdotool windowclose "$session_wid" 2>/dev/null || true
sleep 2  # Wait for the window to close and state to update
screenshot "08_after_close"

# ---- Step 3: Verify tmux session still alive ----------------------------- #

gui_section "Verifying tmux session still alive"

tmux_count=$(sk_tmux_count)
assert_eq "$tmux_count" "$initial_tmux" "tmux sessions still alive (hidden, not killed)"

# ---- Step 4: Verify no session windows visible --------------------------- #

session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "0" "No session windows visible"

# Verify control window still exists
control_count=$(window_count "^shellkeep$")
assert_gt "$control_count" "0" "Control window still visible"

screenshot "08_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
