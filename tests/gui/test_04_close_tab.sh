#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 04: Close a tab via Ctrl+Shift+W
#
# 1. Connect, create second tab
# 2. Verify 2 tmux sessions
# 3. Close current tab (Ctrl+Shift+W)
# 4. Confirm termination in dialog
# 5. Verify 1 tmux session remaining

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 04: Close Tab"

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

# ---- Step 1: Connect and create second tab ------------------------------- #

gui_section "Connecting and creating second tab"
connect_and_wait

session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3

press_key "ctrl+shift+t"
sleep 5  # Wait for new tab connection
screenshot "04_two_tabs"

# ---- Step 2: Verify 2 tmux sessions ------------------------------------- #

gui_section "Verifying 2 sessions"

tmux_count=$(sk_tmux_count)
assert_eq "$tmux_count" "2" "2 tmux sessions exist"

tab_count=$(sk_tab_count)
assert_eq "$tab_count" "2" "shared.json has 2 tabs"

# ---- Step 3: Close current tab ------------------------------------------- #

gui_section "Closing tab"

focus_window "$session_wid"
sleep 0.3
press_key "ctrl+shift+w"
sleep 2  # Wait for confirmation dialog
screenshot "04_confirm_dialog"

# Press Enter or Tab+Enter to confirm "Terminate" button
# The dialog has "Cancel" and "Terminate" -- Terminate should be focusable
press_key "Tab"
sleep 0.2
press_enter
sleep 3  # Wait for kill + state sync
screenshot "04_after_close"

# ---- Step 4: Verify 1 session remaining --------------------------------- #

gui_section "Verifying 1 session remaining"

tmux_count=$(sk_tmux_count)
assert_eq "$tmux_count" "1" "1 tmux session remaining"

tab_count=$(sk_tab_count)
assert_eq "$tab_count" "1" "shared.json has 1 tab"

screenshot "04_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
