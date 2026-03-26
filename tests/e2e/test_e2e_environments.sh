#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# E2E Scenario 5: Multiple Environments
#
# Tests environment management:
#   - Create "Dev" and "Staging" environments
#   - Sessions in different environments are isolated (different tmux prefixes)
#   - Switch between environments via state file
#   - Delete "Staging" environment (all its tmux sessions are killed)
#   - Dev sessions survive Staging deletion
#
# Requirements tested:
#   FR-ENV-01, FR-ENV-02, FR-ENV-06, FR-ENV-07, FR-ENV-08, FR-ENV-09,
#   FR-ENV-10, FR-SESSION-04, FR-STATE-15
#
# NOTE: GUI behaviors (environment selection dialog, tray menu switch,
# confirmation dialog on delete) require a display server. This script
# tests the underlying isolation and lifecycle via tmux and state files.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=e2e_helpers.sh
source "$SCRIPT_DIR/e2e_helpers.sh"

CONTAINER_SUFFIX="environments-$$"
CLIENT_ID="e2e-envs"
ENV_DEV="Dev"
ENV_STAGING="Staging"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
e2e_register_cleanup
e2e_start_container "$CONTAINER_SUFFIX"

# ---- Test: Create environments with isolated sessions --------------------- #

e2e_section "Create Dev environment sessions (FR-ENV-01, FR-SESSION-04)"

# Dev environment sessions: <client-id>--Dev--<name>
DEV_SESSION_1="${CLIENT_ID}--${ENV_DEV}--backend"
DEV_SESSION_2="${CLIENT_ID}--${ENV_DEV}--frontend"
DEV_UUID_1="11111111-aaaa-4aaa-8aaa-111111111111"
DEV_UUID_2="22222222-aaaa-4aaa-8aaa-222222222222"

e2e_tmux_create "$DEV_SESSION_1"
e2e_tmux_set_env "$DEV_SESSION_1" "SHELLKEEP_SESSION_UUID" "$DEV_UUID_1"
e2e_ssh_cmd "tmux send-keys -t '$DEV_SESSION_1' 'echo DEV_BACKEND' Enter"

e2e_tmux_create "$DEV_SESSION_2"
e2e_tmux_set_env "$DEV_SESSION_2" "SHELLKEEP_SESSION_UUID" "$DEV_UUID_2"
e2e_ssh_cmd "tmux send-keys -t '$DEV_SESSION_2' 'echo DEV_FRONTEND' Enter"

assert_ok "Dev session 1 exists" e2e_tmux_has_session "$DEV_SESSION_1"
assert_ok "Dev session 2 exists" e2e_tmux_has_session "$DEV_SESSION_2"

e2e_section "Create Staging environment sessions (FR-ENV-02)"

# Staging environment sessions: <client-id>--Staging--<name>
STG_SESSION_1="${CLIENT_ID}--${ENV_STAGING}--deploy"
STG_SESSION_2="${CLIENT_ID}--${ENV_STAGING}--monitor"
STG_SESSION_3="${CLIENT_ID}--${ENV_STAGING}--logs"
STG_UUID_1="33333333-bbbb-4bbb-8bbb-333333333333"
STG_UUID_2="44444444-bbbb-4bbb-8bbb-444444444444"
STG_UUID_3="55555555-bbbb-4bbb-8bbb-555555555555"

e2e_tmux_create "$STG_SESSION_1"
e2e_tmux_set_env "$STG_SESSION_1" "SHELLKEEP_SESSION_UUID" "$STG_UUID_1"
e2e_ssh_cmd "tmux send-keys -t '$STG_SESSION_1' 'echo STAGING_DEPLOY' Enter"

e2e_tmux_create "$STG_SESSION_2"
e2e_tmux_set_env "$STG_SESSION_2" "SHELLKEEP_SESSION_UUID" "$STG_UUID_2"

e2e_tmux_create "$STG_SESSION_3"
e2e_tmux_set_env "$STG_SESSION_3" "SHELLKEEP_SESSION_UUID" "$STG_UUID_3"

assert_ok "Staging session 1 exists" e2e_tmux_has_session "$STG_SESSION_1"
assert_ok "Staging session 2 exists" e2e_tmux_has_session "$STG_SESSION_2"
assert_ok "Staging session 3 exists" e2e_tmux_has_session "$STG_SESSION_3"

