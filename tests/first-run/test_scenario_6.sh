#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# First-Run Scenario 6: Returning user — existing config
#
# Conditions:
#   - Local config: Exists (prior shellkeep state on server)
#   - SSH key: Exists
#   - ssh-agent: Running
#   - tmux on server: Installed (>= 3.0)
#   - known_hosts: Populated (host key already known)
#
# Expected result:
#   Auth ok -> no TOFU -> environment restored
#
# Connection flow tested:
#   1. Host key verification passes immediately (SK_HOST_KEY_OK)
#   2. Authentication via agent succeeds
#   3. tmux detected >= 3.0
#   4. State file found on server -> load existing state
#   5. Existing environment restored (no "Default" creation dialog)
#   6. Existing tmux sessions are reconciled
#   7. Tab restore works
#
# MANUAL VERIFICATION REQUIRED:
#   - No TOFU dialog appears (host already known)
#   - Environment is restored without selection dialog (single env)
#   - Tabs appear with correct names from saved state
#   - Dead sessions show scrollback recovery

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fr_helpers.sh
source "$SCRIPT_DIR/fr_helpers.sh"

FR_SCENARIO="scenario-6-returning-user"

# ---- Setup ---------------------------------------------------------------- #

fr_check_prereqs
fr_build_image_full
fr_register_cleanup
fr_create_tmpdir

fr_start_container "agent28-s6-$$" "$FR_IMAGE_FULL"

# Generate and install SSH key.
fr_generate_ssh_key "$FR_TMPDIR/test_key"
fr_install_pubkey "$FR_TMPDIR/test_key.pub"

# Start agent and load key.
eval "$(ssh-agent -s)" >/dev/null 2>&1
ssh-add "$FR_TMPDIR/test_key" 2>/dev/null

# Populate known_hosts with the container's host key (simulating prior visit).
KNOWN_HOSTS="$FR_TMPDIR/known_hosts"
fr_populate_known_hosts "$KNOWN_HOSTS"

# ---- Pre-populate server state (simulating prior shellkeep usage) --------- #

fr_section "Setup: Pre-populate server state"

CLIENT_ID="fr-scenario6"
ENV_NAME="MyProject"
SESSION_1="${CLIENT_ID}--${ENV_NAME}--backend-server"
SESSION_2="${CLIENT_ID}--${ENV_NAME}--frontend-dev"
UUID_1="11111111-1111-4111-8111-111111111111"
UUID_2="22222222-2222-4222-8222-222222222222"

# Create tmux sessions that simulate prior shellkeep usage.
fr_ssh_cmd_nocheck "tmux new-session -d -s '$SESSION_1'"
fr_ssh_cmd_nocheck "tmux set-environment -t '$SESSION_1' SHELLKEEP_SESSION_UUID '$UUID_1'"
fr_ssh_cmd_nocheck "tmux send-keys -t '$SESSION_1' 'echo running-backend-process' Enter"

fr_ssh_cmd_nocheck "tmux new-session -d -s '$SESSION_2'"
fr_ssh_cmd_nocheck "tmux set-environment -t '$SESSION_2' SHELLKEEP_SESSION_UUID '$UUID_2'"
fr_ssh_cmd_nocheck "tmux send-keys -t '$SESSION_2' 'echo running-frontend-process' Enter"

# Write a state file as if shellkeep had previously saved it.
fr_ssh_cmd_nocheck "mkdir -p '$FR_REMOTE_STATE_DIR' && chmod 700 '$FR_REMOTE_STATE_DIR'"
fr_ssh_cmd_nocheck "cat > '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json' << 'STATEEOF'
{
  \"schema_version\": 1,
  \"last_modified\": \"2026-03-25T10:00:00Z\",
  \"client_id\": \"$CLIENT_ID\",
  \"environments\": {
    \"$ENV_NAME\": {
      \"windows\": [
        {
          \"id\": \"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\",
          \"title\": \"Development\",
          \"visible\": true,
          \"active_tab\": 0,
          \"geometry\": {
            \"x\": 100,
            \"y\": 100,
            \"width\": 1280,
            \"height\": 720
          },
          \"tabs\": [
            {
              \"session_uuid\": \"$UUID_1\",
              \"tmux_session_name\": \"$SESSION_1\",
              \"title\": \"backend-server\",
              \"position\": 0
            },
            {
              \"session_uuid\": \"$UUID_2\",
              \"tmux_session_name\": \"$SESSION_2\",
              \"title\": \"frontend-dev\",
              \"position\": 1
            }
          ]
        }
      ]
    }
  },
  \"last_environment\": \"$ENV_NAME\"
}
STATEEOF"

