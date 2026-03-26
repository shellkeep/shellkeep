#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Chaos Test 03: Packet loss 30%
#
# Applies 30% packet loss via tc netem and verifies that:
# - SSH connections may drop but can be re-established
# - tmux sessions survive on the server regardless
# - After clearing the loss, sessions are fully restorable
# - State file is not corrupted
#
# GUI verification required (manual):
# - Auto-reconnection triggers when connection drops
# - Session restored with correct scrollback after reconnect

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/chaos_helpers.sh"

SCENARIO="03-loss-30"

# ---- Setup ---------------------------------------------------------------- #

chaos_check_prereqs
chaos_build_image
chaos_start_container "agent31-${SCENARIO}"
chaos_register_cleanup

chaos_section "Scenario 3: Packet Loss 30%"

# Create tmux sessions.
chaos_tmux_create "chaos--default--session-01"
chaos_tmux_create "chaos--default--session-02"
chaos_tmux_create "chaos--default--session-03"

chaos_tmux_set_env "chaos--default--session-01" "SHELLKEEP_SESSION_UUID" "uuid-chaos-03-a"
chaos_tmux_set_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID" "uuid-chaos-03-b"
chaos_tmux_set_env "chaos--default--session-03" "SHELLKEEP_SESSION_UUID" "uuid-chaos-03-c"

chaos_write_state "chaos" '{
  "version": 1,
  "client_id": "chaos",
  "environment": "default",
  "sessions": [
    {"uuid": "uuid-chaos-03-a", "name": "session-01"},
    {"uuid": "uuid-chaos-03-b", "name": "session-02"},
    {"uuid": "uuid-chaos-03-c", "name": "session-03"}
  ]
}'

# Seed some output in sessions before chaos.
chaos_tmux_send_keys "chaos--default--session-01" "echo PRE_CHAOS_MARKER"
sleep 1

# ---- Apply chaos ---------------------------------------------------------- #

chaos_log "Applying 30% packet loss ..."
chaos_apply_loss 30

# ---- Verification during chaos -------------------------------------------- #

chaos_section "Verifying behavior under 30% packet loss"

# 1. tmux sessions survive on the server (checked via docker exec, not SSH).
session_count=$(chaos_tmux_count_direct)
assert_eq "All 3 tmux sessions survive 30% loss" "$session_count" "3"

# 2. Attempt SSH connections -- some may fail, that is expected at 30%.
ssh_successes=0
for i in $(seq 1 10); do
  if chaos_ssh_probe 15; then
    ((ssh_successes++)) || true
  fi
done
chaos_log "SSH probes succeeded: ${ssh_successes}/10"
# At 30% loss, TCP retransmits usually allow connections, but some may fail.
# We expect at least some to succeed.
assert_num_ge "At least 3/10 SSH probes succeeded under 30% loss" "$ssh_successes" 3

# 3. Sessions still running (server-side check).
for sess in "chaos--default--session-01" "chaos--default--session-02" "chaos--default--session-03"; do
  assert_ok "Session $sess exists during 30% loss" chaos_tmux_has_session "$sess"
done

# 4. Pre-chaos output preserved in tmux scrollback.
output=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Pre-chaos output preserved" "$output" "PRE_CHAOS_MARKER"

# ---- Restore network ----------------------------------------------------- #

chaos_section "Restoring network and verifying recovery"

chaos_clear_tc
sleep 2  # Allow TCP to stabilize.

# 5. SSH works reliably again.
assert_ok "SSH works after loss cleared" chaos_ssh_probe 10

# 6. All sessions still alive.
session_count=$(chaos_tmux_count_direct)
assert_eq "All 3 sessions survived and restored" "$session_count" "3"

# 7. Sessions are interactive post-restore.
chaos_tmux_send_keys "chaos--default--session-01" "echo POST_RESTORE_OK"
sleep 2
output2=$(chaos_tmux_capture "chaos--default--session-01")
assert_contains "Session interactive after restore" "$output2" "POST_RESTORE_OK"

# 8. UUIDs intact.
uuid=$(chaos_tmux_get_env "chaos--default--session-02" "SHELLKEEP_SESSION_UUID")
assert_eq "UUID preserved through 30% loss" "$uuid" "uuid-chaos-03-b"

# 9. State file valid.
state_status=$(chaos_verify_state "chaos")
assert_eq "State file valid after 30% loss" "$state_status" "VALID"

# 10. No crashes.
assert_ok "sshd still running" chaos_verify_sshd_running
assert_ok "No crash files" chaos_verify_no_crashes

chaos_summary
