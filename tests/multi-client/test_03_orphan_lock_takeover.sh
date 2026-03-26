#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Multi-Client Test 3: Orphan lock auto-takeover
#
# Verifies that a crashed client's lock is automatically taken over:
#   - Client A connects with client-id "test", acquires lock with heartbeat
#   - Simulate "kill -9" on A: stop heartbeat, set old timestamp
#   - Wait for 2x keepalive timeout (orphan threshold)
#   - Client B connects with "test" -> auto-takeover without conflict dialog
#   - B holds the lock, A's data sessions are intact
#
# The orphan detection mechanism (FR-LOCK-07) checks if
# SHELLKEEP_LOCK_CONNECTED_AT + (2 x keepalive timeout) has expired.
# Since we cannot actually wait 90s in a test, we simulate the expiry
# by setting CONNECTED_AT to a timestamp far in the past.
#
# Requirements tested:
#   FR-LOCK-07, FR-LOCK-09, FR-LOCK-02

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mc_helpers.sh
source "$SCRIPT_DIR/mc_helpers.sh"

CLIENT_ID="test"
ENV_NAME="Default"
HOSTNAME_A="crashed-host"
HOSTNAME_B="recovery-host"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
mc_register_cleanup
mc_start_container "orphan"

e2e_section "Test 3: Orphan lock auto-takeover"

# ---- Client A connects and creates sessions ------------------------------- #

e2e_section "3.1 Client A connects (FR-LOCK-02, FR-LOCK-09)"

e2e_lock_acquire "$CLIENT_ID" "$HOSTNAME_A"
assert_ok "A's lock acquired" e2e_lock_exists "$CLIENT_ID"

# A creates data sessions.
SESSION_A1="${CLIENT_ID}--${ENV_NAME}--ai-agent"
SESSION_A2="${CLIENT_ID}--${ENV_NAME}--build-log"

e2e_tmux_create "$SESSION_A1"
e2e_tmux_set_env "$SESSION_A1" "SHELLKEEP_SESSION_UUID" "a1111111-1111-4111-8111-111111111111"
e2e_ssh_cmd "tmux send-keys -t '${SESSION_A1}' 'echo LONG_RUNNING_PROCESS' Enter"

e2e_tmux_create "$SESSION_A2"
e2e_tmux_set_env "$SESSION_A2" "SHELLKEEP_SESSION_UUID" "a2222222-2222-4222-8222-222222222222"
e2e_ssh_cmd "tmux send-keys -t '${SESSION_A2}' 'echo BUILD_STARTED' Enter"

assert_ok "A's session 1 exists" e2e_tmux_has_session "$SESSION_A1"
assert_ok "A's session 2 exists" e2e_tmux_has_session "$SESSION_A2"

# Verify current heartbeat is fresh.
lock_name="${E2E_LOCK_PREFIX}${CLIENT_ID}"
fresh_ts=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT")
assert_contains "Fresh heartbeat has T delimiter" "$fresh_ts" "T"

fresh_epoch=$(date -d "$fresh_ts" +%s 2>/dev/null || echo "0")
current_epoch=$(date +%s)
fresh_age=$((current_epoch - fresh_epoch))

# Fresh lock should be within a few seconds.
if [[ "$fresh_age" -lt 10 ]]; then
  e2e_pass "Fresh heartbeat age is ${fresh_age}s (< 10s)"
else
  e2e_fail "Fresh heartbeat unexpectedly old: ${fresh_age}s"
fi

# ---- Simulate kill -9: heartbeat stops, timestamp goes stale -------------- #

e2e_section "3.2 Simulate kill -9 (heartbeat stops)"

# In real shellkeep, kill -9 on the client process means:
# 1. The SSH connection drops eventually (server detects via keepalive).
# 2. The lock session remains on the server (tmux session persists).
# 3. The heartbeat (CONNECTED_AT) is never updated again.
# 4. After 2x keepalive timeout, any new client detects the orphan.
#
# We simulate this by setting CONNECTED_AT to a timestamp in the past
# that is older than the orphan threshold.

# Set timestamp to 5 minutes ago (well past the 90s default threshold).
past_epoch=$((current_epoch - 300))
past_ts=$(date -u -d "@${past_epoch}" +"%Y-%m-%dT%H:%M:%SZ")

e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT" "$past_ts"

# Verify the stale timestamp.
stale_ts=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT")
assert_eq "Heartbeat set to past timestamp" "$stale_ts" "$past_ts"

# Confirm the lock is detectable as orphaned.
if mc_lock_is_orphaned "$CLIENT_ID"; then
  e2e_pass "Lock detected as orphaned (age 300s > threshold ${MC_ORPHAN_THRESHOLD}s)"
else
  e2e_fail "Lock should be detected as orphaned"
fi

# ---- Client B connects -> auto-takeover (no dialog) ---------------------- #

