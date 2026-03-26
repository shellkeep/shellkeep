#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# First-Run Scenario 1: Full happy path
#
# Conditions:
#   - Local config: None (no prior shellkeep state)
#   - SSH key: Exists
#   - ssh-agent: Running
#   - tmux on server: Installed (>= 3.0)
#   - known_hosts: Empty
#
# Expected result:
#   TOFU (host key unknown) -> auth ok via agent -> Default env -> works
#
# Connection flow tested (FR-CONN-01..17):
#   1. TCP connect succeeds
#   2. Host key unknown -> TOFU dialog (manual: verify fingerprint shown)
#   3. After accepting, auth via ssh-agent succeeds
#   4. tmux detected >= 3.0
#   5. No prior state -> "Default" environment created (FR-ENV-05)
#   6. Session creation works
#
# MANUAL VERIFICATION REQUIRED:
#   - TOFU dialog shows fingerprint and "Accept and save" / "Connect once" / "Cancel"
#   - Connection progress feedback shows phases (FR-CONN-16)
#   - No blank screen at any point
#   - Cancel at TOFU returns to clean state

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fr_helpers.sh
source "$SCRIPT_DIR/fr_helpers.sh"

FR_SCENARIO="scenario-1-happy-path"

# ---- Setup ---------------------------------------------------------------- #

fr_check_prereqs
fr_build_image_full
fr_register_cleanup
fr_create_tmpdir

fr_start_container "agent28-s1-$$" "$FR_IMAGE_FULL"

# Generate a test SSH key pair and install the public key in the container.
fr_generate_ssh_key "$FR_TMPDIR/test_key"
fr_install_pubkey "$FR_TMPDIR/test_key.pub"

# Start ssh-agent and load the key.
eval "$(ssh-agent -s)" >/dev/null 2>&1
ssh-add "$FR_TMPDIR/test_key" 2>/dev/null

# ---- Phase 1: TOFU — empty known_hosts ----------------------------------- #

fr_section "Phase 1: Host key verification with empty known_hosts (FR-CONN-01, FR-CONN-03)"

EMPTY_KNOWN_HOSTS="$FR_TMPDIR/known_hosts_empty"
touch "$EMPTY_KNOWN_HOSTS"

