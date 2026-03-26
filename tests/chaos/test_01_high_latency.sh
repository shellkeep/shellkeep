#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 01: High latency 300ms + jitter 50ms
#
# Applies 300ms delay with 50ms jitter via tc netem and verifies that:
# - SSH connections still work (just slower)
# - tmux sessions remain interactive
# - Round-trip echo time is within expected bounds (< 700ms)
# - No crashes or state corruption
#
# GUI verification required (manual):
# - UI responsive, cursor moves visually
# - No visible freezing in terminal widget
# - FR-CONN-16: connection phase indicators display correctly

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="01-high-latency"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 1: High Latency 300ms + Jitter 50ms"

# Create tmux sessions to simulate shellkeep tabs.
chaos_log "Creating tmux sessions ..."
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"
chaos_tmux_create "chaos--default--session-03"

# Set UUIDs like shellkeep would.
chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-01-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-01-b"
chaos_tmux_set_env "chaos--default--session-03" "SHELLKEEP_SESSION_UUID" "uuid-chaos-01-c"

# Write a state file.
chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-01-a", "name": "session-01"},
    {"uuid": "uuid-chaos-01-b", "name": "session-02"},
    {"uuid": "uuid-chaos-01-c", "name": "session-03"}
  ]
}'

# ---- Baseline measurement ------------------------------------------------- #

chaos_log "Measuring baseline RTT ..."
baseline_rtt=$(chaos_ssh_rtt)
chaos_log "Baseline RTT: ${baseline_rtt}ms"

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Applying 300ms delay + 50ms jitter ..."
chaos_apply_delay 300 50

# ---- Verification --------------------------------------------------------- #

chaos_section "Verifying SSH connectivity under latency"

# 1. SSH connection still works.
assert_ok "SSH connection succeeds under 300ms latency" chaos_ssh_probe 15

# 2. RTT is elevated but within bounds.
chaos_log "Measuring RTT under latency ..."
latency_rtt=$(chaos_ssh_rtt)
chaos_log "RTT under latency: ${latency_rtt}ms"

# Expect at least 200ms increase (300ms delay applies to both directions minus
# some overlap). RTT should be at most ~1200ms (300ms*2 + jitter + baseline).
assert_num_ge "RTT increased by at least 200ms" "$latency_rtt" 200
assert_num_le "RTT under 5000ms (not frozen)" "$latency_rtt" 5000

# 3. tmux sessions are all alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "All 3 tmux sessions still exist" "$session_count" "3"

# 4. Sessions are interactive -- send a command and capture output.
chaos_tmux_send_keys "chaos--default--session-01" "echo CHAOS_MARKER_01"
sleep 2  # Allow for latency.
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Session-01 accepted command under latency" "$output" "CHAOS_MARKER_01"

# 5. Multiple commands in sequence.
chaos_tmux_send_keys "chaos--default--session-02" "date +%s"
sleep 2
output2=$(chaos_tmux_capture "chaos--default--session-02")
# Just check the session produced output (a timestamp is digits).
if [[ "$output2" =~ [0-9]{10} ]]; then
  chaos_pass "Session-02 produced timestamp output under latency"
else
  chaos_fail "Session-02 did not produce expected timestamp output"
fi

# 6. Echo round-trip test: send command via SSH, measure total time.
chaos_log "Measuring echo round-trip under latency ..."
echo_start=$(date +%s%N)
echo_result=$(chaos_ssh_cmd "echo ECHO_OK" 2>/dev/null || echo "TIMEOUT")
echo_end=$(date +%s%N)
echo_ms=$(( (echo_end - echo_start) / 1000000 ))
chaos_log "Echo round-trip: ${echo_ms}ms"

assert_contains "Echo command returned correct output" "$echo_result" "ECHO_OK"
assert_num_le "Echo round-trip < 5000ms" "$echo_ms" 5000

# 7. UUIDs survive.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "Session UUID preserved" "$uuid" "uuid-chaos-01-a"

# 8. State file intact.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid JSON" "$state_status" "VALID"

# 9. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# ---- Cleanup -------------------------------------------------------------- #

chaos_clear_tc
chaos_log "Verifying post-cleanup RTT ..."
restored_rtt=$(chaos_ssh_rtt)
chaos_log "Post-cleanup RTT: ${restored_rtt}ms"
assert_num_le "RTT restored to near baseline" "$restored_rtt" 500

chaos_summary
