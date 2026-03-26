#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 09: Gradual latency increase 50ms -> 500ms
#
# Incrementally increases network latency from 50ms to 500ms over 60 seconds
# and verifies that:
# - SSH connections remain stable throughout the ramp
# - tmux sessions continue working at each latency level
# - No crash at any point during the increase
# - Session state remains consistent
#
# GUI verification required (manual):
# - Yellow/orange latency indicator appears as latency increases
# - No crash or freeze at any latency level
# - Typing remains responsive (progressively slower)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="09-gradual-latency"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 9: Gradual Latency Increase 50ms -> 500ms"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-09-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-09-b"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-09-a", "name": "session-01"},
    {"uuid": "uuid-chaos-09-b", "name": "session-02"}
  ]
}'

# ---- Gradual latency ramp ------------------------------------------------- #

chaos_section "Ramping latency from 50ms to 500ms over 60 seconds"

# Latency steps: 50, 100, 150, 200, 250, 300, 350, 400, 450, 500
# 10 steps, ~6 seconds each = 60 seconds total.
ramp_failures=0
ramp_steps=0

for delay in 50 100 150 200 250 300 350 400 450 500; do
  chaos_log "Setting latency to ${delay}ms ..."
  chaos_docker_exec "tc qdisc del dev eth0 root 2>/dev/null; tc qdisc add dev eth0 root netem delay ${delay}ms 10ms"
  ((ramp_steps++)) || true

  # Allow time for the setting to take effect.
  sleep 2

  # Verify SSH still works at this latency.
  if chaos_ssh_probe 15; then
    chaos_log "  SSH OK at ${delay}ms"
  else
    chaos_log "  SSH FAILED at ${delay}ms"
    ((ramp_failures++)) || true
  fi

  # Verify tmux sessions alive.
  count=$(chaos_tmux_count_direct)
  if [[ "$count" != "2" ]]; then
    chaos_fail "Sessions lost at ${delay}ms latency (count=$count)"
  fi

  # Interact with a session at this latency level.
  chaos_tmux_send_keys "chaos--default--session-01" "echo latency_${delay}"
  sleep 3  # Wait longer at higher latencies.
done

# ---- Verification at peak latency ---------------------------------------- #

chaos_section "Verifying at peak latency (500ms)"

# 1. SSH connects (slowly) at 500ms.
assert_ok "SSH connects at 500ms latency" chaos_ssh_probe 20

# 2. Sessions alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "Both sessions survive latency ramp" "$session_count" "2"

# 3. Capture output -- should see markers from various latency levels.
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Captured output at 50ms" "$output" "latency_50"
assert_contains "Captured output at 500ms" "$output" "latency_500"

# 4. Most ramp steps succeeded.
chaos_log "Ramp results: ${ramp_failures} failures out of ${ramp_steps} steps"
assert_num_le "At most 2 SSH failures during ramp" "$ramp_failures" 2

# 5. Measure RTT at peak.
rtt=$(chaos_ssh_rtt)
chaos_log "RTT at 500ms latency: ${rtt}ms"
assert_num_ge "RTT reflects high latency" "$rtt" 500
assert_num_le "RTT not unreasonably high" "$rtt" 8000

# 6. UUIDs preserved through entire ramp.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved through latency ramp" "$uuid" "uuid-chaos-09-a"

# 7. State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after latency ramp" "$state_status" "VALID"

# 8. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# ---- Cleanup -------------------------------------------------------------- #

chaos_clear_tc

# Verify latency returns to normal.
post_rtt=$(chaos_ssh_rtt)
chaos_log "Post-cleanup RTT: ${post_rtt}ms"
assert_num_le "Latency restored to normal" "$post_rtt" 500

chaos_summary
