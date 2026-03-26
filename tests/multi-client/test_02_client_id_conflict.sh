#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Multi-Client Test 2: Client-ID conflict and force-takeover
#
# Verifies client-ID conflict detection and resolution:
#   - Client A connects with client-id "shared"
#   - Client B tries "shared" -> conflict detected (lock exists)
#   - B reads conflict metadata (hostname, timestamp, version)
#   - B chooses "Disconnect and connect" -> force-takeover
#   - B now holds the lock, A's lock is gone
#   - A retries -> now A gets the conflict
#   - A force-takes back -> A holds lock again
#   - Data sessions survive all lock transfers
#
# Requirements tested:
#   FR-LOCK-01, FR-LOCK-02, FR-LOCK-03, FR-LOCK-04, FR-LOCK-05,
#   FR-LOCK-06, FR-LOCK-07

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mc_helpers.sh
source "$SCRIPT_DIR/mc_helpers.sh"

CLIENT_ID="shared"
ENV_NAME="Default"
HOSTNAME_A="office-desktop"
HOSTNAME_B="home-laptop"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
mc_register_cleanup
mc_start_container "conflict"

e2e_section "Test 2: Client-ID conflict and force-takeover"

# ---- Client A connects ---------------------------------------------------- #

e2e_section "2.1 Client A acquires lock (FR-LOCK-02)"

e2e_lock_acquire "$CLIENT_ID" "$HOSTNAME_A"
assert_ok "A's lock exists" e2e_lock_exists "$CLIENT_ID"

# A creates a data session.
SESSION_A="${CLIENT_ID}--${ENV_NAME}--work-session"
e2e_tmux_create "$SESSION_A"
e2e_tmux_set_env "$SESSION_A" "SHELLKEEP_SESSION_UUID" "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
e2e_ssh_cmd "tmux send-keys -t '${SESSION_A}' 'echo RUNNING_PROCESS_A' Enter"

assert_ok "A's data session created" e2e_tmux_has_session "$SESSION_A"

# Verify lock metadata (FR-LOCK-03).
lock_name="${E2E_LOCK_PREFIX}${CLIENT_ID}"
a_lock_host=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
a_lock_cid=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CLIENT_ID")
a_lock_ts=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT")
a_lock_ver=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_VERSION")

assert_eq "Lock hostname is A's" "$a_lock_host" "$HOSTNAME_A"
assert_eq "Lock client-id matches" "$a_lock_cid" "$CLIENT_ID"
assert_contains "Lock has ISO timestamp" "$a_lock_ts" "T"
assert_eq "Lock version is 0.1.0" "$a_lock_ver" "0.1.0"

# ---- Client B tries same client-id -> conflict --------------------------- #

e2e_section "2.2 Client B detects conflict (FR-LOCK-01, FR-LOCK-05)"

# B attempts atomic lock creation -- must fail.
result=0
mc_lock_try_acquire "$CLIENT_ID" "$HOSTNAME_B" || result=$?

assert_num_eq "B gets conflict code (1)" "$result" 1

# B reads conflict info to display in dialog.
conflict_host=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
conflict_ts=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT")
conflict_ver=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_VERSION")

assert_eq "Conflict dialog shows A's hostname" "$conflict_host" "$HOSTNAME_A"
assert_contains "Conflict dialog shows timestamp" "$conflict_ts" "T"
assert_eq "Conflict dialog shows version" "$conflict_ver" "0.1.0"
e2e_pass "B has all info needed for conflict dialog"

# Lock still belongs to A.
still_a=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock still held by A" "$still_a" "$HOSTNAME_A"

# ---- Client B force-takes lock ------------------------------------------- #

e2e_section "2.3 Client B force-takeover (FR-LOCK-06)"

mc_lock_force_takeover "$CLIENT_ID" "$HOSTNAME_B"
assert_ok "Lock exists after takeover" e2e_lock_exists "$CLIENT_ID"

# Lock now belongs to B.
new_host=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock hostname is now B's" "$new_host" "$HOSTNAME_B"

new_cid=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CLIENT_ID")
assert_eq "Lock client-id still correct" "$new_cid" "$CLIENT_ID"

# A's data session survives the lock transfer.
assert_ok "A's data session survives B's takeover" e2e_tmux_has_session "$SESSION_A"

# B can access A's session (same client-id namespace).
a_uuid=$(e2e_tmux_get_env "$SESSION_A" "SHELLKEEP_SESSION_UUID")
assert_eq "B can read A's session UUID" "$a_uuid" "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"

# B creates its own session.
SESSION_B="${CLIENT_ID}--${ENV_NAME}--laptop-session"
e2e_tmux_create "$SESSION_B"
e2e_tmux_set_env "$SESSION_B" "SHELLKEEP_SESSION_UUID" "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
assert_ok "B's data session created" e2e_tmux_has_session "$SESSION_B"

# ---- Client A retries -> conflict again ---------------------------------- #

e2e_section "2.4 Client A retries -> gets conflict (FR-LOCK-01)"

result_a=0
mc_lock_try_acquire "$CLIENT_ID" "$HOSTNAME_A" || result_a=$?

assert_num_eq "A gets conflict code (1)" "$result_a" 1

# Lock still belongs to B.
holder=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock still held by B" "$holder" "$HOSTNAME_B"

# ---- Client A force-takes back ------------------------------------------- #

e2e_section "2.5 Client A force-takes back (FR-LOCK-06)"

mc_lock_force_takeover "$CLIENT_ID" "$HOSTNAME_A"
assert_ok "Lock exists after A retakes" e2e_lock_exists "$CLIENT_ID"

retaken_host=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Lock hostname is A's again" "$retaken_host" "$HOSTNAME_A"

# Both data sessions survive.
assert_ok "A's session survives" e2e_tmux_has_session "$SESSION_A"
assert_ok "B's session survives" e2e_tmux_has_session "$SESSION_B"

# ---- Verify no state corruption after ping-pong -------------------------- #

e2e_section "2.6 State integrity after lock ping-pong"

# Write state as A.
all_sessions=$(printf '%s\n' "$SESSION_A" "$SESSION_B")
state_json=$(mc_generate_state "$CLIENT_ID" "$ENV_NAME" "$all_sessions")
e2e_write_state "$CLIENT_ID" "$state_json"

read_back=$(e2e_read_state "$CLIENT_ID")
assert_contains "State has A's UUID" "$read_back" "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
assert_contains "State has B's UUID" "$read_back" "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
assert_contains "State has correct client-id" "$read_back" "\"${CLIENT_ID}\""

# Only one state file for this client-id.
file_count=$(e2e_ssh_cmd "ls ${E2E_REMOTE_STATE_DIR}/${CLIENT_ID}.json 2>/dev/null | wc -l")
assert_num_eq "Single state file for shared client-id" "$file_count" 1

# ---- Cleanup ------------------------------------------------------------- #

e2e_lock_release "$CLIENT_ID"

# ---- Summary ------------------------------------------------------------- #

e2e_section "Test 2 complete: client-ID conflict"
e2e_summary
