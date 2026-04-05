#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 13: Workspace creation and verification
#
# 1. Connect to server
# 2. Verify shared.json has a workspace (auto-created "Default")
# 3. Verify workspace has tabs
# 4. Verify tmux sessions exist

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 13: Workspace Create"

# ---- Helpers -------------------------------------------------------------- #

sk_tmux_count() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null \
    | grep -c '^shellkeep--' || echo "0"
}

sk_workspace_count() {
  local state
  state=$(server_shared_state)
  if [[ "$state" == "{}" ]]; then
    echo "0"
  else
    echo "$state" | jq '.workspaces | length' 2>/dev/null || echo "0"
  fi
}

sk_workspace_names() {
  local state
  state=$(server_shared_state)
  echo "$state" | jq -r '.workspaces | keys[]' 2>/dev/null || true
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
screenshot "13_connected"

# ---- Step 2: Verify workspace exists ------------------------------------- #

gui_section "Verifying workspace"

state=$(server_shared_state)
assert_true $([[ "$state" != "{}" ]]; echo $?) "shared.json exists and is non-empty"

workspace_count=$(sk_workspace_count)
assert_gt "$workspace_count" "0" "shared.json has at least 1 workspace"

# FR-ENV-05: auto-create "Default" workspace on first connection
workspace_names=$(sk_workspace_names)
gui_log "Workspace names: $workspace_names"
assert_contains "$workspace_names" "Default" "Default workspace exists"

# ---- Step 3: Verify workspace has tabs ----------------------------------- #

gui_section "Verifying workspace has tabs"

# Check that the Default workspace has tabs
tab_count=$(echo "$state" | jq '.workspaces["Default"].tabs | length' 2>/dev/null || echo "0")
assert_gt "$tab_count" "0" "Default workspace has tabs"

# ---- Step 4: Verify tmux sessions exist ---------------------------------- #

tmux_count=$(sk_tmux_count)
assert_gt "$tmux_count" "0" "tmux sessions exist on server"

# Verify tmux session names contain workspace UUID
workspace_uuid=$(echo "$state" | jq -r '.workspaces["Default"].uuid' 2>/dev/null || true)
if [[ -n "$workspace_uuid" && "$workspace_uuid" != "null" ]]; then
  gui_log "Workspace UUID: $workspace_uuid"
  tmux_sessions=$(tmux list-sessions -F '#{session_name}' 2>/dev/null | grep '^shellkeep--' || true)
  assert_contains "$tmux_sessions" "$workspace_uuid" "tmux session names contain workspace UUID"
fi

screenshot "13_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
