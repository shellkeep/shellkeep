#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 05: Typing "exit" in terminal closes the tab
#
# 1. Connect to server
# 2. Focus session window
# 3. Type "exit" + Enter in the terminal
# 4. Verify session window closed
# 5. Verify tmux session killed on server

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 05: Exit Closes Tab"

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

initial_session_count=$(window_count "root@127.0.0.1")
gui_log "Initial session window count: $initial_session_count"

initial_tmux=$(sk_tmux_count)
gui_log "Initial tmux sessions: $initial_tmux"

screenshot "05_connected"

# ---- Step 2: Type "exit" in the terminal -------------------------------- #

gui_section "Typing exit in terminal"

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.5

# Type exit in the terminal shell
type_text "exit"
sleep 0.3
press_enter
sleep 3  # Wait for shell to exit, tmux to detect, and cleanup
screenshot "05_after_exit"

# ---- Step 3: Verify session window closed -------------------------------- #

gui_section "Verifying session closed"

# FR-TABS-22: when last tab in a window exits, the window closes
session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "0" "Session window closed after exit"

# ---- Step 4: Verify tmux session killed ---------------------------------- #

tmux_count=$(sk_tmux_count)
assert_eq "$tmux_count" "0" "tmux session killed on server after exit"

screenshot "05_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
