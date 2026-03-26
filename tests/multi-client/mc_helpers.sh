#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Shared helper functions for shellkeep multi-client isolation tests.
# Source this file from individual test scripts.
#
# These helpers build on top of the e2e helpers, adding multi-client
# specific utilities for managing two simultaneous clients with
# independent sessions, state files, and locks.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source the base e2e helpers for Docker, SSH, tmux, lock, and assert functions.
# shellcheck source=../e2e/e2e_helpers.sh
source "$SCRIPT_DIR/../e2e/e2e_helpers.sh"

# ---- Multi-client constants ------------------------------------------------ #

# Container naming prefix for agent 32 tests.
MC_CONTAINER_PREFIX="sk-mc-agent32"

# Default keepalive timeout for orphan detection (seconds).
# FR-LOCK-07: orphan threshold = 2 x keepalive timeout.
MC_KEEPALIVE_TIMEOUT="${MC_KEEPALIVE_TIMEOUT:-45}"
MC_ORPHAN_THRESHOLD=$(( MC_KEEPALIVE_TIMEOUT * 2 ))

# Heartbeat interval (seconds). FR-LOCK-09: every keepalive_interval x 2.
MC_HEARTBEAT_INTERVAL="${MC_HEARTBEAT_INTERVAL:-30}"

# ---- Multi-client lock helpers --------------------------------------------- #

# Acquire a lock with a specific heartbeat timestamp.
# Usage: mc_lock_acquire_with_ts <client-id> <hostname> <iso-timestamp>
mc_lock_acquire_with_ts() {
  local client_id="${1:?client-id required}"
  local hostname="${2:?hostname required}"
  local timestamp="${3:?timestamp required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"

  e2e_ssh_cmd "tmux new-session -d -s '${lock_name}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_CLIENT_ID '${client_id}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_HOSTNAME '${hostname}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_CONNECTED_AT '${timestamp}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_PID '$$' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_VERSION '0.1.0'"
}

# Update the heartbeat timestamp on an existing lock.
# Usage: mc_lock_heartbeat <client-id>
mc_lock_heartbeat() {
  local client_id="${1:?client-id required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"
  local timestamp
  timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT" "$timestamp"
}

