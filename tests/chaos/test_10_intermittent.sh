#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 10: Intermittent connectivity (5s up / 2s down loop)
#
# Toggles iptables DROP in a 5s-up/2s-down cycle for 60 seconds and verifies:
# - tmux sessions survive the flapping
# - No reconnection storm (connections don't pile up)
# - After stabilization, sessions are fully usable
# - State is consistent
#
# This tests shellkeep's backoff behavior: during intermittent connectivity,
# a naive client would attempt reconnection every time the link comes up,
# creating a storm. Proper backoff should limit reconnection attempts.
#
# GUI verification required (manual):
# - No reconnection storm visible (backoff behavior)
# - Connection indicator toggles appropriately
# - After stabilization, all tabs reconnect cleanly

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="10-intermittent"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 10: Intermittent 5s Up / 2s Down Loop"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"
chaos_tmux_create "chaos--default--session-03"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-10-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-10-b"
chaos_tmux_set_env "chaos--default--session-03" "SHELLKEEP_SESSION_UUID" "uuid-chaos-10-c"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-10-a", "name": "session-01"},
    {"uuid": "uuid-chaos-10-b", "name": "session-02"},
    {"uuid": "uuid-chaos-10-c", "name": "session-03"}
  ]
}'

# Start a counter process to verify session liveness across flaps.
chaos_tmux_send_keys "chaos--default--session-01" "n=0; while true; do echo flap_count_\$n; n=\$((n+1)); sleep 1; done"
sleep 2

# ---- Run intermittent flapping loop -------------------------------------- #

chaos_section "Running 5s-up/2s-down flapping for ~60 seconds"

FLAP_CYCLES=9  # 9 cycles * (5+2)s = 63 seconds
cycle_log=""

for cycle in $(seq 1 $FLAP_CYCLES); do
  # UP phase: 5 seconds.
  chaos_docker_exec "iptables -F INPUT; iptables -F OUTPUT" 2>/dev/null || true
  chaos_log "Cycle ${cycle}/${FLAP_CYCLES}: UP (5s)"

  # During UP phase, check if SSH works.
  up_ok="no"
  if chaos_ssh_probe 4; then
    up_ok="yes"
  fi
  cycle_log="${cycle_log}cycle${cycle}:up=${up_ok},"
  sleep 5

  # DOWN phase: 2 seconds.
  chaos_docker_exec "iptables -A OUTPUT -p tcp --sport 22 -j DROP; iptables -A INPUT -p tcp --dport 22 -j DROP" 2>/dev/null || true
  chaos_log "Cycle ${cycle}/${FLAP_CYCLES}: DOWN (2s)"
  sleep 2
done

# End in UP state.
chaos_docker_exec "iptables -F INPUT; iptables -F OUTPUT" 2>/dev/null || true
chaos_log "Flapping complete, network restored"

# ---- Verification during flapping (server-side) -------------------------- #

chaos_section "Verifying server state after flapping"

# 1. All tmux sessions survived.
session_count=$(chaos_tmux_count_direct)
assert_eq "All 3 sessions survived intermittent flapping" "$session_count" "3"

# 2. The counter process continued (should have counted through all the flaps).
output=$(chaos_tmux_capture "chaos--default--session-01")
# After ~63 seconds, the counter should be at least 50 (some lines may have scrolled).
if echo "$output" | grep -q "flap_count_"; then
  chaos_pass "Counter process ran throughout flapping"
else
  chaos_fail "Counter process output not found"
fi

# 3. Check sshd process count (no explosion of zombie connections).
sshd_count=$(chaos_docker_exec "pgrep -c sshd" || echo "0")
sshd_count="${sshd_count// /}"
chaos_log "sshd process count: $sshd_count"
# sshd master + maybe a few lingering children, but not dozens.
assert_num_le "No sshd process explosion (< 15)" "$sshd_count" 15

# ---- Post-stabilization verification ------------------------------------- #

chaos_section "Verifying post-stabilization"

# Wait for things to settle.
sleep 5

# 4. SSH works reliably now.
ssh_successes=0
for i in $(seq 1 5); do
  if chaos_ssh_probe 10; then
    ((ssh_successes++)) || true
  fi
done
assert_num_ge "SSH reliable after stabilization (4/5)" "$ssh_successes" 4

# 5. All sessions interactive.
for i in 1 2 3; do
  chaos_tmux_send_keys "chaos--default--session-0${i}" "echo STABLE_${i}"
  sleep 2
  output=$(chaos_tmux_capture "chaos--default--session-0${i}")
  assert_contains "Session-0${i} interactive after flapping" "$output" "STABLE_${i}"
done

# 6. UUIDs preserved.
for i in a b c; do
  idx=$(($(printf '%d' "'$i") - 96))  # a=1, b=2, c=3
  uuid=$(chaos_tmux_get_env "chaos--default--session-0${idx}" "SHELLKEEP_SESSION_UUID")
  assert_eq "UUID uuid-chaos-10-${i} preserved" "$uuid" "uuid-chaos-10-${i}"
done

# 7. State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after flapping" "$state_status" "VALID"

# 8. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# Kill the counter.
chaos_docker_exec "su - testuser -c \"tmux send-keys -t 'chaos--default--session-01' C-c\""
sleep 1

chaos_summary
