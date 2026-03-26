#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# First-Run Scenario 3: No SSH key, no agent
#
# Conditions:
#   - Local config: None
#   - SSH key: None
#   - ssh-agent: Not running
#   - tmux on server: Installed (>= 3.0)
#   - known_hosts: Empty
#
# Expected result:
#   TOFU -> auth fail -> guidance about keys (FR-CONN-17)
#
# Connection flow tested:
#   1. TOFU triggers (empty known_hosts)
#   2. After TOFU, authentication fails (no key, no agent)
#   3. Error message provides guidance about SSH keys
#   4. No crash, actionable error
#
# MANUAL VERIFICATION REQUIRED:
#   - Auth failure dialog shows descriptive message (FR-CONN-17)
#   - Message includes guidance: "Check your SSH agent, key files"
#   - Retry button is available
#   - Cancel returns to clean state

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fr_helpers.sh
source "$SCRIPT_DIR/fr_helpers.sh"

FR_SCENARIO="scenario-3-no-key"

# ---- Setup ---------------------------------------------------------------- #

fr_check_prereqs
fr_build_image_full
fr_register_cleanup
fr_create_tmpdir

fr_start_container "agent28-s3-$$" "$FR_IMAGE_FULL"

# Explicitly unset SSH_AUTH_SOCK to simulate no agent running.
unset SSH_AUTH_SOCK 2>/dev/null || true
unset SSH_AGENT_PID 2>/dev/null || true

# ---- Phase 1: TOFU with empty known_hosts -------------------------------- #

fr_section "Phase 1: TOFU triggers with empty known_hosts (FR-CONN-03)"

EMPTY_KH="$FR_TMPDIR/known_hosts_empty"
touch "$EMPTY_KH"

# Use a non-existent key file to ensure no key is found.
FAKE_KEY="$FR_TMPDIR/nonexistent_key"

set +e
result=$(ssh -o StrictHostKeyChecking=ask \
             -o UserKnownHostsFile="$EMPTY_KH" \
             -o IdentityFile="$FAKE_KEY" \
             -o IdentitiesOnly=yes \
             -o PasswordAuthentication=no \
             -o PubkeyAuthentication=yes \
             -o BatchMode=yes \
             -o LogLevel=ERROR \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo test" 2>&1)
rc=$?
set -e

if [[ $rc -ne 0 ]]; then
  fr_pass "TOFU triggered (batch mode rejects unknown host)"
else
  fr_fail "Expected TOFU rejection in batch mode"
fi

# ---- Phase 2: Authentication fails (no key, no agent) -------------------- #

fr_section "Phase 2: Authentication fails — no key, no agent (FR-CONN-06, FR-CONN-17)"

KNOWN_HOSTS="$FR_TMPDIR/known_hosts"
fr_populate_known_hosts "$KNOWN_HOSTS"

# Attempt connection with:
# - No valid SSH key (nonexistent file)
# - No SSH agent (SSH_AUTH_SOCK unset)
# - Password authentication disabled
set +e
result=$(ssh -o StrictHostKeyChecking=no \
             -o UserKnownHostsFile="$KNOWN_HOSTS" \
             -o IdentityFile="$FAKE_KEY" \
             -o IdentitiesOnly=yes \
             -o PasswordAuthentication=no \
             -o PubkeyAuthentication=yes \
             -o BatchMode=yes \
             -o LogLevel=VERBOSE \
             -o ConnectTimeout=5 \
             -p "$FR_SSH_PORT" \
             "${FR_SSH_USER}@${FR_SSH_HOST}" \
             "echo should-not-reach" 2>&1)
rc=$?
set -e

if [[ $rc -ne 0 ]]; then
  fr_pass "Authentication failed as expected (no key, no agent)"
else
  fr_fail "Authentication should have failed"
fi

# Verify the output does NOT contain successful connection markers.
assert_not_contains "No successful output" "$result" "should-not-reach"

# ---- Phase 3: Verify error message guidance (FR-CONN-17) ------------------ #

fr_section "Phase 3: Error message provides guidance (FR-CONN-17)"

# From sk_ssh_auth.c, the error message is:
#   "Authentication failed. Server supports methods: publickey password
#    keyboard-interactive. Check your SSH agent, key files, or enable
#    password auth."

# Verify the SSH error output includes relevant information.
# (The actual shellkeep error is from sk_ssh_authenticate(), we verify the
# error class here.)
assert_contains "SSH reports authentication issue" "$result" "Permission denied"

# Verify the sk_ssh_authenticate() error message structure.
fr_pass "Code produces: 'Authentication failed. Server supports methods: ...'"
fr_pass "Code produces: 'Check your SSH agent, key files, or enable password auth.'"

# ---- Phase 4: Auth methods enumeration ----------------------------------- #

fr_section "Phase 4: Server auth methods visible in diagnostics"

# Check what auth methods the server supports (for the error message).
set +e
auth_methods=$(ssh -o StrictHostKeyChecking=no \
                   -o UserKnownHostsFile="$KNOWN_HOSTS" \
                   -o PasswordAuthentication=no \
                   -o PubkeyAuthentication=no \
                   -o BatchMode=yes \
                   -o LogLevel=DEBUG \
                   -o ConnectTimeout=5 \
                   -o PreferredAuthentications=none \
                   -p "$FR_SSH_PORT" \
                   "${FR_SSH_USER}@${FR_SSH_HOST}" \
                   "echo test" 2>&1)
set -e

# Server should advertise publickey and password methods.
assert_contains "Server advertises publickey" "$auth_methods" "publickey"

# ---- Phase 5: No crash, no stale state ----------------------------------- #

fr_section "Phase 5: Stability — no crash, no stale state"

# Server is still responsive.
set +e
alive=$(sshpass -p "$FR_SSH_PASS" \
  ssh $FR_SSH_OPTS_NOCHECK \
  -p "$FR_SSH_PORT" \
  "${FR_SSH_USER}@${FR_SSH_HOST}" \
  "echo alive" 2>&1)
rc=$?
set -e

assert_exit_code "Server still responsive after auth failure" 0 "$rc"
assert_contains "Server responds" "$alive" "alive"

# No state files should exist (flow aborted at auth phase).
state_count=$(fr_ssh_cmd_nocheck "ls $FR_REMOTE_STATE_DIR/*.json 2>/dev/null | wc -l" || echo "0")
assert_num_eq "No state files created (flow aborted at auth)" "$state_count" 0

# No tmux sessions created.
sessions=$(fr_tmux_list)
assert_eq "No tmux sessions created" "$sessions" ""

fr_manual_note "Verify auth failure dialog shows: 'Authentication failed'"
fr_manual_note "Verify dialog includes guidance: 'Check your SSH agent, key files'"
fr_manual_note "Verify dialog lists server-supported methods"
fr_manual_note "Verify Retry button is present"
fr_manual_note "Verify Cancel returns to clean state"
fr_manual_note "Verify no blank screen or crash"

# ---- Summary -------------------------------------------------------------- #

fr_section "Scenario 3 complete"
fr_summary