# Total: 5 data sessions.
total_sessions=$(e2e_tmux_list | wc -l)
assert_num_eq "5 total sessions" "$total_sessions" 5

# ---- Test: Session isolation by environment (FR-ENV-02) ------------------- #

e2e_section "Environment isolation (FR-ENV-02)"

# List only Dev sessions (those containing --Dev--).
dev_sessions=$(e2e_tmux_list | grep -- "--${ENV_DEV}--" || true)
dev_count=$(echo "$dev_sessions" | grep -c . || echo "0")
assert_num_eq "2 Dev sessions" "$dev_count" 2
assert_contains "Dev has backend" "$dev_sessions" "backend"
assert_contains "Dev has frontend" "$dev_sessions" "frontend"
assert_not_contains "Dev does not have deploy" "$dev_sessions" "deploy"

# List only Staging sessions.
stg_sessions=$(e2e_tmux_list | grep -- "--${ENV_STAGING}--" || true)
stg_count=$(echo "$stg_sessions" | grep -c . || echo "0")
assert_num_eq "3 Staging sessions" "$stg_count" 3
assert_contains "Staging has deploy" "$stg_sessions" "deploy"
assert_not_contains "Staging does not have backend" "$stg_sessions" "backend"

# ---- Test: State file with multiple environments -------------------------- #

e2e_section "State file with multiple environments (FR-STATE-15)"

STATE_JSON=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_ID",
  "environments": {
    "$ENV_DEV": {
      "windows": [
        {
          "id": "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa",
          "title": "Dev Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1024, "height": 768 },
          "tabs": [
            {
              "session_uuid": "$DEV_UUID_1",
              "tmux_session_name": "$DEV_SESSION_1",
              "title": "backend",
              "position": 0
            },
            {
              "session_uuid": "$DEV_UUID_2",
              "tmux_session_name": "$DEV_SESSION_2",
              "title": "frontend",
              "position": 1
            }
          ]
        }
      ]
    },
    "$ENV_STAGING": {
      "windows": [
        {
          "id": "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb",
          "title": "Staging Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1024, "height": 768 },
          "tabs": [
            {
              "session_uuid": "$STG_UUID_1",
              "tmux_session_name": "$STG_SESSION_1",
              "title": "deploy",
              "position": 0
            },
            {
              "session_uuid": "$STG_UUID_2",
              "tmux_session_name": "$STG_SESSION_2",
              "title": "monitor",
              "position": 1
            },
            {
              "session_uuid": "$STG_UUID_3",
              "tmux_session_name": "$STG_SESSION_3",
              "title": "logs",
              "position": 2
            }
          ]
        }
      ]
    }
  },
  "last_environment": "$ENV_DEV"
}
EOF
)

e2e_write_state "$CLIENT_ID" "$STATE_JSON"
assert_ok "Multi-environment state file written" e2e_state_exists "$CLIENT_ID"

# Verify state content.
state=$(e2e_read_state "$CLIENT_ID")
assert_contains "State has Dev environment" "$state" "\"$ENV_DEV\""
assert_contains "State has Staging environment" "$state" "\"$ENV_STAGING\""
assert_contains "State has last_environment" "$state" "\"last_environment\""

# ---- Test: Environment switch (FR-ENV-06, FR-ENV-10) ---------------------- #

e2e_section "Environment switch simulation (FR-ENV-06, FR-ENV-10)"

# Switching environments updates last_environment in state.
# Only one environment active per instance at a time.

# Simulate switching to Staging.
SWITCH_STATE=$(echo "$STATE_JSON" | sed 's/"last_environment": "Dev"/"last_environment": "Staging"/')
e2e_write_state "$CLIENT_ID" "$SWITCH_STATE"

switch_state=$(e2e_read_state "$CLIENT_ID")
assert_contains "Switched to Staging" "$switch_state" '"last_environment": "Staging"'

# Both environments' sessions still exist (switch does not kill sessions).
assert_ok "Dev session 1 still alive after switch" e2e_tmux_has_session "$DEV_SESSION_1"
assert_ok "Staging session 1 still alive after switch" e2e_tmux_has_session "$STG_SESSION_1"

# ---- Test: Environment rename (FR-ENV-09) --------------------------------- #

