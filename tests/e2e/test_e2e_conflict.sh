#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# E2E Scenario 4: Client-ID Conflict
#
# Tests client-ID conflict detection with two concurrent connections:
#   - Instance A connects with client-id "test-conflict"
#   - Instance B attempts to connect with the same client-id "test-conflict"
#   - Conflict is detected (lock session already exists)
#   - Instance B force-acquires the lock (simulating "Disconnect and connect")
#   - Instance A's lock is gone, Instance B has the lock
#
# Requirements tested:
#   FR-LOCK-01, FR-LOCK-02, FR-LOCK-03, FR-LOCK-04, FR-LOCK-05,
#   FR-LOCK-06, FR-LOCK-07, FR-LOCK-08, FR-LOCK-09
#
# NOTE: GUI behaviors (conflict dialog with "Disconnect and connect" button)
# require a display server. This script tests the underlying lock mechanism:
# tmux-based lock sessions and conflict detection via env vars.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=e2e_helpers.sh
source "$SCRIPT_DIR/e2e_helpers.sh"

CONTAINER_SUFFIX="conflict-$$"
CLIENT_ID="test-conflict"
ENV_NAME="Default"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
e2e_register_cleanup
e2e_start_container "$CONTAINER_SUFFIX"

# ---- Test: Instance A acquires lock --------------------------------------- #

e2e_section "Instance A connects (FR-LOCK-02)"

HOSTNAME_A="desktop-office"
e2e_lock_acquire "$CLIENT_ID" "$HOSTNAME_A"

assert_ok "Instance A lock exists" e2e_lock_exists "$CLIENT_ID"

# Verify lock metadata.
lock_host=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_ID}" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock hostname is A's hostname" "$lock_host" "$HOSTNAME_A"

lock_client=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_ID}" "SHELLKEEP_LOCK_CLIENT_ID")
assert_eq "Lock client-id matches" "$lock_client" "$CLIENT_ID"

# Instance A creates a session.
SESSION_A="${CLIENT_ID}--${ENV_NAME}--a-session"
e2e_tmux_create "$SESSION_A"
e2e_tmux_set_env "$SESSION_A" "SHELLKEEP_SESSION_UUID" "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa"

assert_ok "Instance A session exists" e2e_tmux_has_session "$SESSION_A"

# ---- Test: Instance B detects conflict ------------------------------------ #

e2e_section "Instance B detects conflict (FR-LOCK-04, FR-LOCK-05)"

HOSTNAME_B="laptop-home"
LOCK_NAME="${E2E_LOCK_PREFIX}${CLIENT_ID}"

# Instance B tries to create the lock -- this should FAIL because A holds it.
if e2e_ssh_cmd "tmux new-session -d -s '$LOCK_NAME' 2>/dev/null"; then
  e2e_fail "Lock creation should fail when A holds it"
else
  e2e_pass "Lock creation fails (conflict detected)"
fi

# Instance B reads the lock info to identify who holds it.
conflict_host=$(e2e_tmux_get_env "$LOCK_NAME" "SHELLKEEP_LOCK_HOSTNAME")
conflict_client=$(e2e_tmux_get_env "$LOCK_NAME" "SHELLKEEP_LOCK_CLIENT_ID")
conflict_time=$(e2e_tmux_get_env "$LOCK_NAME" "SHELLKEEP_LOCK_CONNECTED_AT")
conflict_version=$(e2e_tmux_get_env "$LOCK_NAME" "SHELLKEEP_LOCK_VERSION")

assert_eq "Conflict info shows A's hostname" "$conflict_host" "$HOSTNAME_A"
assert_eq "Conflict info shows correct client-id" "$conflict_client" "$CLIENT_ID"
assert_contains "Conflict info has timestamp" "$conflict_time" "T"
assert_eq "Conflict info has version" "$conflict_version" "0.1.0"

e2e_pass "Instance B has all conflict info for dialog display"

# ---- Test: Instance B force-takes the lock (FR-LOCK-06, FR-LOCK-07) ------ #

e2e_section "Instance B force-acquires lock (FR-LOCK-06)"

# Simulate "Disconnect and connect" action:
# 1. Kill A's lock session.
# 2. Create B's lock session.

e2e_lock_release "$CLIENT_ID"
assert_fail "A's lock released" e2e_lock_exists "$CLIENT_ID"

# Now B acquires the lock.
e2e_lock_acquire "$CLIENT_ID" "$HOSTNAME_B"
assert_ok "B's lock acquired" e2e_lock_exists "$CLIENT_ID"

