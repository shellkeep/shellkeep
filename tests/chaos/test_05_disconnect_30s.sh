#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 05: 30-second disconnect + restore
#
# Blocks SSH for 30 seconds via iptables, then restores and verifies:
# - tmux sessions survive the full 30s outage
# - After restore, all sessions reconnectable within 10s
# - Session content and state are intact
# - Processes running in sessions continued during outage
#
# GUI verification required (manual):
# - Reconnecting indicator appears within a few seconds of disconnect
# - All tabs reconnected within 10s of network restore
# - FR-CONN-16: "Reconnecting..." phase indicator shown

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="05-disconnect-30s"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 5: 30s Disconnect + Restore"

# Create 3 tmux sessions (simulate 3 tabs).
for i in 1 2 3; do
  chaos_tmux_create "chaos--default--tab-${i}"
  chaos_tmux_set_env "chaos--default--tab-${i}" "SHELLKEEP_SESSION_UUID" "uuid-chaos-05-${i}"
done

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-05-1", "name": "tab-1"},
    {"uuid": "uuid-chaos-05-2", "name": "tab-2"},
    {"uuid": "uuid-chaos-05-3", "name": "tab-3"}
  ]
}'

# Start background processes to verify continuity.
# Write a counter script to avoid escaping issues with nested shells.
chaos_docker_exec "cat > /home/testuser/counter.sh << 'COUNTEREOF'
#!/bin/bash
for i in \$(seq 1 120); do echo count_\$i; sleep 1; done
COUNTEREOF
chmod +x /home/testuser/counter.sh
chown testuser:testuser /home/testuser/counter.sh"
chaos_tmux_send_keys "chaos--default--tab-1" "bash ~/counter.sh"
sleep 3

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Blocking SSH for 30 seconds ..."
disconnect_start=$(date +%s)
chaos_block_port 22

# Verify disconnect is effective.
assert_fail "SSH blocked immediately" chaos_ssh_probe 3

# Wait 30 seconds.
chaos_log "Waiting 30 seconds ..."
sleep 30

# ---- Verify during outage (server-side only) ------------------------------ #

chaos_section "Verifying server state after 30s outage"

# All sessions must be alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "All 3 sessions survived 30s outage" "$session_count" "3"

# Counter process should have advanced ~30 iterations.
# Capture with scrollback to find counter output that may have scrolled off screen.
output=$(chaos_docker_exec "su - testuser -c \"tmux capture-pane -t 'chaos--default--tab-1' -p -S -200\"")
# After ~33s (3s setup + 30s wait), counter should be past count_20.
# Check for any count in the 20-39 range to prove the process ran during the outage.
if echo "$output" | grep -qE "count_(2[0-9]|3[0-9])"; then
  chaos_pass "Background process continued during 30s outage"
else
  chaos_fail "Background process may have stalled (last counts: $(echo "$output" | grep -o 'count_[0-9]*' | tail -5 | tr '\n' ' '))"
fi

# ---- Restore network ----------------------------------------------------- #

chaos_log "Restoring network ..."
chaos_unblock_all
restore_start=$(date +%s)

# Measure time to reconnect.
reconnect_retries=20
reconnect_ok=false
for i in $(seq 1 $reconnect_retries); do
  if chaos_ssh_probe 3; then
    reconnect_ok=true
    break
  fi
  sleep 0.5
done

reconnect_end=$(date +%s)
reconnect_time=$((reconnect_end - restore_start))
chaos_log "Reconnection took ${reconnect_time}s"

if $reconnect_ok; then
  chaos_pass "SSH reconnected after restore"
else
  chaos_fail "SSH did not reconnect within ${reconnect_retries} attempts"
fi

assert_num_le "Reconnection within 10s of restore" "$reconnect_time" 10

# ---- Post-restore verification -------------------------------------------- #

chaos_section "Verifying post-restore state"

# All sessions alive and interactive.
for i in 1 2 3; do
  assert_ok "Session tab-${i} exists after restore" chaos_tmux_has_session "chaos--default--tab-${i}"
done

# Interactive test on each session.
for i in 2 3; do
  chaos_tmux_send_keys "chaos--default--tab-${i}" "echo RESTORED_TAB_${i}"
  sleep 2
  output=$(chaos_tmux_capture "chaos--default--tab-${i}")
  assert_contains "Tab-${i} interactive after 30s restore" "$output" "RESTORED_TAB_${i}"
done

# UUIDs preserved.
for i in 1 2 3; do
  uuid=$(chaos_tmux_get_env "chaos--default--tab-${i}" "SHELLKEEP_SESSION_UUID")
  assert_eq "UUID for tab-${i} preserved" "$uuid" "uuid-chaos-05-${i}"
done

# State file integrity.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after 30s outage" "$state_status" "VALID"

# No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

chaos_summary