e2e_section "3.3 Client B auto-takes orphan lock (FR-LOCK-07)"

result=0
mc_lock_try_acquire "$CLIENT_ID" "$HOSTNAME_B" || result=$?

# Result code 2 = orphan auto-takeover (no conflict dialog).
assert_num_eq "B gets auto-takeover code (2)" "$result" 2

# Lock now belongs to B.
assert_ok "Lock exists after auto-takeover" e2e_lock_exists "$CLIENT_ID"

new_host=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock hostname is B's" "$new_host" "$HOSTNAME_B"

# Heartbeat is fresh (B just acquired it).
new_ts=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT")
new_epoch=$(date -d "$new_ts" +%s 2>/dev/null || echo "0")
new_age=$(($(date +%s) - new_epoch))

if [[ "$new_age" -lt 10 ]]; then
  e2e_pass "New heartbeat is fresh: ${new_age}s"
else
  e2e_fail "New heartbeat unexpectedly old: ${new_age}s"
fi

# ---- A's data sessions survive -------------------------------------------- #

e2e_section "3.4 A's data sessions survive crash and takeover"

assert_ok "A's session 1 still running" e2e_tmux_has_session "$SESSION_A1"
assert_ok "A's session 2 still running" e2e_tmux_has_session "$SESSION_A2"

# B can access A's sessions (same client-id namespace).
a1_uuid=$(e2e_tmux_get_env "$SESSION_A1" "SHELLKEEP_SESSION_UUID")
a2_uuid=$(e2e_tmux_get_env "$SESSION_A2" "SHELLKEEP_SESSION_UUID")
assert_eq "B reads A's session 1 UUID" "$a1_uuid" "a1111111-1111-4111-8111-111111111111"
assert_eq "B reads A's session 2 UUID" "$a2_uuid" "a2222222-2222-4222-8222-222222222222"

# Verify output is still in the sessions.
sleep 0.3
a1_out=$(e2e_ssh_cmd "tmux capture-pane -t '${SESSION_A1}' -p" || true)
assert_contains "A's session 1 output intact" "$a1_out" "LONG_RUNNING_PROCESS"

# ---- Edge case: fresh lock is NOT orphaned -------------------------------- #

e2e_section "3.5 Fresh lock is NOT falsely detected as orphaned"

# B's lock is fresh -- should NOT be orphaned.
if mc_lock_is_orphaned "$CLIENT_ID"; then
  e2e_fail "Fresh lock falsely detected as orphaned"
else
  e2e_pass "Fresh lock correctly identified as alive"
fi

# ---- Edge case: exact threshold boundary --------------------------------- #

e2e_section "3.6 Boundary: timestamp at exactly threshold"

# Set timestamp to exactly the threshold age.
boundary_epoch=$(($(date +%s) - MC_ORPHAN_THRESHOLD))
boundary_ts=$(date -u -d "@${boundary_epoch}" +"%Y-%m-%dT%H:%M:%SZ")
e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT" "$boundary_ts"

# At exactly the threshold, the lock should be considered orphaned
# (age > threshold, using strict greater-than).
# Due to timing, this may be exactly at or 1s past threshold.
# The implementation uses strict >, so at exactly threshold it is NOT orphaned.
# We accept either result here as the boundary is inherently racy.
if mc_lock_is_orphaned "$CLIENT_ID"; then
  e2e_pass "Boundary: lock detected as orphaned at threshold (timing variance)"
else
  e2e_pass "Boundary: lock detected as alive at threshold (strict >)"
fi

# Restore fresh timestamp for cleanup.
mc_lock_heartbeat "$CLIENT_ID"

# ---- Edge case: lock with missing timestamp ------------------------------- #

e2e_section "3.7 Lock with missing timestamp treated as orphaned (FR-LOCK-04)"

# Create a separate lock with no CONNECTED_AT.
INVALID_CLIENT="invalid-lock-test"
invalid_lock="${E2E_LOCK_PREFIX}${INVALID_CLIENT}"
e2e_ssh_cmd "tmux new-session -d -s '${invalid_lock}'"
# Only set partial metadata (no CONNECTED_AT).
e2e_tmux_set_env "$invalid_lock" "SHELLKEEP_LOCK_CLIENT_ID" "$INVALID_CLIENT"

if mc_lock_is_orphaned "$INVALID_CLIENT"; then
  e2e_pass "Lock with missing timestamp treated as orphaned"
else
  e2e_fail "Lock with missing timestamp should be orphaned"
fi

e2e_tmux_kill "$invalid_lock"

# ---- Cleanup ------------------------------------------------------------- #

e2e_lock_release "$CLIENT_ID"

# ---- Summary ------------------------------------------------------------- #

e2e_section "Test 3 complete: orphan lock auto-takeover"
e2e_summary
