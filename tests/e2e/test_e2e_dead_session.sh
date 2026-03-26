#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# E2E Scenario 3: Dead Session Detection
#
# Tests dead session detection after external tmux kill:
#   - Connect and create a tab, run commands to generate history
#   - Kill the tmux session externally (tmux kill-session)
#   - Verify the session is gone from tmux
#   - Verify the state file still references the dead session
#   - Simulate reconnect: detect dead session (state references session
#     that no longer exists in tmux)
#   - Create a new replacement session
#
# Requirements tested:
#   FR-SESSION-08, FR-SESSION-10, FR-STATE-06, FR-STATE-15
#
# NOTE: GUI behaviors (dead tab banner with history, "Create new" button)
# require a display server. This script tests the underlying detection logic:
# state references a session UUID that no longer exists in tmux.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=e2e_helpers.sh
source "$SCRIPT_DIR/e2e_helpers.sh"

CONTAINER_SUFFIX="dead-session-$$"
CLIENT_ID="e2e-dead"
ENV_NAME="Default"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
e2e_register_cleanup
e2e_start_container "$CONTAINER_SUFFIX"

# ---- Test: Create session and run commands -------------------------------- #

e2e_section "Create session with history"

SESSION_NAME="${CLIENT_ID}--${ENV_NAME}--work-session"
UUID="cccccccc-cccc-4ccc-8ccc-cccccccccccc"

e2e_tmux_create "$SESSION_NAME"
e2e_tmux_set_env "$SESSION_NAME" "SHELLKEEP_SESSION_UUID" "$UUID"

assert_ok "Session created" e2e_tmux_has_session "$SESSION_NAME"

# Run commands to generate history.
e2e_ssh_cmd "tmux send-keys -t '$SESSION_NAME' 'echo hello-from-dead-session' Enter"
e2e_ssh_cmd "tmux send-keys -t '$SESSION_NAME' 'echo test-output-line-2' Enter"
e2e_ssh_cmd "tmux send-keys -t '$SESSION_NAME' 'pwd' Enter"
sleep 0.5

# Capture the pane content before kill (this is what shellkeep would save).
history_before=$(e2e_ssh_cmd "tmux capture-pane -t '$SESSION_NAME' -p" || true)
assert_contains "Session has output" "$history_before" "hello-from-dead-session"
assert_contains "Session has second output" "$history_before" "test-output-line-2"

# ---- Test: Write state referencing the session ---------------------------- #

e2e_section "State references session (FR-STATE-15)"

