#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 02: Packet loss 10%
#
# Applies 10% packet loss via tc netem and verifies that:
# - SSH sessions remain stable (no disconnect)
# - tmux sessions continue running
# - Command output is complete (SSH/TCP handles retransmission)
# - State file remains consistent
#
# GUI verification required (manual):
# - Session stable, no disconnect indicator shown
# - Output renders completely in terminal widget

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="02-loss-10"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 2: Packet Loss 10%"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-02-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-02-b"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-02-a", "name": "session-01"},
    {"uuid": "uuid-chaos-02-b", "name": "session-02"}
  ]
}'

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Applying 10% packet loss ..."
chaos_apply_loss 10

# ---- Verification --------------------------------------------------------- #

chaos_section "Verifying stability under 10% packet loss"

# 1. Multiple SSH connections succeed (retry a few times to account for loss).
ssh_successes=0
for i in $(seq 1 5); do
  if chaos_ssh_probe 10; then
    ((ssh_successes++)) || true
  fi
done
assert_num_ge "At least 4/5 SSH probes succeeded" "$ssh_successes" 4

# 2. tmux sessions alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "Both tmux sessions exist" "$session_count" "2"

# 3. Run a command that produces multi-line output and verify completeness.
chaos_tmux_send_keys "chaos--default--session-01" "seq 1 20"
sleep 3
output=$(chaos_tmux_capture "chaos--default--session-01")
# Check that both first and last lines appear (TCP retransmits fill gaps).
assert_contains "Output contains first line" "$output" "1"
assert_contains "Output contains last line (20)" "$output" "20"

# 4. Sustained operation: run multiple commands over 15 seconds.
chaos_log "Running sustained operations for 15 seconds ..."
for i in $(seq 1 5); do
  chaos_tmux_send_keys "chaos--default--session-02" "echo SUSTAINED_${i}"
  sleep 3
done
output2=$(chaos_tmux_capture "chaos--default--session-02")
assert_contains "Sustained output includes final command" "$output2" "SUSTAINED_5"

# 5. Sessions still interactive after sustained loss.
chaos_tmux_send_keys "chaos--default--session-01" "echo STILL_ALIVE"
sleep 3
output3=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Session still interactive after sustained loss" "$output3" "STILL_ALIVE"

# 6. State file integrity.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid" "$state_status" "VALID"

# 7. UUIDs preserved.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved under packet loss" "$uuid" "uuid-chaos-02-a"

# 8. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# ---- Cleanup -------------------------------------------------------------- #

chaos_clear_tc
chaos_summary