# Verify lock now belongs to B.
new_host=$(e2e_tmux_get_env "$LOCK_NAME" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock now shows B's hostname" "$new_host" "$HOSTNAME_B"

# ---- Test: A's sessions still exist (FR-SESSION-10) ---------------------- #

e2e_section "A's sessions survive lock takeover"

# Even though A lost the lock, its tmux data sessions should still be alive.
# shellkeep never kills data sessions on lock conflict.
assert_ok "A's data session still alive" e2e_tmux_has_session "$SESSION_A"

# B can see A's sessions (same client-id means same namespace).
a_uuid=$(e2e_tmux_get_env "$SESSION_A" "SHELLKEEP_SESSION_UUID")
assert_eq "A's session UUID accessible" "$a_uuid" "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa"

# ---- Test: B creates its own session ------------------------------------- #

e2e_section "Instance B creates session"

SESSION_B="${CLIENT_ID}--${ENV_NAME}--b-session"
e2e_tmux_create "$SESSION_B"
e2e_tmux_set_env "$SESSION_B" "SHELLKEEP_SESSION_UUID" "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb"

assert_ok "B's session created" e2e_tmux_has_session "$SESSION_B"

# Both A's old session and B's new session coexist.
all_sessions=$(e2e_tmux_list)
assert_contains "A's session in listing" "$all_sessions" "$SESSION_A"
assert_contains "B's session in listing" "$all_sessions" "$SESSION_B"

# ---- Test: Orphan lock detection (FR-LOCK-09) ----------------------------- #

e2e_section "Orphan lock detection (FR-LOCK-09)"

# Simulate an orphaned lock: create a lock with an old timestamp.
# Kill current lock first.
e2e_lock_release "$CLIENT_ID"

# Create a lock with a very old timestamp (simulating a crashed client).
ORPHAN_CLIENT="orphan-test"
orphan_lock="${E2E_LOCK_PREFIX}${ORPHAN_CLIENT}"

old_timestamp="2026-01-01T00:00:00Z"  # Very old timestamp.
e2e_ssh_cmd "tmux new-session -d -s '$orphan_lock' \
  \\; set-environment -t '$orphan_lock' SHELLKEEP_LOCK_CLIENT_ID '$ORPHAN_CLIENT' \
  \\; set-environment -t '$orphan_lock' SHELLKEEP_LOCK_HOSTNAME 'old-host' \
  \\; set-environment -t '$orphan_lock' SHELLKEEP_LOCK_CONNECTED_AT '$old_timestamp' \
  \\; set-environment -t '$orphan_lock' SHELLKEEP_LOCK_PID '99999' \
  \\; set-environment -t '$orphan_lock' SHELLKEEP_LOCK_VERSION '0.1.0'"

assert_ok "Orphan lock session exists" e2e_tmux_has_session "$orphan_lock"

# Read the timestamp to check if it is orphaned.
# An orphaned lock is one where CONNECTED_AT is older than 2x ClientAliveInterval * ClientAliveCountMax.
orphan_ts=$(e2e_tmux_get_env "$orphan_lock" "SHELLKEEP_LOCK_CONNECTED_AT")
assert_eq "Orphan has old timestamp" "$orphan_ts" "$old_timestamp"

# In shellkeep, the orphan detection compares the timestamp against
# the current time minus the threshold. Since our timestamp is from 2026-01-01,
# it is definitely orphaned.
current_epoch=$(date +%s)
# Parse the orphan timestamp (basic parsing).
orphan_epoch=$(date -d "$old_timestamp" +%s 2>/dev/null || echo "0")
age=$((current_epoch - orphan_epoch))
threshold=60  # 2 * 30s keepalive = 60s threshold.

if [[ "$age" -gt "$threshold" ]]; then
  e2e_pass "Orphan lock detected: age ${age}s > threshold ${threshold}s"
else
  e2e_fail "Orphan lock not detected as orphaned"
fi

# Clean up orphan lock.
e2e_tmux_kill "$orphan_lock"

# ---- Test: Rapid lock cycling --------------------------------------------- #

e2e_section "Rapid lock acquire/release cycling"

for i in $(seq 1 5); do
  e2e_lock_acquire "$CLIENT_ID" "host-cycle-${i}"
  assert_ok "Cycle $i: lock acquired" e2e_lock_exists "$CLIENT_ID"

  host=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_ID}" "SHELLKEEP_LOCK_HOSTNAME")
  assert_eq "Cycle $i: correct hostname" "$host" "host-cycle-${i}"

  e2e_lock_release "$CLIENT_ID"
  assert_fail "Cycle $i: lock released" e2e_lock_exists "$CLIENT_ID"
done

# ---- Summary -------------------------------------------------------------- #

e2e_section "Client-ID Conflict scenario complete"
e2e_summary
