#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 06: Create multiple windows via Ctrl+Shift+N
#
# 1. Connect to server
# 2. Press Ctrl+Shift+N to create second window
# 3. Verify 2 session windows exist
# 4. Verify shared.json has tabs with 2 different server_window_id values

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 06: Multi-Window Create"

# ---- Helpers -------------------------------------------------------------- #

sk_window_id_count() {
  local state
  state=$(server_shared_state)
  if [[ "$state" == "{}" ]]; then
    echo "0"
  else
    echo "$state" | jq '[.workspaces[].tabs[].server_window_id // empty] | unique | length' 2>/dev/null || echo "0"
  fi
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
screenshot "06_connected"

# ---- Step 2: Create second window ---------------------------------------- #

gui_section "Creating second window"

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

press_key "ctrl+shift+n"
sleep 3  # Wait for new window
screenshot "06_second_window"

# ---- Step 3: Verify 2 session windows ------------------------------------ #

gui_section "Verifying 2 session windows"

# Wait a bit for the second window to fully appear
wait_window_count "root@127.0.0.1" 2 10 || true
session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "2" "2 session windows exist"

# Focus each window to verify they are interactive
window_ids=$(get_window_ids "root@127.0.0.1")
window_num=1
while IFS= read -r wid; do
  if [[ -n "$wid" ]]; then
    focus_window "$wid"
    sleep 0.3
    screenshot "06_window_${window_num}"
    gui_log "Window $window_num (wid=$wid) focused successfully"
    ((window_num++)) || true
  fi
done <<< "$window_ids"

# ---- Step 4: Wait for state sync and verify ------------------------------ #

sleep 3  # Wait for state sync
gui_section "Verifying server_window_id values"

unique_ids=$(sk_window_id_count)
assert_eq "$unique_ids" "2" "shared.json has 2 different server_window_id values"

screenshot "06_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
