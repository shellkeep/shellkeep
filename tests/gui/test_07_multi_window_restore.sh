#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 07: Multi-window restore after restart
#
# 1. Connect, create second window
# 2. Verify 2 session windows and 2 server_window_ids
# 3. Stop shellkeep
# 4. Restart, reconnect
# 5. Verify 2 session windows restored
# 6. Verify server_window_ids preserved

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 07: Multi-Window Restore"

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

sk_window_id_list() {
  local state
  state=$(server_shared_state)
  echo "$state" | jq -r '[.workspaces[].tabs[].server_window_id // empty] | unique | .[]' 2>/dev/null || true
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

# ---- Step 1: Connect and create second window ---------------------------- #

gui_section "First run: connect and create 2 windows"
connect_and_wait

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

press_key "ctrl+shift+n"
sleep 5  # Wait for second window + new tab connection
screenshot "07_two_windows"

# ---- Step 2: Verify 2 windows ------------------------------------------- #

gui_section "Verifying 2 windows (first run)"

wait_window_count "root@127.0.0.1" 2 10 || true
session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "2" "2 session windows exist (first run)"

unique_ids=$(sk_window_id_count)
assert_eq "$unique_ids" "2" "2 distinct server_window_ids (first run)"

# Save the window IDs for comparison
original_ids=$(sk_window_id_list | sort)
gui_log "Original window IDs: $original_ids"

# ---- Step 3: Stop shellkeep --------------------------------------------- #

gui_section "Stopping shellkeep"
stop_shellkeep
sleep 1

# ---- Step 4: Restart and reconnect -------------------------------------- #

gui_section "Second run: restart and reconnect"
start_shellkeep
connect_and_wait
screenshot "07_reconnected"

# ---- Step 5: Verify 2 windows restored ---------------------------------- #

gui_section "Verifying 2 windows restored"

wait_window_count "root@127.0.0.1" 2 10 || true
session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "2" "2 session windows restored after restart"

# ---- Step 6: Verify window IDs preserved -------------------------------- #

restored_ids=$(sk_window_id_list | sort)
gui_log "Restored window IDs: $restored_ids"

assert_eq "$restored_ids" "$original_ids" "server_window_ids preserved after restart"

unique_ids=$(sk_window_id_count)
assert_eq "$unique_ids" "2" "2 distinct server_window_ids still present"

screenshot "07_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
