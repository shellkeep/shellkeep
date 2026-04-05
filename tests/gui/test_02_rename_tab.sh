#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 02: Rename a tab and verify persistence
#
# 1. Connect to server
# 2. Press F2 to rename tab, type "MyTestTab", press Enter
# 3. Verify shared.json contains "MyTestTab"
# 4. Restart shellkeep, reconnect
# 5. Verify "MyTestTab" persists

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 02: Rename Tab"

# ---- Helpers -------------------------------------------------------------- #

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
screenshot "02_connected"

# ---- Step 2: Rename tab -------------------------------------------------- #

gui_section "Renaming tab"

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

press_key "F2"
sleep 0.5
type_text "MyTestTab"
sleep 0.3
press_enter
sleep 3  # Wait for state sync (2s debounce + 1s buffer)
screenshot "02_renamed"

# ---- Step 3: Verify rename in state -------------------------------------- #

gui_section "Verifying rename in state"

state=$(server_shared_state)
assert_contains "$state" "MyTestTab" "shared.json contains 'MyTestTab'"

# ---- Step 4: Restart and verify persistence ------------------------------- #

gui_section "Verifying persistence after restart"

stop_shellkeep
sleep 1

start_shellkeep
connect_and_wait
screenshot "02_reconnected"

# Verify name persisted
state=$(server_shared_state)
assert_contains "$state" "MyTestTab" "shared.json still has 'MyTestTab' after restart"

screenshot "02_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
