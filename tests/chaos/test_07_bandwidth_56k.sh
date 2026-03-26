#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 07: 56kbps bandwidth limit
#
# Applies a 56kbit/s bandwidth cap via tc tbf and verifies that:
# - SSH connections succeed (slowly)
# - tmux sessions remain usable
# - No crash or freeze under extreme bandwidth constraint
# - Large output is slow but eventually completes
#
# GUI verification required (manual):
# - Terminal is slow but does not freeze or crash
# - No visual corruption in terminal widget
# - User can still type commands (high latency acceptable)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="07-bandwidth-56k"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 7: 56kbps Bandwidth Limit"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-07-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-07-b"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-07-a", "name": "session-01"},
    {"uuid": "uuid-chaos-07-b", "name": "session-02"}
  ]
}'

# ---- Baseline ------------------------------------------------------------ #

chaos_log "Measuring baseline transfer ..."
baseline_start=$(date +%s%N)
chaos_ssh_cmd "echo BASELINE_OK" &>/dev/null
baseline_end=$(date +%s%N)
baseline_ms=$(( (baseline_end - baseline_start) / 1000000 ))
chaos_log "Baseline command: ${baseline_ms}ms"

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Applying 56kbit/s bandwidth limit ..."
chaos_apply_bandwidth "56kbit" "1540" "50ms"

# ---- Verification --------------------------------------------------------- #

chaos_section "Verifying operation under 56kbps"

# 1. SSH still connects (may be slow).
assert_ok "SSH connects under 56kbps" chaos_ssh_probe 30

# 2. Simple command works.
result=$(timeout 30 sshpass -p "$CHAOS_SSH_PASS" \
  ssh $CHAOS_SSH_OPTS \
  -p "$CHAOS_SSH_PORT" \
  "${CHAOS_SSH_USER}@${CHAOS_SSH_HOST}" \
  "echo BW_TEST_OK" 2>/dev/null || echo "TIMEOUT")
assert_contains "Simple command succeeds under 56kbps" "$result" "BW_TEST_OK"

# 3. tmux sessions alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "Both sessions exist under bandwidth limit" "$session_count" "2"

# 4. Interactive command in session.
chaos_tmux_send_keys "chaos--default--session-01" "echo SLOW_BUT_OK"
sleep 5  # Extra wait for constrained bandwidth.
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Session accepts commands under 56kbps" "$output" "SLOW_BUT_OK"

# 5. Moderate output test (not too large -- 56kbps is ~7KB/s).
chaos_tmux_send_keys "chaos--default--session-02" "seq 1 50"
sleep 8
output2=$(chaos_tmux_capture "chaos--default--session-02")
assert_contains "Moderate output starts" "$output2" "1"
# At 56kbps the full seq might take a moment but should complete.
assert_contains "Moderate output completes" "$output2" "50"

# 6. UUIDs preserved.
uuid=$(chaos_tmux_get_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved under bandwidth limit" "$uuid" "uuid-chaos-07-a"

# 7. State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid under bandwidth limit" "$state_status" "VALID"

# 8. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

# ---- Cleanup -------------------------------------------------------------- #

chaos_clear_tc

# Verify speed restored.
chaos_log "Verifying bandwidth restored ..."
assert_ok "Fast connection after clearing bandwidth limit" chaos_ssh_probe 10

chaos_summary