# Check if a lock is orphaned (heartbeat expired).
# Returns 0 if orphaned, 1 if still alive.
# Usage: mc_lock_is_orphaned <client-id>
mc_lock_is_orphaned() {
  local client_id="${1:?client-id required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"

  local ts
  ts=$(e2e_tmux_get_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT")
  if [[ -z "$ts" ]]; then
    # No timestamp means invalid lock -- treat as orphaned.
    return 0
  fi

  local lock_epoch current_epoch age
  lock_epoch=$(date -d "$ts" +%s 2>/dev/null || echo "0")
  current_epoch=$(date +%s)
  age=$((current_epoch - lock_epoch))

  if [[ "$age" -gt "$MC_ORPHAN_THRESHOLD" ]]; then
    return 0  # Orphaned.
  fi
  return 1  # Still alive.
}

# Attempt to acquire a lock, handling conflict scenarios.
# Returns:
#   0 = lock acquired (no conflict)
#   1 = conflict detected (lock held by another client)
#   2 = orphan auto-takeover (lock was orphaned, auto-acquired)
# Usage: mc_lock_try_acquire <client-id> <hostname>
mc_lock_try_acquire() {
  local client_id="${1:?client-id required}"
  local hostname="${2:?hostname required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"

  # Try atomic creation.
  if e2e_ssh_cmd "tmux new-session -d -s '${lock_name}' 2>/dev/null"; then
    # Lock created successfully -- set env vars.
    local timestamp
    timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_CLIENT_ID" "$client_id"
    e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_HOSTNAME" "$hostname"
    e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_CONNECTED_AT" "$timestamp"
    e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_PID" "$$"
    e2e_tmux_set_env "$lock_name" "SHELLKEEP_LOCK_VERSION" "0.1.0"
    return 0
  fi

  # Lock already exists. Check if orphaned.
  if mc_lock_is_orphaned "$client_id"; then
    # Orphan: auto-takeover without dialog (FR-LOCK-07).
    e2e_tmux_kill "$lock_name"
    e2e_lock_acquire "$client_id" "$hostname"
    return 2
  fi

  # Conflict: lock is held by an active client.
  return 1
}

# Force-takeover a lock (simulates "Disconnect and connect" action).
# FR-LOCK-05, FR-LOCK-06: kills existing lock and creates new one.
# Usage: mc_lock_force_takeover <client-id> <hostname>
mc_lock_force_takeover() {
  local client_id="${1:?client-id required}"
  local hostname="${2:?hostname required}"

  e2e_lock_release "$client_id"
  e2e_lock_acquire "$client_id" "$hostname"
}

# ---- Multi-client session helpers ------------------------------------------ #

# Create a set of sessions for a client in a given environment.
# Usage: mc_create_sessions <client-id> <env-name> <count> <name-prefix>
# Outputs session names (one per line) to stdout.
mc_create_sessions() {
  local client_id="${1:?client-id required}"
  local env_name="${2:?environment name required}"
  local count="${3:?count required}"
  local prefix="${4:?name prefix required}"

  local i sname suuid
  for (( i=1; i<=count; i++ )); do
    sname="${client_id}--${env_name}--${prefix}-${i}"
    suuid="$(uuidgen 2>/dev/null || printf '%08x-%04x-4%03x-8%03x-%012x' $RANDOM $RANDOM $RANDOM $RANDOM $RANDOM)"
    e2e_tmux_create "$sname"
    e2e_tmux_set_env "$sname" "SHELLKEEP_SESSION_UUID" "$suuid"
    echo "$sname"
  done
}

# List sessions belonging to a specific client-id.
# Usage: mc_list_client_sessions <client-id>
mc_list_client_sessions() {
  local client_id="${1:?client-id required}"
  e2e_tmux_list | grep "^${client_id}--" || true
}

# Count sessions belonging to a specific client-id.
# Usage: mc_count_client_sessions <client-id>
mc_count_client_sessions() {
  local client_id="${1:?client-id required}"
  local list
  list=$(mc_list_client_sessions "$client_id")
  if [[ -z "$list" ]]; then
    echo "0"
  else
    echo "$list" | wc -l | tr -d ' '
  fi
}

# ---- Multi-client state helpers -------------------------------------------- #

# Generate a minimal valid state JSON for a client with given sessions.
# Usage: mc_generate_state <client-id> <env-name> <session-names-array>
# Note: session names must be newline-separated string.
mc_generate_state() {
  local client_id="${1:?client-id required}"
  local env_name="${2:?environment name required}"
  local sessions_str="${3:-}"

  local timestamp
  timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

  local tabs_json=""
  local position=0
  if [[ -n "$sessions_str" ]]; then
    while IFS= read -r sname <&3; do
      [[ -z "$sname" ]] && continue
      local suuid
      suuid=$(e2e_tmux_get_env "$sname" "SHELLKEEP_SESSION_UUID" 2>/dev/null || echo "unknown")
      local title
      title=$(echo "$sname" | awk -F'--' '{print $NF}')

      if [[ "$position" -gt 0 ]]; then
        tabs_json+=","
      fi
      tabs_json+="
            {
              \"session_uuid\": \"${suuid}\",
              \"tmux_session_name\": \"${sname}\",
              \"title\": \"${title}\",
              \"position\": ${position}
            }"
      ((position++)) || true
    done 3<<< "$sessions_str"
  fi

  cat <<EOF
{
  "schema_version": 1,
  "last_modified": "${timestamp}",
  "client_id": "${client_id}",
  "environments": {
    "${env_name}": {
      "windows": [
        {
          "id": "$(uuidgen 2>/dev/null || printf '%08x-%04x-4%03x-8%03x-%012x' $RANDOM $RANDOM $RANDOM $RANDOM $RANDOM)",
          "title": "${client_id} Window",
          "visible": true,
          "active_tab": 0,
          "geometry": { "x": 0, "y": 0, "width": 1280, "height": 720 },
          "tabs": [${tabs_json}
          ]
        }
      ]
    }
  },
  "last_environment": "${env_name}"
}
EOF
}

# ---- Container management (agent-32 specific) ----------------------------- #

# Start a container with agent-32 naming convention.
# Usage: mc_start_container <test-name>
mc_start_container() {
  local test_name="${1:?test name required}"
  local suffix="${MC_CONTAINER_PREFIX}-${test_name}-$$"
  e2e_start_container "$suffix"
}

# ---- Cleanup --------------------------------------------------------------- #

# Enhanced cleanup that stops all agent-32 containers.
mc_cleanup() {
  e2e_stop_container
  # Stop any leftover agent-32 containers.
  docker ps -q --filter "name=${MC_CONTAINER_PREFIX}" 2>/dev/null | \
    xargs -r docker stop 2>/dev/null || true
}

mc_register_cleanup() {
  trap 'mc_cleanup' EXIT
}
