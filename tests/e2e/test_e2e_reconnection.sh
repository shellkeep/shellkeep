#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# E2E Scenario 2: Reconnection
#
# Tests disconnect/reconnect with network interruption simulation:
#   - Connect and create 5 tabs with long-running processes (sleep 3600)
#   - Simulate network interruption via iptables DROP
#   - Verify processes survive during disconnection
#   - Restore network and verify all 5 sessions are reconnectable
#   - Verify sleep processes are still running after reconnect
#
# Requirements tested:
#   FR-CONN-18, FR-SESSION-03, FR-SESSION-10, FR-STATE-06,
#   NFR-PERF-03 (reconnection within SLO)
#
# NOTE: GUI behaviors (spinner during disconnect, visual reconnection
# feedback) require a display server and must be verified manually.
# This script tests that tmux sessions and their processes survive
# network interruptions.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=e2e_helpers.sh
source "$SCRIPT_DIR/e2e_helpers.sh"

CONTAINER_SUFFIX="reconnect-$$"
CLIENT_ID="e2e-reconnect"
ENV_NAME="Default"
NUM_TABS=5

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
e2e_register_cleanup
e2e_start_container "$CONTAINER_SUFFIX"

# ---- Test: Create 5 sessions with long-running processes ------------------ #

e2e_section "Create $NUM_TABS sessions with sleep 3600 (FR-SESSION-09)"

declare -a SESSION_NAMES
declare -a SESSION_UUIDS

for i in $(seq 1 $NUM_TABS); do
  session_name="${CLIENT_ID}--${ENV_NAME}--tab-${i}"
  uuid="$(printf '%08d-0000-4000-8000-%012d' "$i" "$i")"
  SESSION_NAMES+=("$session_name")
  SESSION_UUIDS+=("$uuid")

  e2e_tmux_create "$session_name"
  e2e_tmux_set_env "$session_name" "SHELLKEEP_SESSION_UUID" "$uuid"

  # Start a long-running process in each session.
  e2e_ssh_cmd "tmux send-keys -t '$session_name' 'sleep 3600 &' Enter"
  sleep 0.5
  e2e_ssh_cmd "tmux send-keys -t '$session_name' 'echo MARKER_TAB_${i}' Enter"
done

# Verify all sessions exist.
session_count=$(e2e_tmux_list | wc -l)
assert_num_eq "All $NUM_TABS sessions created" "$session_count" "$NUM_TABS"

# Verify sleep processes are running (give time for all to start).
sleep 3
sleep_count=$(e2e_ssh_cmd "pgrep -xc sleep" 2>/dev/null | tr -cd '0-9' || true)
sleep_count="${sleep_count:-0}"
if [[ "$sleep_count" -ge "$NUM_TABS" ]]; then
  e2e_pass "All $NUM_TABS sleep processes running (found $sleep_count)"
else
  e2e_fail "Expected $NUM_TABS sleep processes, found $sleep_count"
fi

# ---- Test: Write state file before disconnection -------------------------- #

e2e_section "State file before disconnection"

tabs_json=""
for i in $(seq 0 $((NUM_TABS - 1))); do
  idx=$((i + 1))
  if [[ -n "$tabs_json" ]]; then tabs_json+=","; fi
  tabs_json+=$(cat <<TABJSON
            {
              "session_uuid": "${SESSION_UUIDS[$i]}",
              "tmux_session_name": "${SESSION_NAMES[$i]}",
              "title": "tab-${idx}",
              "position": $i
            }
TABJSON
)
done

