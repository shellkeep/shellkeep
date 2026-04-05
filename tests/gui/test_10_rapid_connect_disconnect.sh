#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 10: Rapid connect/disconnect cycles don't lose tabs
#
# 1. Connect to server, verify initial tab count
# 2. Stop shellkeep
# 3. Repeat 5 times: start, connect, wait 2s, stop
# 4. Final start, connect, verify tab count unchanged

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 10: Rapid Connect/Disconnect"

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

connect_and_wait_short() {
  local timeout="${1:-5}"
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
  wait_for_window "root@127.0.0.1" 15 || true
  sleep "$timeout"
}

# ---- Setup ---------------------------------------------------------------- #

cleanup_server_state
start_xvfb
start_shellkeep

# ---- Step 1: Initial connect --------------------------------------------- #

gui_section "Initial connection"
connect_and_wait_short 5
screenshot "10_initial"

initial_tabs=$(sk_tab_count)
gui_log "Initial tab count: $initial_tabs"
assert_gt "$initial_tabs" "0" "Initial connection created tabs"

stop_shellkeep
sleep 1

# ---- Step 2: Rapid cycles ----------------------------------------------- #

gui_section "Rapid connect/disconnect cycles"

for i in $(seq 1 5); do
  gui_log "Cycle $i/5"
  start_shellkeep
  connect_and_wait_short 2
  screenshot "10_cycle_${i}"
  stop_shellkeep
  sleep 1
done

# ---- Step 3: Final verify ------------------------------------------------ #

gui_section "Final verification"

start_shellkeep
connect_and_wait_short 5
screenshot "10_final_connect"

final_tabs=$(sk_tab_count)
gui_log "Final tab count: $final_tabs"

assert_eq "$final_tabs" "$initial_tabs" "Tab count unchanged after rapid cycles (no tabs lost)"

screenshot "10_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
