#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# E2E Scenario 6: Multi-Client Isolation
#
# Tests multiple clients with different client-IDs operating simultaneously:
#   - Client "desktop" with 3 tabs
#   - Client "laptop" with 2 tabs
#   - Verify session isolation (different tmux prefixes)
#   - Verify separate state files
#   - Verify independent locks
#   - Verify one client's actions do not affect the other
#
# Requirements tested:
#   FR-LOCK-01, FR-LOCK-02, FR-SESSION-04, FR-STATE-01, FR-STATE-02,
#   FR-ENV-02
#
# NOTE: Testing two actual shellkeep GUI instances simultaneously requires
# two X11 displays. This script tests the underlying isolation via tmux
# sessions, state files, and lock sessions.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=e2e_helpers.sh
source "$SCRIPT_DIR/e2e_helpers.sh"

CONTAINER_SUFFIX="multi-client-$$"
CLIENT_DESKTOP="desktop"
CLIENT_LAPTOP="laptop"
ENV_NAME="Default"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
e2e_register_cleanup
e2e_start_container "$CONTAINER_SUFFIX"

# ---- Test: Both clients acquire independent locks ------------------------- #

e2e_section "Independent lock acquisition (FR-LOCK-01, FR-LOCK-02)"

e2e_lock_acquire "$CLIENT_DESKTOP" "desktop-host"
e2e_lock_acquire "$CLIENT_LAPTOP" "laptop-host"

assert_ok "Desktop lock exists" e2e_lock_exists "$CLIENT_DESKTOP"
assert_ok "Laptop lock exists" e2e_lock_exists "$CLIENT_LAPTOP"

# Verify locks are independent.
desktop_lock_host=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_DESKTOP}" "SHELLKEEP_LOCK_HOSTNAME")
laptop_lock_host=$(e2e_tmux_get_env "${E2E_LOCK_PREFIX}${CLIENT_LAPTOP}" "SHELLKEEP_LOCK_HOSTNAME")

assert_eq "Desktop lock hostname" "$desktop_lock_host" "desktop-host"
assert_eq "Laptop lock hostname" "$laptop_lock_host" "laptop-host"

# ---- Test: Desktop creates 3 sessions ------------------------------------ #

e2e_section "Desktop client: 3 sessions (FR-SESSION-04)"

DESKTOP_SESSIONS=()
DESKTOP_UUIDS=()
for i in 1 2 3; do
  sname="${CLIENT_DESKTOP}--${ENV_NAME}--desk-tab-${i}"
  suuid="$(printf 'dddddddd-dddd-4ddd-8ddd-%012d' "$i")"
  e2e_tmux_create "$sname"
  e2e_tmux_set_env "$sname" "SHELLKEEP_SESSION_UUID" "$suuid"
  e2e_ssh_cmd "tmux send-keys -t '$sname' 'echo DESKTOP_${i}' Enter"
  DESKTOP_SESSIONS+=("$sname")
  DESKTOP_UUIDS+=("$suuid")
done

for i in 0 1 2; do
  assert_ok "Desktop session $((i+1)) exists" e2e_tmux_has_session "${DESKTOP_SESSIONS[$i]}"
done

# ---- Test: Laptop creates 2 sessions ------------------------------------- #

e2e_section "Laptop client: 2 sessions (FR-SESSION-04)"

LAPTOP_SESSIONS=()
LAPTOP_UUIDS=()
for i in 1 2; do
  sname="${CLIENT_LAPTOP}--${ENV_NAME}--lap-tab-${i}"
  suuid="$(printf 'llllllll-llll-4lll-8lll-%012d' "$i")"
  e2e_tmux_create "$sname"
  e2e_tmux_set_env "$sname" "SHELLKEEP_SESSION_UUID" "$suuid"
  e2e_ssh_cmd "tmux send-keys -t '$sname' 'echo LAPTOP_${i}' Enter"
  LAPTOP_SESSIONS+=("$sname")
  LAPTOP_UUIDS+=("$suuid")
done

for i in 0 1; do
  assert_ok "Laptop session $((i+1)) exists" e2e_tmux_has_session "${LAPTOP_SESSIONS[$i]}"
done

# ---- Test: Session isolation ---------------------------------------------- #

e2e_section "Session isolation by client-id (FR-ENV-02)"

# Total sessions: 3 desktop + 2 laptop + 2 locks = 7.
total=$(e2e_tmux_list | wc -l)
assert_num_eq "7 total tmux sessions" "$total" 7

