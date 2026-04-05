#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 01: Connect to server and verify tab creation
#
# 1. Start shellkeep
# 2. Wait for control window
# 3. Type "root@127.0.0.1" and press Enter
# 4. Wait for session window
# 5. Verify tmux session exists
# 6. Verify shared.json has tabs

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 01: Connect and Tab"

# ---- Helpers -------------------------------------------------------------- #

# Count shellkeep tmux sessions (actual prefix is shellkeep--)
sk_tmux_count() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null \
    | grep -c '^shellkeep--' || echo "0"
}

sk_tab_count() {
  local state
  state=$(server_shared_state)
  if [[ "$state" == "{}" ]]; then
    echo "0"
  else
    echo "$state" | jq '[.workspaces[].tabs | length] | add // 0' 2>/dev/null || echo "0"
  fi
}

# ---- Setup ---------------------------------------------------------------- #

cleanup_server_state
start_xvfb
start_shellkeep

# ---- Step 1: Wait for control window -------------------------------------- #

gui_section "Connecting to server"

control_wid=$(get_window_ids "^shellkeep$" | head -1)
if [[ -z "$control_wid" ]]; then
  wait_for_window "^shellkeep$" 10
  control_wid=$(get_window_ids "^shellkeep$" | head -1)
fi

focus_window "$control_wid"
sleep 0.5
screenshot "01_control_window"

# ---- Step 2: Type server address and connect ------------------------------ #

click_at 450 350
sleep 0.3
type_text "root@127.0.0.1"
sleep 0.3
screenshot "01_typed_address"
press_enter

# ---- Step 3: Wait for session window -------------------------------------- #

gui_log "Waiting for session window..."
wait_for_window "root@127.0.0.1" 15
sleep 5  # Wait for SSH handshake + tmux setup + state sync
screenshot "01_connected"

# ---- Step 4: Verify tmux session exists ----------------------------------- #

gui_section "Verifying server state"

tmux_count=$(sk_tmux_count)
assert_gt "$tmux_count" "0" "At least 1 tmux session exists"

# ---- Step 5: Verify shared.json exists and has tabs ----------------------- #

state=$(server_shared_state)
assert_true $([[ "$state" != "{}" ]]; echo $?) "shared.json exists and is non-empty"

tab_count=$(sk_tab_count)
assert_gt "$tab_count" "0" "shared.json has at least 1 tab"

screenshot "01_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