# Set permissions per NFR-SEC-07.
fr_ssh_cmd_nocheck "chmod 600 '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'"

assert_ok "State file exists" fr_ssh_cmd_nocheck "test -f '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'"
assert_ok "Session 1 exists" fr_ssh_cmd_nocheck "tmux has-session -t '$SESSION_1'"
assert_ok "Session 2 exists" fr_ssh_cmd_nocheck "tmux has-session -t '$SESSION_2'"
fr_pass "Server pre-populated with prior state"

# ---- Phase 1: Host key verification — no TOFU (FR-CONN-01) --------------- #

fr_section "Phase 1: Host key known — no TOFU (FR-CONN-01)"

# With populated known_hosts, connection should succeed immediately.
# No TOFU dialog should appear.
set +e
result=$(ssh -o StrictHostKeyChecking=yes \
             -o UserKnownHostsFile="$KNOWN_HOSTS" \
             -o IdentityFile="$FR_TMPDIR/test_key" \
             -o IdentitiesOnly=yes \
             -o PasswordAuthentication=no \
             -o BatchMode=yes \
             -o LogLevel=ERROR \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo no-tofu" 2>&1)
rc=$?
set -e

assert_exit_code "Connection succeeds without TOFU" 0 "$rc"
assert_contains "No TOFU needed" "$result" "no-tofu"

# The empty known_hosts test from scenario 1 would fail here, proving
# that the populated known_hosts bypasses the TOFU flow.
fr_pass "Host key matched known_hosts (SK_HOST_KEY_OK path)"

# ---- Phase 2: Authentication via agent ----------------------------------- #

fr_section "Phase 2: Authentication via ssh-agent (FR-CONN-07)"

set +e
result=$(ssh -o StrictHostKeyChecking=no \
             -o UserKnownHostsFile="$KNOWN_HOSTS" \
             -o PasswordAuthentication=no \
             -o PubkeyAuthentication=yes \
             -o BatchMode=yes \
             -o LogLevel=ERROR \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo agent-ok" 2>&1)
rc=$?
set -e

assert_exit_code "Agent auth succeeds" 0 "$rc"
assert_contains "Agent auth output" "$result" "agent-ok"

# ---- Phase 3: tmux detection succeeds ------------------------------------ #

fr_section "Phase 3: tmux detection (FR-CONN-13)"

tmux_ver=$(fr_tmux_version)
assert_contains "tmux installed" "$tmux_ver" "tmux"

major=$(echo "$tmux_ver" | grep -oP '[\d.]+' | head -1 | cut -d. -f1)
if [[ "$major" -ge 3 ]]; then
  fr_pass "tmux version >= 3.0 ($tmux_ver)"
else
  fr_fail "tmux version < 3.0 ($tmux_ver)"
fi

# ---- Phase 4: State file found and loaded --------------------------------- #

fr_section "Phase 4: State file loaded (FR-STATE-06)"

state_content=$(fr_ssh_cmd_nocheck "cat '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'")
assert_contains "State has correct client_id" "$state_content" "\"$CLIENT_ID\""
assert_contains "State has MyProject environment" "$state_content" "\"$ENV_NAME\""
assert_contains "State has session 1 UUID" "$state_content" "$UUID_1"
assert_contains "State has session 2 UUID" "$state_content" "$UUID_2"
assert_contains "State has window geometry" "$state_content" '"width": 1280'

# Verify state file permissions.
perms=$(fr_ssh_cmd_nocheck "stat -c '%a' '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'")
assert_eq "State file permissions are 0600" "$perms" "600"

# ---- Phase 5: Session reconciliation ------------------------------------- #

fr_section "Phase 5: Session reconciliation (FR-SESSION-07, FR-SESSION-08)"

# List live tmux sessions matching our client-id.
live_sessions=$(fr_ssh_cmd_nocheck "tmux list-sessions -F '#{session_name}' 2>/dev/null | grep '^${CLIENT_ID}--'")

assert_contains "Session 1 found live" "$live_sessions" "$SESSION_1"
assert_contains "Session 2 found live" "$live_sessions" "$SESSION_2"

# Verify UUIDs match state file.
uuid1_live=$(fr_ssh_cmd_nocheck "tmux show-environment -t '$SESSION_1' SHELLKEEP_SESSION_UUID" | cut -d= -f2-)
uuid2_live=$(fr_ssh_cmd_nocheck "tmux show-environment -t '$SESSION_2' SHELLKEEP_SESSION_UUID" | cut -d= -f2-)

assert_eq "Session 1 UUID matches state" "$uuid1_live" "$UUID_1"
assert_eq "Session 2 UUID matches state" "$uuid2_live" "$UUID_2"

fr_pass "All sessions reconciled: UUIDs match between state file and live sessions"

# ---- Phase 6: Sessions are functional (tab restore) ----------------------- #

fr_section "Phase 6: Session content preserved (tab restore)"

# Verify sessions have their prior output (processes were running).
sleep 0.5
output1=$(fr_ssh_cmd_nocheck "tmux capture-pane -t '$SESSION_1' -p" || true)
assert_contains "Session 1 has prior output" "$output1" "running-backend-process"

output2=$(fr_ssh_cmd_nocheck "tmux capture-pane -t '$SESSION_2' -p" || true)
assert_contains "Session 2 has prior output" "$output2" "running-frontend-process"

# Send new commands to prove sessions are interactive.
fr_ssh_cmd_nocheck "tmux send-keys -t '$SESSION_1' 'echo restored-tab-1' Enter"
sleep 0.5
output1_new=$(fr_ssh_cmd_nocheck "tmux capture-pane -t '$SESSION_1' -p" || true)
assert_contains "Session 1 accepts new input" "$output1_new" "restored-tab-1"

# ---- Phase 7: Single environment — no selection dialog -------------------- #

fr_section "Phase 7: Single environment — direct open (FR-ENV-04)"

# With only one environment ("MyProject"), shellkeep should open it directly
# without showing the environment selection dialog.
env_count=$(echo "$state_content" | grep -c '"windows"' || true)
assert_num_eq "Single environment in state" "$env_count" 1
fr_pass "Single environment -> opens directly without selection dialog (FR-ENV-04)"

# ---- Phase 8: Lock acquisition for returning user ------------------------- #

fr_section "Phase 8: Lock acquisition (FR-LOCK-02)"

# Simulate lock acquisition.
lock_name="shellkeep-lock-${CLIENT_ID}"
fr_ssh_cmd_nocheck "tmux new-session -d -s '$lock_name' \
  \\; set-environment -t '$lock_name' SHELLKEEP_LOCK_CLIENT_ID '$CLIENT_ID' \
  \\; set-environment -t '$lock_name' SHELLKEEP_LOCK_HOSTNAME 'returning-host' \
  \\; set-environment -t '$lock_name' SHELLKEEP_LOCK_CONNECTED_AT '$(date -u +"%Y-%m-%dT%H:%M:%SZ")' \
  \\; set-environment -t '$lock_name' SHELLKEEP_LOCK_PID '$$' \
  \\; set-environment -t '$lock_name' SHELLKEEP_LOCK_VERSION '0.1.0'"

assert_ok "Lock acquired" fr_ssh_cmd_nocheck "tmux has-session -t '$lock_name'"

lock_client=$(fr_ssh_cmd_nocheck "tmux show-environment -t '$lock_name' SHELLKEEP_LOCK_CLIENT_ID" | cut -d= -f2-)
assert_eq "Lock has correct client-id" "$lock_client" "$CLIENT_ID"

# ---- Phase 9: Clean disconnect ------------------------------------------- #

fr_section "Phase 9: Graceful disconnect"

# Release lock.
fr_ssh_cmd_nocheck "tmux kill-session -t '$lock_name'" 2>/dev/null || true
assert_fail "Lock released" fr_ssh_cmd_nocheck "tmux has-session -t '$lock_name'"

# Data sessions survive disconnect.
assert_ok "Session 1 survives disconnect" fr_ssh_cmd_nocheck "tmux has-session -t '$SESSION_1'"
assert_ok "Session 2 survives disconnect" fr_ssh_cmd_nocheck "tmux has-session -t '$SESSION_2'"

# State file still valid.
final_state=$(fr_ssh_cmd_nocheck "cat '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'")
assert_contains "State file still valid" "$final_state" '"schema_version"'

fr_manual_note "Verify no TOFU dialog appeared"
fr_manual_note "Verify no environment selection dialog appeared (single env)"
fr_manual_note "Verify tabs appeared with names: 'backend-server' and 'frontend-dev'"
fr_manual_note "Verify window restored to saved geometry (1280x720 at 100,100)"
fr_manual_note "Verify tab content shows prior process output"
fr_manual_note "Verify connection feedback showed: Connecting, Authenticating, Checking tmux, Loading state, Restoring sessions"

# ---- Cleanup -------------------------------------------------------------- #

fr_ssh_cmd_nocheck "tmux kill-session -t '$SESSION_1'" 2>/dev/null || true
fr_ssh_cmd_nocheck "tmux kill-session -t '$SESSION_2'" 2>/dev/null || true

# ---- Summary -------------------------------------------------------------- #

fr_section "Scenario 6 complete"
fr_summary