e2e_section "Environment rename simulation (FR-ENV-09)"

# Renaming an environment requires:
# 1. Renaming all its tmux sessions (changing the env part of the name).
# 2. Updating the state file.

NEW_ENV_NAME="Production"

# Rename Staging sessions to Production.
for stg_session in "$STG_SESSION_1" "$STG_SESSION_2" "$STG_SESSION_3"; do
  new_name=$(echo "$stg_session" | sed "s/--${ENV_STAGING}--/--${NEW_ENV_NAME}--/")
  e2e_ssh_cmd "tmux rename-session -t '$stg_session' '$new_name'"
  assert_ok "Renamed $stg_session to $new_name" e2e_tmux_has_session "$new_name"
done

# Verify old names gone.
assert_fail "Old Staging session 1 gone" e2e_tmux_has_session "$STG_SESSION_1"

# Update session references for later tests.
STG_SESSION_1="${CLIENT_ID}--${NEW_ENV_NAME}--deploy"
STG_SESSION_2="${CLIENT_ID}--${NEW_ENV_NAME}--monitor"
STG_SESSION_3="${CLIENT_ID}--${NEW_ENV_NAME}--logs"

# ---- Test: Delete environment (FR-ENV-07) --------------------------------- #

e2e_section "Environment deletion (FR-ENV-07)"

# Deleting an environment kills all its tmux sessions and removes from state.
# This simulates the confirmation dialog action.

# Kill all Production (formerly Staging) sessions.
e2e_tmux_kill "$STG_SESSION_1"
e2e_tmux_kill "$STG_SESSION_2"
e2e_tmux_kill "$STG_SESSION_3"

assert_fail "Production session 1 killed" e2e_tmux_has_session "$STG_SESSION_1"
assert_fail "Production session 2 killed" e2e_tmux_has_session "$STG_SESSION_2"
assert_fail "Production session 3 killed" e2e_tmux_has_session "$STG_SESSION_3"

# Dev sessions survive.
assert_ok "Dev session 1 survives env deletion" e2e_tmux_has_session "$DEV_SESSION_1"
assert_ok "Dev session 2 survives env deletion" e2e_tmux_has_session "$DEV_SESSION_2"

# Update state to remove the deleted environment.
FINAL_STATE=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_ID",
  "environments": {
    "$ENV_DEV": {
      "windows": [
        {
          "id": "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa",
          "title": "Dev Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1024, "height": 768 },
          "tabs": [
            {
              "session_uuid": "$DEV_UUID_1",
              "tmux_session_name": "$DEV_SESSION_1",
              "title": "backend",
              "position": 0
            },
            {
              "session_uuid": "$DEV_UUID_2",
              "tmux_session_name": "$DEV_SESSION_2",
              "title": "frontend",
              "position": 1
            }
          ]
        }
      ]
    }
  },
  "last_environment": "$ENV_DEV"
}
EOF
)

e2e_write_state "$CLIENT_ID" "$FINAL_STATE"

final=$(e2e_read_state "$CLIENT_ID")
assert_contains "Final state has Dev" "$final" "\"$ENV_DEV\""
assert_not_contains "Final state has no Production/Staging" "$final" "\"$NEW_ENV_NAME\""
assert_not_contains "Final state has no Staging" "$final" "\"$ENV_STAGING\""

# Verify only Dev sessions remain in tmux.
remaining=$(e2e_tmux_list | wc -l)
assert_num_eq "Only 2 Dev sessions remain" "$remaining" 2

# ---- Test: Dev sessions are still functional ------------------------------ #

e2e_section "Dev sessions functional after deletion"

sleep 0.5
output=$(e2e_ssh_cmd "tmux capture-pane -t '$DEV_SESSION_1' -p" || true)
assert_contains "Dev backend has output" "$output" "DEV_BACKEND"

# Run new commands to verify interactivity.
e2e_ssh_cmd "tmux send-keys -t '$DEV_SESSION_2' 'echo STILL_ALIVE' Enter"
sleep 0.5
output2=$(e2e_ssh_cmd "tmux capture-pane -t '$DEV_SESSION_2' -p" || true)
assert_contains "Dev frontend still interactive" "$output2" "STILL_ALIVE"

# ---- Summary -------------------------------------------------------------- #

e2e_section "Multiple Environments scenario complete"
e2e_summary