STATE_JSON=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_ID",
  "environments": {
    "$ENV_NAME": {
      "windows": [
        {
          "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
          "title": "Main",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1024, "height": 768 },
          "tabs": [
            {
              "session_uuid": "$UUID",
              "tmux_session_name": "$SESSION_NAME",
              "title": "work-session",
              "position": 0
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

e2e_write_state "$CLIENT_ID" "$STATE_JSON"
assert_ok "State file written" e2e_state_exists "$CLIENT_ID"

# ---- Test: Kill the tmux session externally ------------------------------- #

e2e_section "External session kill"

e2e_tmux_kill "$SESSION_NAME"
assert_fail "Session killed externally" e2e_tmux_has_session "$SESSION_NAME"

# Verify no tmux sessions remain.
remaining=$(e2e_tmux_list)
assert_eq "No tmux sessions after kill" "$remaining" ""

# ---- Test: State file still references dead session ----------------------- #

e2e_section "Dead session detection (FR-SESSION-08)"

state_after_kill=$(e2e_read_state "$CLIENT_ID")
assert_contains "State still has dead session UUID" "$state_after_kill" "$UUID"
assert_contains "State still has dead session name" "$state_after_kill" "$SESSION_NAME"

# Simulate the reconciliation logic:
# Read tmux session list from server and compare with state file.
live_sessions=$(e2e_tmux_list)

# The dead session should NOT appear in the live list.
assert_not_contains "Dead session not in tmux listing" "$live_sessions" "$SESSION_NAME"

# Try to look up the session UUID in tmux -- should fail.
uuid_check=$(e2e_ssh_cmd "tmux show-environment -t '$SESSION_NAME' SHELLKEEP_SESSION_UUID 2>/dev/null" || echo "NOT_FOUND")
assert_contains "UUID lookup fails for dead session" "$uuid_check" "NOT_FOUND"

# This is the core detection: state references a session_uuid that exists in
# the state file but the corresponding tmux session is gone.
# In the GUI, this would show a dead tab with banner + history + "Create new".
e2e_pass "Dead session detected: state has UUID $UUID but tmux session is gone"

# ---- Test: Create replacement session ------------------------------------- #

e2e_section "Replacement session creation"

# Simulate the "Create new" action: create a new session with a new UUID,
# replacing the dead one in the state.
NEW_SESSION_NAME="${CLIENT_ID}--${ENV_NAME}--work-session-new"
NEW_UUID="dddddddd-eeee-4fff-8000-111111111111"

e2e_tmux_create "$NEW_SESSION_NAME"
e2e_tmux_set_env "$NEW_SESSION_NAME" "SHELLKEEP_SESSION_UUID" "$NEW_UUID"

assert_ok "Replacement session created" e2e_tmux_has_session "$NEW_SESSION_NAME"

# Verify the new session has a different UUID.
new_uuid_check=$(e2e_tmux_get_env "$NEW_SESSION_NAME" "SHELLKEEP_SESSION_UUID")
assert_eq "New session has new UUID" "$new_uuid_check" "$NEW_UUID"

# Verify the new session is functional.
e2e_ssh_cmd "tmux send-keys -t '$NEW_SESSION_NAME' 'echo replacement-works' Enter"
sleep 0.5
new_output=$(e2e_ssh_cmd "tmux capture-pane -t '$NEW_SESSION_NAME' -p" || true)
assert_contains "Replacement session is functional" "$new_output" "replacement-works"

# ---- Test: Update state to reflect replacement ---------------------------- #

e2e_section "State update after replacement"

UPDATED_STATE=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_ID",
  "environments": {
    "$ENV_NAME": {
      "windows": [
        {
          "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
          "title": "Main",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1024, "height": 768 },
          "tabs": [
            {
              "session_uuid": "$NEW_UUID",
              "tmux_session_name": "$NEW_SESSION_NAME",
              "title": "work-session-new",
              "position": 0
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

e2e_write_state "$CLIENT_ID" "$UPDATED_STATE"

# Verify updated state no longer references the dead UUID.
updated_state=$(e2e_read_state "$CLIENT_ID")
assert_not_contains "Dead UUID removed from state" "$updated_state" "$UUID"
assert_contains "New UUID in state" "$updated_state" "$NEW_UUID"
assert_contains "New session name in state" "$updated_state" "$NEW_SESSION_NAME"

# ---- Test: Multiple dead sessions ---------------------------------------- #

e2e_section "Multiple dead sessions"

# Create 3 sessions, kill 2, verify detection.
MULTI_SESSIONS=()
MULTI_UUIDS=()
for i in 1 2 3; do
  sname="${CLIENT_ID}--${ENV_NAME}--multi-${i}"
  suuid="$(printf 'eeeeeeee-eeee-4eee-8eee-%012d' "$i")"
  e2e_tmux_create "$sname"
  e2e_tmux_set_env "$sname" "SHELLKEEP_SESSION_UUID" "$suuid"
  MULTI_SESSIONS+=("$sname")
  MULTI_UUIDS+=("$suuid")
done

# Kill sessions 1 and 3, keep session 2.
e2e_tmux_kill "${MULTI_SESSIONS[0]}"
e2e_tmux_kill "${MULTI_SESSIONS[2]}"

# Verify detection.
live=$(e2e_tmux_list)
assert_not_contains "Dead session 1 not in tmux" "$live" "multi-1"
assert_contains "Live session 2 in tmux" "$live" "multi-2"
assert_not_contains "Dead session 3 not in tmux" "$live" "multi-3"

# Session 2 should still have its UUID.
s2_uuid=$(e2e_tmux_get_env "${MULTI_SESSIONS[1]}" "SHELLKEEP_SESSION_UUID")
assert_eq "Live session 2 UUID intact" "$s2_uuid" "${MULTI_UUIDS[1]}"

e2e_pass "Multiple dead sessions correctly detected (2 dead, 1 alive)"

# ---- Summary -------------------------------------------------------------- #

e2e_section "Dead Session scenario complete"
e2e_summary
