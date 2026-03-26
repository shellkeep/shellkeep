#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 04: Abrupt disconnect (iptables DROP)
#
# Blocks all SSH traffic via iptables DROP and verifies that:
# - SSH connections are completely severed
# - tmux sessions survive on the server
# - After restoring, connections can be re-established
# - State and UUIDs are intact
#
# GUI verification required (manual):
# - FR-CONN-16: spinner/reconnecting indicator appears
# - Reconnect happens automatically after iptables restore
# - All tabs re-attach to their tmux sessions

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="04-abrupt-disconnect"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 4: Abrupt Disconnect (iptables DROP)"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-04-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-04-b"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-04-a", "name": "session-01"},
    {"uuid": "uuid-chaos-04-b", "name": "session-02"}
  ]
}'

# Run a long-lived process in session-01 to verify it survives.
chaos_tmux_send_keys "chaos--default--session-01" "while true; do echo tick_\$(date +%s); sleep 2; done"
sleep 3

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Blocking SSH port 22 via iptables DROP ..."
chaos_block_port 22

# ---- Verification during disconnect -------------------------------------- #

chaos_section "Verifying behavior during disconnect"

# 1. SSH is completely blocked.
assert_fail "SSH connection fails during DROP" chaos_ssh_probe 5

# 2. tmux sessions survive (server-side check).
session_count=$(chaos_tmux_count_direct)
assert_eq "tmux sessions survive iptables DROP" "$session_count" "2"

# 3. Long-running process still ticking (capture via docker exec).
sleep 4
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Long-running process still ticking" "$output" "tick_"

# 4. UUIDs still set.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved during disconnect" "$uuid" "uuid-chaos-04-a"

# ---- Restore network ----------------------------------------------------- #

chaos_section "Restoring network and verifying reconnection"

chaos_log "Unblocking SSH port ..."
chaos_unblock_all
sleep 2

# 5. SSH works again.
assert_ok "SSH reconnects after iptables flush" chaos_ssh_probe 10

# 6. Sessions still alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "Both sessions alive after restore" "$session_count" "2"

# 7. Can re-attach and interact.
chaos_tmux_send_keys "chaos--default--session-02" "echo RECONNECTED_OK"
sleep 2
output2=$(chaos_tmux_capture "chaos--default--session-02")
assert_contains "Session interactive after reconnect" "$output2" "RECONNECTED_OK"

# 8. Long-running process continued during disconnect.
output3=$(chaos_tmux_capture "chaos--default--session-01")
# There should be multiple tick_ lines accumulated during disconnect.
tick_count=$(echo "$output3" | grep -c "tick_" || echo "0")
chaos_log "Tick count after restore: $tick_count"
assert_num_ge "Process produced output during disconnect" "$tick_count" 2

# 9. State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after disconnect" "$state_status" "VALID"

# 10. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# Kill the long-running process.
chaos_docker_exec "su - testuser -c \"tmux send-keys -t 'chaos--default--session-01' C-c\""
sleep 1

chaos_summary
