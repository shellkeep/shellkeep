#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# GUI E2E Test 15: Full lifecycle smoke test
#
# 1. Connect to server
# 2. Create second window (Ctrl+Shift+N)
# 3. Create extra tab in each window (Ctrl+Shift+T)
# 4. Rename a tab to "LifecycleTab"
# 5. Verify: 2 windows, multiple tabs, "LifecycleTab" in state
# 6. Stop shellkeep, restart, reconnect
# 7. Verify: 2 windows restored, tabs restored, name persisted
# 8. Type "exit" in a tab, verify it closes
# 9. Final comprehensive summary

source "$(dirname "$0")/gui_helpers.sh"

trap cleanup_server_state EXIT

gui_section "Test 15: Full Lifecycle"

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

sk_window_id_count() {
  local state
  state=$(server_shared_state)
  if [[ "$state" == "{}" ]]; then
    echo "0"
  else
    echo "$state" | jq '[.workspaces[].tabs[].server_window_id // empty] | unique | length' 2>/dev/null || echo "0"
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

# ---- Phase 1: Build up session state ------------------------------------- #

gui_section "Phase 1: Connect and build session state"

connect_and_wait
screenshot "15_phase1_connected"

# Create second window
session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.3
press_key "ctrl+shift+n"
sleep 3
screenshot "15_phase1_two_windows"

# Wait for second window
wait_window_count "root@127.0.0.1" 2 10 || true

# Create extra tab in first window
window_ids=$(get_window_ids "root@127.0.0.1")
first_wid=$(echo "$window_ids" | head -1)
focus_window "$first_wid"
sleep 0.3
press_key "ctrl+shift+t"
sleep 5
screenshot "15_phase1_extra_tab_w1"

# Create extra tab in second window
second_wid=$(echo "$window_ids" | tail -1)
focus_window "$second_wid"
sleep 0.3
press_key "ctrl+shift+t"
sleep 5
screenshot "15_phase1_extra_tab_w2"

# Rename a tab in the first window
focus_window "$first_wid"
sleep 0.3
press_key "F2"
sleep 0.5
type_text "LifecycleTab"
sleep 0.3
press_enter
sleep 3  # State sync
screenshot "15_phase1_renamed"

# ---- Phase 1 verification ------------------------------------------------ #

gui_section "Phase 1 verification"

session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "2" "2 session windows exist"

tab_count=$(sk_tab_count)
assert_gt "$tab_count" "2" "More than 2 tabs in state"
gui_log "Tab count: $tab_count"

tmux_count=$(sk_tmux_count)
assert_gt "$tmux_count" "2" "More than 2 tmux sessions"
gui_log "Tmux session count: $tmux_count"

state=$(server_shared_state)
assert_contains "$state" "LifecycleTab" "State contains 'LifecycleTab'"

unique_windows=$(sk_window_id_count)
assert_eq "$unique_windows" "2" "2 distinct server_window_ids"

# Save counts for comparison after restart
tabs_before_restart=$tab_count
tmux_before_restart=$tmux_count

# ---- Phase 2: Restart and verify restoration ----------------------------- #

gui_section "Phase 2: Restart and verify restoration"

stop_shellkeep
sleep 1

start_shellkeep
connect_and_wait
screenshot "15_phase2_reconnected"

# Wait for all windows to restore
wait_window_count "root@127.0.0.1" 2 10 || true

session_count=$(window_count "root@127.0.0.1")
assert_eq "$session_count" "2" "2 windows restored after restart"

tab_count=$(sk_tab_count)
assert_eq "$tab_count" "$tabs_before_restart" "Tab count preserved after restart"

tmux_count=$(sk_tmux_count)
assert_eq "$tmux_count" "$tmux_before_restart" "Tmux session count preserved"

state=$(server_shared_state)
assert_contains "$state" "LifecycleTab" "'LifecycleTab' name persisted after restart"

screenshot "15_phase2_verified"

# ---- Phase 3: Exit a session --------------------------------------------- #

gui_section "Phase 3: Exit a session"

# Focus a session window and type exit
session_wid=$(get_window_ids "root@127.0.0.1" | head -1)
focus_window "$session_wid"
sleep 0.5

type_text "exit"
sleep 0.3
press_enter
sleep 3  # Wait for shell exit + tmux cleanup + state sync
screenshot "15_phase3_after_exit"

# Verify tmux count decreased
tmux_after_exit=$(sk_tmux_count)
gui_log "Tmux sessions after exit: $tmux_after_exit"
expected_tmux=$((tmux_before_restart - 1))
assert_eq "$tmux_after_exit" "$expected_tmux" "Tmux session killed after exit"

# Verify tab count decreased
tab_after_exit=$(sk_tab_count)
gui_log "Tab count after exit: $tab_after_exit"
expected_tabs=$((tabs_before_restart - 1))
assert_eq "$tab_after_exit" "$expected_tabs" "Tab removed from state after exit"

screenshot "15_phase3_verified"

# ---- Final teardown ------------------------------------------------------- #

gui_section "Phase 4: Clean shutdown"

stop_shellkeep

# ---- Comprehensive summary ------------------------------------------------ #

echo ""
gui_log "=== Full Lifecycle Test Complete ==="
gui_log "  Phase 1: Built 2 windows, multiple tabs, renamed tab"
gui_log "  Phase 2: Restarted and verified full restoration"
gui_log "  Phase 3: Exited a session and verified cleanup"
gui_log "  Phase 4: Clean shutdown"

test_summary
