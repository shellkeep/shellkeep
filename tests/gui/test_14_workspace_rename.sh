#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 14: Workspace state persistence across restarts
#
# 1. Connect to server
# 2. Verify shared.json has workspace data
# 3. Stop, restart, reconnect
# 4. Verify workspace data persisted

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 14: Workspace Persistence"

# ---- Helpers -------------------------------------------------------------- #

sk_workspace_count() {
  local state
  state=$(server_shared_state)
  if [[ "$state" == "{}" ]]; then
    echo "0"
  else
    echo "$state" | jq '.workspaces | length' 2>/dev/null || echo "0"
  fi
}

sk_workspace_data() {
  local state
  state=$(server_shared_state)
  echo "$state" | jq -c '.workspaces' 2>/dev/null || echo "{}"
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

# ---- Step 1: First run - connect ---------------------------------------- #

gui_section "First run: connect and verify workspace"
connect_and_wait
screenshot "14_first_run"

# ---- Step 2: Verify workspace data -------------------------------------- #

workspace_count=$(sk_workspace_count)
assert_gt "$workspace_count" "0" "Workspace data exists (first run)"

first_run_data=$(sk_workspace_data)
gui_log "First run workspace data: $first_run_data"

# Get workspace UUID (should be stable)
state=$(server_shared_state)
workspace_uuid=$(echo "$state" | jq -r '.workspaces["Default"].uuid // empty' 2>/dev/null)
gui_log "Workspace UUID: $workspace_uuid"

# Verify workspace has the expected structure
workspace_name=$(echo "$state" | jq -r '.workspaces["Default"].name // empty' 2>/dev/null)
assert_eq "$workspace_name" "Default" "Workspace name is 'Default'"

tab_count=$(echo "$state" | jq '.workspaces["Default"].tabs | length' 2>/dev/null || echo "0")
assert_gt "$tab_count" "0" "Workspace has tabs"

# ---- Step 3: Stop and restart ------------------------------------------- #

gui_section "Stopping and restarting"
stop_shellkeep
sleep 1

start_shellkeep
connect_and_wait
screenshot "14_second_run"

# ---- Step 4: Verify workspace data persisted ----------------------------- #

gui_section "Verifying workspace data persisted"

workspace_count_after=$(sk_workspace_count)
assert_eq "$workspace_count_after" "$workspace_count" "Workspace count unchanged after restart"

# Verify UUID is the same (workspace identity preserved)
state_after=$(server_shared_state)
workspace_uuid_after=$(echo "$state_after" | jq -r '.workspaces["Default"].uuid // empty' 2>/dev/null)
assert_eq "$workspace_uuid_after" "$workspace_uuid" "Workspace UUID preserved after restart"

# Verify workspace name persisted
workspace_name_after=$(echo "$state_after" | jq -r '.workspaces["Default"].name // empty' 2>/dev/null)
assert_eq "$workspace_name_after" "Default" "Workspace name persisted"

# Verify tabs are still there
tab_count_after=$(echo "$state_after" | jq '.workspaces["Default"].tabs | length' 2>/dev/null || echo "0")
assert_gt "$tab_count_after" "0" "Tabs persisted after restart"

screenshot "14_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
