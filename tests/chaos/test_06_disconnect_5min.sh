#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 06: 5-minute disconnect + restore
#
# Blocks SSH for 5 minutes via iptables, then restores and verifies:
# - tmux sessions survive the extended outage
# - Reconnection works after 5 minutes
# - Session content and state are intact
# - sshd keepalive timers may have killed SSH channels but tmux persists
#
# NOTE: This test takes approximately 5.5 minutes to complete.
#
# GUI verification required (manual):
# - Reconnecting indicator stays visible during entire outage
# - UI does not crash or freeze during extended disconnect
# - All tabs reconnect after restore

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="06-disconnect-5min"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 6: 5-minute Disconnect + Restore"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-06-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-06-b"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-06-a", "name": "session-01"},
    {"uuid": "uuid-chaos-06-b", "name": "session-02"}
  ]
}'

# Start a timestamp logger to prove the session was active throughout.
chaos_tmux_send_keys "chaos--default--session-01" "while true; do date '+%H:%M:%S alive'; sleep 10; done"
sleep 2

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Blocking SSH for 5 minutes ..."
chaos_block_port 22

# Confirm disconnect.
assert_fail "SSH blocked" chaos_ssh_probe 3

# Wait 5 minutes.
chaos_log "Waiting 5 minutes (300 seconds) ..."
# Check server-side state at intervals to confirm tmux survival.
for minute in 1 2 3 4 5; do
  sleep 60
  chaos_log "Minute ${minute}/5 elapsed ..."
  count=$(chaos_tmux_count_direct)
  chaos_log "  tmux sessions: $count"
done

# ---- Verify during outage ------------------------------------------------- #

chaos_section "Verifying server state after 5-minute outage"

session_count=$(chaos_tmux_count_direct)
assert_eq "Both sessions survived 5-minute outage" "$session_count" "2"

# Verify the logger was running throughout.
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Session produced output during 5min outage" "$output" "alive"

# ---- Restore network ----------------------------------------------------- #

chaos_log "Restoring network ..."
chaos_unblock_all
sleep 3

# Reconnection test.
reconnect_ok=false
for i in $(seq 1 20); do
  if chaos_ssh_probe 5; then
    reconnect_ok=true
    break
  fi
  sleep 1
done

if $reconnect_ok; then
  chaos_pass "SSH reconnects after 5-minute outage"
else
  chaos_fail "SSH did not reconnect after 5-minute outage"
fi

# ---- Post-restore verification -------------------------------------------- #

chaos_section "Verifying post-restore state"

# Sessions alive and interactive.
for sess in "chaos--default--session-01" "chaos--default--session-02"; do
  assert_ok "Session $sess exists after 5min restore" chaos_tmux_has_session "$sess"
done

chaos_tmux_send_keys "chaos--default--session-02" "echo FIVE_MIN_RESTORE_OK"
sleep 2
output2=$(chaos_tmux_capture "chaos--default--session-02")
assert_contains "Session interactive after 5min restore" "$output2" "FIVE_MIN_RESTORE_OK"

# UUIDs preserved.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved through 5min outage" "$uuid" "uuid-chaos-06-a"

# State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after 5min outage" "$state_status" "VALID"

# No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# Kill the logger.
chaos_tmux_send_keys "chaos--default--session-01" "C-c"
sleep 1

chaos_summary
