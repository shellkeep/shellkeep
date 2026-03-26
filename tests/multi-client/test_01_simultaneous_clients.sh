#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Multi-Client Test 1: Two client-IDs simultaneously
#
# Verifies that two clients ("desktop" with 3 tabs and "laptop" with 2 tabs)
# can coexist on the same server with complete isolation:
#   - Both clients acquire independent locks
#   - Sessions use different tmux name prefixes (<client-id>--<env>--<name>)
#   - Separate state files on the server
#   - Independent locks (different lock sessions)
#   - Closing a tab in client A does not affect client B
#   - Both disconnect -> locks destroyed, sessions alive on server
#
# Requirements tested:
#   FR-LOCK-01, FR-LOCK-02, FR-LOCK-10, FR-SESSION-04, FR-SESSION-10,
#   FR-STATE-01, FR-ENV-02

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mc_helpers.sh
source "$SCRIPT_DIR/mc_helpers.sh"

CLIENT_A="desktop"
CLIENT_B="laptop"
ENV_NAME="Default"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
mc_register_cleanup
mc_start_container "simul"

e2e_section "Test 1: Two client-IDs simultaneously"

# ---- Both clients acquire locks ------------------------------------------- #

e2e_section "1.1 Independent lock acquisition"

e2e_lock_acquire "$CLIENT_A" "desktop-workstation"
e2e_lock_acquire "$CLIENT_B" "laptop-portable"

assert_ok "Desktop lock acquired" e2e_lock_exists "$CLIENT_A"
assert_ok "Laptop lock acquired" e2e_lock_exists "$CLIENT_B"

# Verify lock metadata is independent.
a_host=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_A}" "SHELLKEEP_LOCK_HOSTNAME")
b_host=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_B}" "SHELLKEEP_LOCK_HOSTNAME")
assert_eq "Desktop lock has desktop hostname" "$a_host" "desktop-workstation"
assert_eq "Laptop lock has laptop hostname" "$b_host" "laptop-portable"

a_cid=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_A}" "SHELLKEEP_LOCK_CLIENT_ID")
b_cid=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_B}" "SHELLKEEP_LOCK_CLIENT_ID")
assert_eq "Desktop lock client-id correct" "$a_cid" "$CLIENT_A"
assert_eq "Laptop lock client-id correct" "$b_cid" "$CLIENT_B"

# ---- Desktop creates 3 sessions, laptop creates 2 ----------------------- #

e2e_section "1.2 Session creation with client-id prefixes"

# Desktop: 3 tabs.
DESKTOP_S1="${CLIENT_A}--${ENV_NAME}--code-editor"
DESKTOP_S2="${CLIENT_A}--${ENV_NAME}--build-runner"
DESKTOP_S3="${CLIENT_A}--${ENV_NAME}--log-viewer"

DESKTOP_U1="d1111111-1111-4111-8111-111111111111"
DESKTOP_U2="d2222222-2222-4222-8222-222222222222"
DESKTOP_U3="d3333333-3333-4333-8333-333333333333"

for sname in "$DESKTOP_S1" "$DESKTOP_S2" "$DESKTOP_S3"; do
  e2e_tmux_create "$sname"
done
e2e_tmux_set_env "$DESKTOP_S1" "SHELLKEEP_SESSION_UUID" "$DESKTOP_U1"
e2e_tmux_set_env "$DESKTOP_S2" "SHELLKEEP_SESSION_UUID" "$DESKTOP_U2"
e2e_tmux_set_env "$DESKTOP_S3" "SHELLKEEP_SESSION_UUID" "$DESKTOP_U3"

# Laptop: 2 tabs.
LAPTOP_S1="${CLIENT_B}--${ENV_NAME}--ssh-session"
LAPTOP_S2="${CLIENT_B}--${ENV_NAME}--monitoring"

LAPTOP_U1="a1111111-1111-4111-8111-111111111111"
LAPTOP_U2="a2222222-2222-4222-8222-222222222222"

for sname in "$LAPTOP_S1" "$LAPTOP_S2"; do
  e2e_tmux_create "$sname"
done
e2e_tmux_set_env "$LAPTOP_S1" "SHELLKEEP_SESSION_UUID" "$LAPTOP_U1"
e2e_tmux_set_env "$LAPTOP_S2" "SHELLKEEP_SESSION_UUID" "$LAPTOP_U2"

# Verify all sessions exist.
for sname in "$DESKTOP_S1" "$DESKTOP_S2" "$DESKTOP_S3"; do
  assert_ok "Desktop session $sname exists" e2e_tmux_has_session "$sname"
done
for sname in "$LAPTOP_S1" "$LAPTOP_S2"; do
  assert_ok "Laptop session $sname exists" e2e_tmux_has_session "$sname"
done

# ---- Verify session name prefixes are isolated --------------------------- #

e2e_section "1.3 Session namespace isolation (FR-SESSION-04, FR-ENV-02)"

# Total: 3 desktop + 2 laptop + 2 locks = 7.
total=$(e2e_tmux_list | wc -l)
assert_num_eq "7 total tmux sessions" "$total" 7

desktop_count=$(mc_count_client_sessions "$CLIENT_A")
laptop_count=$(mc_count_client_sessions "$CLIENT_B")
assert_num_eq "3 desktop data sessions" "$desktop_count" 3
assert_num_eq "2 laptop data sessions" "$laptop_count" 2

# Cross-check: no mixing.
desktop_list=$(mc_list_client_sessions "$CLIENT_A")
laptop_list=$(mc_list_client_sessions "$CLIENT_B")
assert_not_contains "Desktop list has no laptop sessions" "$desktop_list" "$CLIENT_B"
assert_not_contains "Laptop list has no desktop sessions" "$laptop_list" "$CLIENT_A"

