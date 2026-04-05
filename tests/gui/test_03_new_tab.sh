#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 03: Create a new tab via Ctrl+Shift+T
#
# 1. Connect to server
# 2. Get initial tmux count
# 3. Press Ctrl+Shift+T
# 4. Wait for new tab connection + state sync
# 5. Verify tmux count increased by 1
# 6. Verify shared.json has 2 tabs

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 03: New Tab"

# ---- Helpers -------------------------------------------------------------- #

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
screenshot "03_connected"

# ---- Step 2: Get initial counts ------------------------------------------ #

initial_tmux=$(sk_tmux_count)
initial_tabs=$(sk_tab_count)
gui_log "Initial tmux sessions: $initial_tmux, tabs: $initial_tabs"

# ---- Step 3: Create new tab ---------------------------------------------- #

gui_section "Creating new tab"

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

press_key "ctrl+shift+t"
sleep 5  # Wait for new tab SSH connection + state sync
screenshot "03_new_tab"

# ---- Step 4: Verify counts increased ------------------------------------- #

gui_section "Verifying new tab"

new_tmux=$(sk_tmux_count)
new_tabs=$(sk_tab_count)
gui_log "After new tab - tmux sessions: $new_tmux, tabs: $new_tabs"

expected_tmux=$((initial_tmux + 1))
assert_eq "$new_tmux" "$expected_tmux" "tmux count increased by 1"
assert_eq "$new_tabs" "2" "shared.json has 2 tabs"

screenshot "03_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
