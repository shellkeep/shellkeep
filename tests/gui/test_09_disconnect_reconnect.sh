#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 09: Disconnect and reconnect preserves tabs and names
#
# 1. Connect, create second tab, rename it "Tab-B"
# 2. Verify shared.json has 2 tabs, one named "Tab-B"
# 3. Stop shellkeep (simulates disconnect)
# 4. Restart, reconnect
# 5. Verify 2 tabs restored and "Tab-B" persists

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 09: Disconnect Reconnect"

# ---- Helpers -------------------------------------------------------------- #

sk_tab_count() {
  local state
  state=$(server_shared_state)
  if [[ "$state" == "{}" ]]; then
    echo "0"
  else
    echo "$state" | jq '[.workspaces[].tabs | length] | add // 0' 2>/dev/null || echo "0"
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

# ---- Step 1: Connect, new tab, rename ----------------------------------- #

gui_section "First run: connect, create tab, rename"
connect_and_wait

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

# Create second tab
press_key "ctrl+shift+t"
sleep 5  # Wait for new tab connection
screenshot "09_two_tabs"

# Rename current tab to "Tab-B"
press_key "F2"
sleep 0.5
type_text "Tab-B"
sleep 0.3
press_enter
sleep 3  # Wait for state sync
screenshot "09_renamed"

# ---- Step 2: Verify state ------------------------------------------------ #

gui_section "Verifying state (first run)"

tab_count=$(sk_tab_count)
assert_eq "$tab_count" "2" "shared.json has 2 tabs"

state=$(server_shared_state)
assert_contains "$state" "Tab-B" "shared.json contains 'Tab-B'"

# ---- Step 3: Stop shellkeep (disconnect) --------------------------------- #

gui_section "Stopping shellkeep (disconnect)"
stop_shellkeep
sleep 1

# ---- Step 4: Restart and reconnect -------------------------------------- #

gui_section "Second run: reconnect"
start_shellkeep
connect_and_wait
screenshot "09_reconnected"

# ---- Step 5: Verify tabs restored ---------------------------------------- #

gui_section "Verifying tabs restored"

tab_count=$(sk_tab_count)
assert_eq "$tab_count" "2" "2 tabs restored after reconnect"

state=$(server_shared_state)
assert_contains "$state" "Tab-B" "Tab-B name persisted after reconnect"

screenshot "09_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