# Desktop sessions: filter by client-id prefix.
desktop_list=$(e2e_tmux_list | grep "^${CLIENT_DESKTOP}--" || true)
desktop_count=$(echo "$desktop_list" | grep -c . || echo "0")
assert_num_eq "3 desktop data sessions" "$desktop_count" 3

# Laptop sessions.
laptop_list=$(e2e_tmux_list | grep "^${CLIENT_LAPTOP}--" || true)
laptop_count=$(echo "$laptop_list" | grep -c . || echo "0")
assert_num_eq "2 laptop data sessions" "$laptop_count" 2

# Desktop sessions do not contain laptop sessions.
assert_not_contains "Desktop list has no laptop sessions" "$desktop_list" "lap-tab"
assert_not_contains "Laptop list has no desktop sessions" "$laptop_list" "desk-tab"

# ---- Test: Separate state files (FR-STATE-01, FR-STATE-02) ---------------- #

e2e_section "Separate state files (FR-STATE-01)"

# Desktop state file.
DESKTOP_STATE=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_DESKTOP",
  "environments": {
    "$ENV_NAME": {
      "windows": [
        {
          "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          "title": "Desktop Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
          "tabs": [
            {
              "session_uuid": "${DESKTOP_UUIDS[0]}",
              "tmux_session_name": "${DESKTOP_SESSIONS[0]}",
              "title": "desk-tab-1",
              "position": 0
            },
            {
              "session_uuid": "${DESKTOP_UUIDS[1]}",
              "tmux_session_name": "${DESKTOP_SESSIONS[1]}",
              "title": "desk-tab-2",
              "position": 1
            },
            {
              "session_uuid": "${DESKTOP_UUIDS[2]}",
              "tmux_session_name": "${DESKTOP_SESSIONS[2]}",
              "title": "desk-tab-3",
              "position": 2
            }
          ]
        }
      ]
    }
  },
  "last_environment": "$ENV_NAME"
}
EOF
)

LAPTOP_STATE=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_LAPTOP",
  "environments": {
    "$ENV_NAME": {
      "windows": [
        {
          "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          "title": "Laptop Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1366, "height": 768 },
          "tabs": [
            {
              "session_uuid": "${LAPTOP_UUIDS[0]}",
              "tmux_session_name": "${LAPTOP_SESSIONS[0]}",
              "title": "lap-tab-1",
              "position": 0
            },
            {
              "session_uuid": "${LAPTOP_UUIDS[1]}",
              "tmux_session_name": "${LAPTOP_SESSIONS[1]}",
              "title": "lap-tab-2",
              "position": 1
            }
          ]
        }
      ]
    }
  },
  "last_environment": "$ENV_NAME"
}
EOF
)

e2e_write_state "$CLIENT_DESKTOP" "$DESKTOP_STATE"
e2e_write_state "$CLIENT_LAPTOP" "$LAPTOP_STATE"

assert_ok "Desktop state file exists" e2e_state_exists "$CLIENT_DESKTOP"
assert_ok "Laptop state file exists" e2e_state_exists "$CLIENT_LAPTOP"

# Verify files are separate.
state_file_count=$(e2e_ssh_cmd "ls '$E2E_REMOTE_STATE_DIR'/*.json 2>/dev/null | wc -l")
assert_num_eq "2 separate state files" "$state_file_count" 2

# Verify content isolation.
desktop_state=$(e2e_read_state "$CLIENT_DESKTOP")
laptop_state=$(e2e_read_state "$CLIENT_LAPTOP")

assert_contains "Desktop state has desktop client-id" "$desktop_state" "\"$CLIENT_DESKTOP\""
assert_not_contains "Desktop state has no laptop client-id" "$desktop_state" "\"$CLIENT_LAPTOP\""

assert_contains "Laptop state has laptop client-id" "$laptop_state" "\"$CLIENT_LAPTOP\""
assert_not_contains "Laptop state has no desktop client-id" "$laptop_state" "\"$CLIENT_DESKTOP\""

assert_contains "Desktop state has desktop UUIDs" "$desktop_state" "${DESKTOP_UUIDS[0]}"
assert_not_contains "Desktop state has no laptop UUIDs" "$desktop_state" "${LAPTOP_UUIDS[0]}"

# ---- Test: One client's actions do not affect the other ------------------- #

e2e_section "Cross-client isolation: desktop actions"