STATE_JSON=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "$CLIENT_ID",
  "environments": {
    "$ENV_NAME": {
      "windows": [
        {
          "id": "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
          "title": "Main",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1024, "height": 768 },
          "tabs": [
$tabs_json
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

# ---- Test: Simulate network interruption via iptables --------------------- #

e2e_section "Network interruption simulation"

# We use docker exec (not SSH) because SSH will be blocked.
# Simulate network interruption using tc netem (100% packet loss).
# This is more reliable than iptables in Docker containers because
# iptables rules can conflict with Docker's own NAT/forwarding rules.
e2e_docker_exec "tc qdisc add dev eth0 root netem loss 100%" || {
  e2e_skip "tc/netem not available (need NET_ADMIN + iproute2); skipping network disruption test"
  # Continue with remaining tests even if tc fails.
}

e2e_log "Network blocked. Waiting 3 seconds to simulate disconnection..."
sleep 3

# Verify SSH is indeed blocked (this should time out or fail).
if e2e_ssh_cmd_timeout 3 "echo reachable" &>/dev/null; then
  e2e_fail "SSH should be unreachable during network block"
else
  e2e_pass "SSH unreachable during network block"
fi

# ---- Test: Processes survive during disconnection (via docker exec) ------- #

e2e_section "Process survival during disconnection"

# Check processes via docker exec (bypasses network block).
sleep_during=$(e2e_docker_exec "pgrep -xc sleep" 2>/dev/null | tr -cd '0-9' || true)
sleep_during="${sleep_during:-0}"
if [[ "$sleep_during" -ge "$NUM_TABS" ]]; then
  e2e_pass "Sleep processes alive during disconnect (found $sleep_during)"
else
  e2e_fail "Expected $NUM_TABS sleep processes during disconnect, found $sleep_during"
fi

# Verify tmux sessions still exist via docker exec.
tmux_during=$(e2e_docker_exec "su - testuser -c 'tmux list-sessions -F \"#{session_name}\" 2>/dev/null'" | wc -l | tr -cd '0-9')
tmux_during="${tmux_during:-0}"
assert_num_eq "All tmux sessions alive during disconnect" "$tmux_during" "$NUM_TABS"

# ---- Test: Restore network ------------------------------------------------ #

e2e_section "Network restoration"

# Remove the tc netem qdisc to restore network connectivity.
e2e_docker_exec "tc qdisc del dev eth0 root" || true

# Wait for SSH to become reachable again.
retries=30
reachable=false
while [[ $retries -gt 0 ]]; do
  if e2e_ssh_cmd_timeout 5 "echo alive" &>/dev/null; then
    reachable=true
    break
  fi
  e2e_log "SSH retry $((31 - retries))/30..."
  sleep 1
  ((retries--)) || true
done

if $reachable; then
  e2e_pass "SSH reachable after network restore"
else
  e2e_fail "SSH not reachable after network restore"
  # Cannot continue without SSH.
  e2e_summary
fi

# ---- Test: All sessions reconnectable after network restore --------------- #

e2e_section "Session reconnection after network restore"

for i in $(seq 0 $((NUM_TABS - 1))); do
  idx=$((i + 1))
  session="${SESSION_NAMES[$i]}"
  assert_ok "Session tab-${idx} exists after reconnect" e2e_tmux_has_session "$session"

  # Verify UUID preserved.
  uuid=$(e2e_tmux_get_env "$session" "SHELLKEEP_SESSION_UUID")
  assert_eq "Session tab-${idx} UUID preserved" "$uuid" "${SESSION_UUIDS[$i]}"
done

# ---- Test: Sleep processes still running after reconnect ------------------ #

e2e_section "Process survival after reconnect"

sleep_after=$(e2e_ssh_cmd "pgrep -xc sleep" 2>/dev/null | tr -cd '0-9' || true)
sleep_after="${sleep_after:-0}"
if [[ "$sleep_after" -ge "$NUM_TABS" ]]; then
  e2e_pass "All sleep processes alive after reconnect (found $sleep_after)"
else
  e2e_fail "Expected $NUM_TABS sleep processes after reconnect, found $sleep_after"
fi

# ---- Test: Can interact with sessions after reconnect --------------------- #

e2e_section "Session interactivity after reconnect"

for i in $(seq 0 $((NUM_TABS - 1))); do
  idx=$((i + 1))
  session="${SESSION_NAMES[$i]}"

  e2e_ssh_cmd "tmux send-keys -t '$session' 'echo RECONNECT_OK_${idx}' Enter"
done

sleep 1

for i in $(seq 0 $((NUM_TABS - 1))); do
  idx=$((i + 1))
  session="${SESSION_NAMES[$i]}"

  output=$(e2e_ssh_cmd "tmux capture-pane -t '$session' -p" || true)
  assert_contains "Session tab-${idx} interactive after reconnect" "$output" "RECONNECT_OK_${idx}"
done

# ---- Test: State file intact after reconnect ------------------------------ #

e2e_section "State file integrity after reconnect"

state_after=$(e2e_read_state "$CLIENT_ID")
assert_contains "State file intact after reconnect" "$state_after" '"schema_version"'
assert_contains "State has all sessions" "$state_after" "tab-1"
assert_contains "State has all sessions" "$state_after" "tab-5"

# ---- Test: Verify capture history survived -------------------------------- #

e2e_section "Terminal history after reconnect"

# Check that the MARKER we echoed before the disconnect is still in the pane.
for i in $(seq 0 $((NUM_TABS - 1))); do
  idx=$((i + 1))
  session="${SESSION_NAMES[$i]}"

  history=$(e2e_ssh_cmd "tmux capture-pane -t '$session' -p -S -50" || true)
  assert_contains "Session tab-${idx} retains pre-disconnect output" "$history" "MARKER_TAB_${idx}"
done

# ---- Summary -------------------------------------------------------------- #

e2e_section "Reconnection scenario complete"
e2e_summary
