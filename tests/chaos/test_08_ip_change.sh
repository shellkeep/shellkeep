#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 08: IP change simulation
#
# Simulates an IP address change by briefly disconnecting the network interface
# inside the container and re-adding it. This mimics what happens when a
# laptop switches WiFi networks or a DHCP lease changes.
#
# Verifies that:
# - tmux sessions survive the brief network disruption
# - SSH connections can be re-established after the IP stabilizes
# - State file and UUIDs are intact
#
# NOTE: Inside a Docker container, we cannot fully remove/re-add eth0 without
# losing the container's network entirely. Instead, we simulate IP change by:
# 1. Adding a secondary IP address
# 2. Briefly dropping traffic (simulating the transition gap)
# 3. Removing the secondary IP
# This approximates what NetworkManager would detect.
#
# GUI verification required (manual):
# - NetworkManager detects IP change
# - Proactive reconnect triggered
# - All tabs re-attach seamlessly

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="08-ip-change"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 8: IP Change Simulation"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-08-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-08-b"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-08-a", "name": "session-01"},
    {"uuid": "uuid-chaos-08-b", "name": "session-02"}
  ]
}'

# Verify connectivity baseline.
assert_ok "Baseline SSH connection works" chaos_ssh_probe 10

# Record the original IP.
original_ip=$(chaos_docker_exec "hostname -I | awk '{print \$1}'" | tr -d '[:space:]')
chaos_log "Original IP: $original_ip"

# ---- Simulate IP change -------------------------------------------------- #

chaos_section "Simulating IP change"

# Phase 1: Add a secondary IP (simulates new DHCP lease arriving).
chaos_log "Adding secondary IP address ..."
chaos_docker_exec "ip addr add 172.20.0.99/16 dev eth0 2>/dev/null || true"

# Phase 2: Brief network disruption (simulates the gap during transition).
chaos_log "Simulating transition gap (5s DROP) ..."
chaos_block_port 22
sleep 5

# Phase 3: Restore connectivity (simulates new route established).
chaos_unblock_all
chaos_log "Network restored with new address"

# Phase 4: Remove secondary IP (cleanup, original IP remains).
sleep 2
chaos_docker_exec "ip addr del 172.20.0.99/16 dev eth0 2>/dev/null || true"

# ---- Verification --------------------------------------------------------- #

chaos_section "Verifying post-IP-change state"

# 1. SSH reconnects.
reconnect_ok=false
for i in $(seq 1 15); do
  if chaos_ssh_probe 5; then
    reconnect_ok=true
    break
  fi
  sleep 1
done

if $reconnect_ok; then
  chaos_pass "SSH reconnects after IP change simulation"
else
  chaos_fail "SSH did not reconnect after IP change"
fi

# 2. tmux sessions survived.
session_count=$(chaos_tmux_count_direct)
assert_eq "Both sessions survived IP change" "$session_count" "2"

# 3. Sessions interactive.
chaos_tmux_send_keys "chaos--default--session-01" "echo IP_CHANGE_SURVIVED"
sleep 2
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Session interactive after IP change" "$output" "IP_CHANGE_SURVIVED"

# 4. Current IP is the same (since we removed the secondary).
current_ip=$(chaos_docker_exec "hostname -I | awk '{print \$1}'" | tr -d '[:space:]')
chaos_log "Current IP: $current_ip"
assert_eq "IP address restored to original" "$current_ip" "$original_ip"

# 5. UUIDs intact.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved through IP change" "$uuid" "uuid-chaos-08-a"

# 6. State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after IP change" "$state_status" "VALID"

# 7. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

chaos_summary