# ---- Separate state files ------------------------------------------------- #

e2e_section "1.4 Separate state files (FR-STATE-01)"

DESKTOP_SESSIONS_STR=$(printf '%s\n' "$DESKTOP_S1" "$DESKTOP_S2" "$DESKTOP_S3")
LAPTOP_SESSIONS_STR=$(printf '%s\n' "$LAPTOP_S1" "$LAPTOP_S2")

desktop_state=$(mc_generate_state "$CLIENT_A" "$ENV_NAME" "$DESKTOP_SESSIONS_STR")
laptop_state=$(mc_generate_state "$CLIENT_B" "$ENV_NAME" "$LAPTOP_SESSIONS_STR")

e2e_write_state "$CLIENT_A" "$desktop_state"
e2e_write_state "$CLIENT_B" "$laptop_state"

assert_ok "Desktop state file exists" e2e_state_exists "$CLIENT_A"
assert_ok "Laptop state file exists" e2e_state_exists "$CLIENT_B"

# Verify two separate files.
file_count=$(e2e_ssh_cmd "ls ${E2E_REMOTE_STATE_DIR}/*.json 2>/dev/null | wc -l")
assert_num_eq "2 state files on server" "$file_count" 2

# Verify content isolation.
d_state=$(e2e_read_state "$CLIENT_A")
l_state=$(e2e_read_state "$CLIENT_B")

assert_contains "Desktop state has desktop client-id" "$d_state" "\"${CLIENT_A}\""
assert_not_contains "Desktop state excludes laptop client-id" "$d_state" "\"${CLIENT_B}\""
assert_contains "Desktop state has desktop UUID" "$d_state" "$DESKTOP_U1"
assert_not_contains "Desktop state excludes laptop UUID" "$d_state" "$LAPTOP_U1"

assert_contains "Laptop state has laptop client-id" "$l_state" "\"${CLIENT_B}\""
assert_not_contains "Laptop state excludes desktop client-id" "$l_state" "\"${CLIENT_A}\""
assert_contains "Laptop state has laptop UUID" "$l_state" "$LAPTOP_U1"
assert_not_contains "Laptop state excludes desktop UUID" "$l_state" "$DESKTOP_U1"

# ---- Close tab in A -> B unaffected -------------------------------------- #

e2e_section "1.5 Close tab in desktop -> laptop unaffected (FR-SESSION-10)"

# Desktop closes tab 3 (session kept on server per FR-SESSION-10, but we
# simulate full close by killing the session to test isolation).
e2e_tmux_kill "$DESKTOP_S3"
assert_fail "Desktop session 3 killed" e2e_tmux_has_session "$DESKTOP_S3"

# All laptop sessions still alive.
assert_ok "Laptop session 1 unaffected" e2e_tmux_has_session "$LAPTOP_S1"
assert_ok "Laptop session 2 unaffected" e2e_tmux_has_session "$LAPTOP_S2"

# Laptop lock untouched.
assert_ok "Laptop lock still alive" e2e_lock_exists "$CLIENT_B"

# Laptop state file unchanged.
l_state_after=$(e2e_read_state "$CLIENT_B")
assert_contains "Laptop state still has UUID 1" "$l_state_after" "$LAPTOP_U1"
assert_contains "Laptop state still has UUID 2" "$l_state_after" "$LAPTOP_U2"

# ---- Concurrent interaction ---------------------------------------------- #

e2e_section "1.6 Concurrent terminal interaction"

e2e_ssh_cmd "tmux send-keys -t '${DESKTOP_S1}' 'echo MARKER_DESK_A' Enter"
e2e_ssh_cmd "tmux send-keys -t '${LAPTOP_S1}' 'echo MARKER_LAP_A' Enter"
sleep 0.5

desk_out=$(e2e_ssh_cmd "tmux capture-pane -t '${DESKTOP_S1}' -p" || true)
lap_out=$(e2e_ssh_cmd "tmux capture-pane -t '${LAPTOP_S1}' -p" || true)

assert_contains "Desktop sees its own output" "$desk_out" "MARKER_DESK_A"
assert_not_contains "Desktop does not see laptop output" "$desk_out" "MARKER_LAP_A"

assert_contains "Laptop sees its own output" "$lap_out" "MARKER_LAP_A"
assert_not_contains "Laptop does not see desktop output" "$lap_out" "MARKER_DESK_A"

# ---- Both disconnect -> locks destroyed, sessions alive ------------------ #

e2e_section "1.7 Both disconnect: locks destroyed, sessions alive (FR-LOCK-10)"

e2e_lock_release "$CLIENT_A"
e2e_lock_release "$CLIENT_B"

assert_fail "Desktop lock destroyed" e2e_lock_exists "$CLIENT_A"
assert_fail "Laptop lock destroyed" e2e_lock_exists "$CLIENT_B"

# Data sessions survive lock destruction.
assert_ok "Desktop session 1 survives disconnect" e2e_tmux_has_session "$DESKTOP_S1"
assert_ok "Desktop session 2 survives disconnect" e2e_tmux_has_session "$DESKTOP_S2"
assert_ok "Laptop session 1 survives disconnect" e2e_tmux_has_session "$LAPTOP_S1"
assert_ok "Laptop session 2 survives disconnect" e2e_tmux_has_session "$LAPTOP_S2"

# Remaining sessions: 2 desktop + 2 laptop = 4 (no locks).
remaining=$(e2e_tmux_list | wc -l)
assert_num_eq "4 sessions remain (no locks)" "$remaining" 4

# ---- Summary ------------------------------------------------------------- #

e2e_section "Test 1 complete: simultaneous clients"
e2e_summary