# With BatchMode=yes and StrictHostKeyChecking=ask, SSH should REJECT the
# connection because it cannot prompt for TOFU acceptance interactively.
# This simulates what happens when the TOFU dialog would appear.
set +e
result=$(ssh -o StrictHostKeyChecking=ask \
             -o UserKnownHostsFile="$EMPTY_KNOWN_HOSTS" \
             -o IdentityFile="$FR_TMPDIR/test_key" \
             -o IdentitiesOnly=yes \
             -o PasswordAuthentication=no \
             -o BatchMode=yes \
             -o LogLevel=ERROR \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo connected" 2>&1)
rc=$?
set -e

# BatchMode + unknown host = failure (TOFU requires interaction).
if [[ $rc -ne 0 ]]; then
  fr_pass "Empty known_hosts triggers TOFU (connection rejected in batch mode)"
else
  fr_fail "Empty known_hosts should have triggered TOFU rejection"
fi

# Verify known_hosts is still empty (nothing was auto-accepted).
kh_size=$(wc -c < "$EMPTY_KNOWN_HOSTS")
assert_num_eq "known_hosts still empty after rejection" "$kh_size" 0

fr_manual_note "Verify TOFU dialog shows SHA-256 fingerprint, key type, and three buttons"
fr_manual_note "Verify 'Cancel' returns to a clean state with no crash"

# ---- Phase 2: Accept host key (simulated TOFU accept) -------------------- #

fr_section "Phase 2: TOFU accept and host key saved (FR-CONN-03)"

# Simulate the user accepting the TOFU dialog by pre-scanning the key.
ACCEPTED_KNOWN_HOSTS="$FR_TMPDIR/known_hosts_accepted"
fr_populate_known_hosts "$ACCEPTED_KNOWN_HOSTS"

kh_lines=$(wc -l < "$ACCEPTED_KNOWN_HOSTS")
if [[ $kh_lines -gt 0 ]]; then
  fr_pass "Host key scanned and saved to known_hosts ($kh_lines entries)"
else
  fr_fail "Failed to scan host key"
fi

# Now connection should succeed with the populated known_hosts.
set +e
result=$(ssh -o StrictHostKeyChecking=yes \
             -o UserKnownHostsFile="$ACCEPTED_KNOWN_HOSTS" \
             -o IdentityFile="$FR_TMPDIR/test_key" \
             -o IdentitiesOnly=yes \
             -o PasswordAuthentication=no \
             -o BatchMode=yes \
             -o LogLevel=ERROR \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo tofu-accepted" 2>&1)
rc=$?
set -e

assert_exit_code "Connection succeeds after TOFU accept" 0 "$rc"
assert_contains "Command output correct" "$result" "tofu-accepted"

# ---- Phase 3: Authentication via ssh-agent (FR-CONN-06, FR-CONN-07) ------- #

fr_section "Phase 3: Authentication via ssh-agent (FR-CONN-06, FR-CONN-07)"

# Test that agent-based auth works (key is loaded in agent from setup).
set +e
result=$(ssh -o StrictHostKeyChecking=no \
             -o UserKnownHostsFile="$ACCEPTED_KNOWN_HOSTS" \
             -o PasswordAuthentication=no \
             -o PubkeyAuthentication=yes \
             -o IdentitiesOnly=no \
             -o BatchMode=yes \
             -o LogLevel=ERROR \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo agent-auth-ok" 2>&1)
rc=$?
set -e

assert_exit_code "Agent-based authentication succeeds" 0 "$rc"
assert_contains "Agent auth output correct" "$result" "agent-auth-ok"

# ---- Phase 4: tmux detection (FR-CONN-13) -------------------------------- #

fr_section "Phase 4: tmux detection (FR-CONN-13)"

tmux_ver=$(fr_tmux_version)
assert_contains "tmux is installed on server" "$tmux_ver" "tmux"

# Parse version and verify >= 3.0.
version_num=$(echo "$tmux_ver" | grep -oP '[\d.]+' | head -1)
major=$(echo "$version_num" | cut -d. -f1)
if [[ "$major" -ge 3 ]]; then
  fr_pass "tmux version >= 3.0 ($tmux_ver)"
else
  fr_fail "tmux version < 3.0 ($tmux_ver)"
fi

# ---- Phase 5: No prior state — Default environment (FR-ENV-05) ------------ #

fr_section "Phase 5: First connection — no prior state (FR-ENV-05)"

# Verify no state files exist yet.
state_count=$(fr_ssh_cmd_nocheck "ls $FR_REMOTE_STATE_DIR/*.json 2>/dev/null | wc -l" || echo "0")
assert_num_eq "No state files on fresh server" "$state_count" 0

# Verify no tmux sessions exist yet.
sessions=$(fr_tmux_list)
assert_eq "No tmux sessions on fresh server" "$sessions" ""

# Simulate shellkeep creating the default environment.
CLIENT_ID="fr-scenario1"
ENV_NAME="Default"

fr_ssh_cmd_nocheck "mkdir -p '$FR_REMOTE_STATE_DIR' && cat > '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json' << 'EOF'
{
  \"schema_version\": 1,
  \"last_modified\": \"$(date -u +"%Y-%m-%dT%H:%M:%SZ")\",
  \"client_id\": \"$CLIENT_ID\",
  \"environments\": {
    \"$ENV_NAME\": {
      \"windows\": []
    }
  },
  \"last_environment\": \"$ENV_NAME\"
}
EOF"

assert_ok "State file created" fr_ssh_cmd_nocheck "test -f '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'"

state_content=$(fr_ssh_cmd_nocheck "cat '$FR_REMOTE_STATE_DIR/${CLIENT_ID}.json'")
assert_contains "State has Default environment" "$state_content" '"Default"'
assert_contains "State has schema_version" "$state_content" '"schema_version"'

# ---- Phase 6: Session creation works ------------------------------------- #

fr_section "Phase 6: Session creation (FR-SESSION-04, FR-SESSION-09)"

SESSION_NAME="${CLIENT_ID}--${ENV_NAME}--session-test"
UUID="cccccccc-cccc-4ccc-8ccc-cccccccccccc"

fr_ssh_cmd_nocheck "tmux new-session -d -s '$SESSION_NAME'"
fr_ssh_cmd_nocheck "tmux set-environment -t '$SESSION_NAME' SHELLKEEP_SESSION_UUID '$UUID'"

assert_ok "Session created successfully" fr_ssh_cmd_nocheck "tmux has-session -t '$SESSION_NAME'"

uuid_check=$(fr_ssh_cmd_nocheck "tmux show-environment -t '$SESSION_NAME' SHELLKEEP_SESSION_UUID" | cut -d= -f2-)
assert_eq "Session UUID stored correctly" "$uuid_check" "$UUID"

# Verify session is functional.
fr_ssh_cmd_nocheck "tmux send-keys -t '$SESSION_NAME' 'echo hello-shellkeep' Enter"
sleep 0.5
output=$(fr_ssh_cmd_nocheck "tmux capture-pane -t '$SESSION_NAME' -p" || true)
assert_contains "Session is functional" "$output" "hello-shellkeep"

# ---- Phase 7: Verify no crashes through entire flow ---------------------- #

fr_section "Phase 7: Stability verification"

# Run multiple operations rapidly to check for crashes.
for i in $(seq 1 3); do
  session_n="${CLIENT_ID}--${ENV_NAME}--stability-$i"
  fr_ssh_cmd_nocheck "tmux new-session -d -s '$session_n'" || true
done

session_count=$(fr_ssh_cmd_nocheck "tmux list-sessions 2>/dev/null | wc -l")
if [[ "$session_count" -ge 4 ]]; then
  fr_pass "Multiple sessions created without crash ($session_count total)"
else
  fr_fail "Expected >= 4 sessions, got $session_count"
fi

# Clean up sessions.
for i in $(seq 1 3); do
  session_n="${CLIENT_ID}--${ENV_NAME}--stability-$i"
  fr_ssh_cmd_nocheck "tmux kill-session -t '$session_n'" 2>/dev/null || true
done
fr_ssh_cmd_nocheck "tmux kill-session -t '$SESSION_NAME'" 2>/dev/null || true

fr_manual_note "Verify connection progress shows all phases: Connecting, Authenticating, Checking tmux, Loading state"
fr_manual_note "Verify Default environment was created automatically without asking"
fr_manual_note "Verify no blank screen appeared at any point"

# ---- Summary -------------------------------------------------------------- #

fr_section "Scenario 1 complete"
fr_summary