# Kill one of desktop's sessions. Laptop should be unaffected.
e2e_tmux_kill "${DESKTOP_SESSIONS[2]}"
assert_fail "Desktop session 3 killed" e2e_tmux_has_session "${DESKTOP_SESSIONS[2]}"

# Laptop sessions still intact.
for i in 0 1; do
  assert_ok "Laptop session $((i+1)) survives desktop kill" \
    e2e_tmux_has_session "${LAPTOP_SESSIONS[$i]}"
done

# Laptop state file unchanged.
laptop_state_after=$(e2e_read_state "$CLIENT_LAPTOP")
assert_contains "Laptop state unchanged" "$laptop_state_after" "${LAPTOP_UUIDS[0]}"
assert_contains "Laptop state unchanged" "$laptop_state_after" "${LAPTOP_UUIDS[1]}"

e2e_section "Cross-client isolation: laptop actions"

# Laptop releases its lock. Desktop lock should be unaffected.
e2e_lock_release "$CLIENT_LAPTOP"
assert_fail "Laptop lock released" e2e_lock_exists "$CLIENT_LAPTOP"
assert_ok "Desktop lock still exists" e2e_lock_exists "$CLIENT_DESKTOP"

# Desktop sessions still intact.
assert_ok "Desktop session 1 survives laptop lock release" \
  e2e_tmux_has_session "${DESKTOP_SESSIONS[0]}"
assert_ok "Desktop session 2 survives laptop lock release" \
  e2e_tmux_has_session "${DESKTOP_SESSIONS[1]}"

# ---- Test: Concurrent interactions --------------------------------------- #

e2e_section "Concurrent session operations"

# Both clients interact with their sessions at the same time.
e2e_ssh_cmd "tmux send-keys -t '${DESKTOP_SESSIONS[0]}' 'echo CONCURRENT_DESKTOP' Enter"
e2e_ssh_cmd "tmux send-keys -t '${LAPTOP_SESSIONS[0]}' 'echo CONCURRENT_LAPTOP' Enter"

sleep 0.5

desk_out=$(e2e_ssh_cmd "tmux capture-pane -t '${DESKTOP_SESSIONS[0]}' -p" || true)
lap_out=$(e2e_ssh_cmd "tmux capture-pane -t '${LAPTOP_SESSIONS[0]}' -p" || true)

assert_contains "Desktop session shows desktop output" "$desk_out" "CONCURRENT_DESKTOP"
assert_not_contains "Desktop session has no laptop output" "$desk_out" "CONCURRENT_LAPTOP"

assert_contains "Laptop session shows laptop output" "$lap_out" "CONCURRENT_LAPTOP"
assert_not_contains "Laptop session has no desktop output" "$lap_out" "CONCURRENT_DESKTOP"

# ---- Test: Independent state updates ------------------------------------- #

e2e_section "Independent state file updates"

# Update desktop state without affecting laptop state.
UPDATED_DESKTOP=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_DESKTOP",
  "environments": {
    "$ENV_NAME": {
      "windows": [
        {
          "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          "title": "Desktop Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
          "tabs": [
            {
              "session_uuid": "${DESKTOP_UUIDS[0]}",
              "tmux_session_name": "${DESKTOP_SESSIONS[0]}",
              "title": "desk-tab-1",
              "position": 0
            },
            {
              "session_uuid": "${DESKTOP_UUIDS[1]}",
              "tmux_session_name": "${DESKTOP_SESSIONS[1]}",
              "title": "desk-tab-2",
              "position": 1
            }
          ]
        }
      ]
    }
  },
  "last_environment": "$ENV_NAME"
}
EOF
)

e2e_write_state "$CLIENT_DESKTOP" "$UPDATED_DESKTOP"

# Verify desktop state updated (2 tabs now instead of 3).
updated_desktop=$(e2e_read_state "$CLIENT_DESKTOP")
assert_not_contains "Desktop state no longer has tab 3" "$updated_desktop" "${DESKTOP_UUIDS[2]}"

# Verify laptop state untouched.
laptop_final=$(e2e_read_state "$CLIENT_LAPTOP")
assert_contains "Laptop state still has all UUIDs" "$laptop_final" "${LAPTOP_UUIDS[0]}"
assert_contains "Laptop state still has all UUIDs" "$laptop_final" "${LAPTOP_UUIDS[1]}"

# ---- Cleanup desktop lock ------------------------------------------------ #

e2e_lock_release "$CLIENT_DESKTOP"

# ---- Summary -------------------------------------------------------------- #

e2e_section "Multi-Client scenario complete"
e2e_summary
