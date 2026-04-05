#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 11: Reconnect after SIGKILL (crash simulation)
#
# 1. Connect to server, verify session
# 2. Kill shellkeep with SIGKILL
# 3. Verify tmux sessions still alive
# 4. Restart shellkeep, reconnect
# 5. Verify tabs restored

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 11: Reconnect After Kill"

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

# ---- Step 1: Connect and verify ----------------------------------------- #

gui_section "Connecting to server"
connect_and_wait
screenshot "11_connected"

tmux_before=$(sk_tmux_count)
tabs_before=$(sk_tab_count)
gui_log "Before kill: tmux=$tmux_before, tabs=$tabs_before"
assert_gt "$tmux_before" "0" "tmux sessions exist before kill"

# ---- Step 2: Kill with SIGKILL ------------------------------------------ #

gui_section "Killing shellkeep with SIGKILL"

shellkeep_pid="$SHELLKEEP_PID"
gui_log "Killing PID $shellkeep_pid with SIGKILL"
kill -9 "$shellkeep_pid" 2>/dev/null || true
wait "$shellkeep_pid" 2>/dev/null || true
SHELLKEEP_PID=""
sleep 2
screenshot "11_after_kill"

# ---- Step 3: Verify tmux sessions survived ------------------------------- #

gui_section "Verifying tmux sessions survived crash"

tmux_after_kill=$(sk_tmux_count)
assert_eq "$tmux_after_kill" "$tmux_before" "tmux sessions survived SIGKILL"

# ---- Step 4: Restart and reconnect -------------------------------------- #

gui_section "Restarting and reconnecting"

start_shellkeep
connect_and_wait
screenshot "11_reconnected"

# ---- Step 5: Verify tabs restored ---------------------------------------- #

gui_section "Verifying tabs restored"

# Session window should appear
session_count=$(window_count "root@127.0.0.1")
assert_gt "$session_count" "0" "Session window appeared after reconnect"

tabs_after=$(sk_tab_count)
assert_eq "$tabs_after" "$tabs_before" "Tab count restored after crash"

screenshot "11_final"

# ---- Teardown ------------------------------------------------------------- #

stop_shellkeep

test_summary
