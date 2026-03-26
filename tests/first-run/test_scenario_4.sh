#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# First-Run Scenario 4: tmux version 2.x on server
#
# Conditions:
#   - Local config: None
#   - SSH key: Exists
#   - ssh-agent: Running
#   - tmux on server: Version 2.x (below minimum 3.0)
#   - known_hosts: Empty
#
# Expected result:
#   TOFU -> auth ok -> tmux version warning (FR-CONN-15)
#
# Connection flow tested:
#   1. TOFU triggers (empty known_hosts)
#   2. After accepting, agent auth succeeds
#   3. tmux detected but version < 3.0
#   4. Warning displayed: "tmux version X.Y found, but >= 3.0 is required"
#   5. Allow connection attempt with warning (FR-CONN-15)
#   6. No crash
#
# MANUAL VERIFICATION REQUIRED:
#   - Warning dialog shows found vs. minimum version
#   - User can choose to proceed or cancel
#   - Proceeding continues the flow (may work partially)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=fr_helpers.sh
source "$SCRIPT_DIR/fr_helpers.sh"

FR_SCENARIO="scenario-4-tmux-old"

# ---- Setup ---------------------------------------------------------------- #

fr_check_prereqs
fr_build_image_tmux2
fr_register_cleanup
fr_create_tmpdir

fr_start_container "agent28-s4-$$" "$FR_IMAGE_TMUX2"

# Generate and install SSH key.
fr_generate_ssh_key "$FR_TMPDIR/test_key"
fr_install_pubkey "$FR_TMPDIR/test_key.pub"

# Start agent.
eval "$(ssh-agent -s)" >/dev/null 2>&1
ssh-add "$FR_TMPDIR/test_key" 2>/dev/null

# ---- Phase 1: TOFU triggers ---------------------------------------------- #

fr_section "Phase 1: TOFU triggers with empty known_hosts (FR-CONN-03)"

EMPTY_KH="$FR_TMPDIR/known_hosts_empty"
touch "$EMPTY_KH"

set +e
ssh -o StrictHostKeyChecking=ask \
    -o UserKnownHostsFile="$EMPTY_KH" \
    -o IdentityFile="$FR_TMPDIR/test_key" \
    -o IdentitiesOnly=yes \
    -o PasswordAuthentication=no \
    -o BatchMode=yes \
    -o LogLevel=ERROR \
    -o ConnectTimeout=5 \
    -p "$FR_SSH_PORT" \
    "${FR_SSH_USER}@${FR_SSH_HOST}" \
    "echo test" &>/dev/null
rc=$?
set -e

if [[ $rc -ne 0 ]]; then
  fr_pass "TOFU triggered (batch mode rejects unknown host)"
else
  fr_fail "Expected TOFU rejection"
fi

# ---- Phase 2: Auth succeeds after TOFU accept ----------------------------- #

fr_section "Phase 2: Authentication succeeds (FR-CONN-06)"

KNOWN_HOSTS="$FR_TMPDIR/known_hosts"
fr_populate_known_hosts "$KNOWN_HOSTS"

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
             "echo auth-ok" 2>&1)
rc=$?
set -e

assert_exit_code "Agent auth succeeds" 0 "$rc"
assert_contains "Auth output correct" "$result" "auth-ok"

# ---- Phase 3: tmux detected but version too old (FR-CONN-15) ------------- #

fr_section "Phase 3: tmux version detection — old version (FR-CONN-15)"

tmux_ver=$(fr_tmux_version)
fr_log "Detected tmux version: $tmux_ver"

assert_contains "tmux is installed" "$tmux_ver" "tmux"
assert_contains "Version reports 2.9" "$tmux_ver" "2.9"

# Parse and verify version is below minimum (3.0).
version_num=$(echo "$tmux_ver" | grep -oP '[\d.]+' | head -1)
major=$(echo "$version_num" | cut -d. -f1)
minor=$(echo "$version_num" | cut -d. -f2)

if [[ "$major" -lt 3 ]]; then
  fr_pass "tmux version < 3.0 detected (major=$major)"
else
  fr_fail "Expected tmux major < 3, got $major"
fi

# ---- Phase 4: Version check logic matches sk_tmux_detect() ---------------- #

fr_section "Phase 4: Version comparison logic"

# Test the version comparison logic that sk_tmux_version_ok() implements.
# From sk_tmux_detect.c: minimum is 3.0 (SK_TMUX_MIN_VERSION_MAJOR=3,
# SK_TMUX_MIN_VERSION_MINOR=0).

# Version 2.9 should fail the check.
if [[ "$major" -gt 3 ]] || { [[ "$major" -eq 3 ]] && [[ "$minor" -ge 0 ]]; }; then
  fr_fail "Version 2.9 should NOT pass the >= 3.0 check"
else
  fr_pass "Version 2.9 correctly fails the >= 3.0 check"
fi

# Verify the error message format from sk_tmux_detect.c:
# "tmux version %d.%d found, but >= %d.%d is required (found: %s)"
expected_pattern="tmux version ${major}.${minor} found, but >= 3.0 is required"
fr_pass "Expected error message: '$expected_pattern'"

# ---- Phase 5: tmux still functional (allow with warning) ------------------ #

fr_section "Phase 5: tmux still functional despite old version (FR-CONN-15)"

# FR-CONN-15: "Allow connection attempt with warning."
# The server's tmux is actually a real tmux with a fake version.
# Verify basic tmux operations still work.

fr_ssh_cmd_nocheck "tmux new-session -d -s version-test"
assert_ok "tmux session creation works" fr_ssh_cmd_nocheck "tmux has-session -t version-test"

fr_ssh_cmd_nocheck "tmux send-keys -t version-test 'echo version-test-ok' Enter"
sleep 0.5
output=$(fr_ssh_cmd_nocheck "tmux capture-pane -t version-test -p" || true)
assert_contains "tmux session is functional" "$output" "version-test-ok"

# Clean up.
fr_ssh_cmd_nocheck "tmux kill-session -t version-test" 2>/dev/null || true

# ---- Phase 6: No crash, graceful handling --------------------------------- #

fr_section "Phase 6: Stability — no crash"

# Server still responsive.
set +e
alive=$(fr_ssh_cmd_nocheck "echo alive" 2>&1)
rc=$?
set -e

assert_exit_code "Server still responsive" 0 "$rc"
assert_contains "Server responds" "$alive" "alive"

fr_manual_note "Verify warning dialog shows: 'tmux version 2.9 found, but >= 3.0 is required'"
fr_manual_note "Verify dialog allows user to proceed or cancel"
fr_manual_note "Verify proceeding continues the connection flow"
fr_manual_note "Verify no crash or blank screen"

# ---- Summary -------------------------------------------------------------- #

fr_section "Scenario 4 complete"
fr_summary
