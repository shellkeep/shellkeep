#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Shared helper functions for shellkeep e2e tests.
# Source this file from individual test scripts.

set -euo pipefail

# ---- Color output --------------------------------------------------------- #

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# ---- Counters ------------------------------------------------------------- #

E2E_PASS=0
E2E_FAIL=0
E2E_SKIP=0

# ---- Configuration -------------------------------------------------------- #

# Docker image and container settings.
E2E_IMAGE="${E2E_IMAGE:-shellkeep-test-sshd}"
E2E_CONTAINER_PREFIX="${E2E_CONTAINER_PREFIX:-sk-e2e}"

# SSH credentials matching the integration Dockerfile.
E2E_SSH_HOST="${E2E_SSH_HOST:-127.0.0.1}"
E2E_SSH_PORT="${E2E_SSH_PORT:-}"
E2E_SSH_USER="${E2E_SSH_USER:-testuser}"
E2E_SSH_PASS="${E2E_SSH_PASS:-testpass}"

# Base SSH options (disable host key checking for ephemeral containers).
E2E_SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"

# State directory on the remote server.
E2E_REMOTE_STATE_DIR="/home/testuser/.shellkeep"

# ---- Logging -------------------------------------------------------------- #

e2e_log() {
  echo -e "${CYAN}[e2e]${NC} $*"
}

e2e_pass() {
  echo -e "  ${GREEN}PASS${NC}: $*"
  ((E2E_PASS++)) || true
}

e2e_fail() {
  echo -e "  ${RED}FAIL${NC}: $*"
  ((E2E_FAIL++)) || true
}

e2e_skip() {
  echo -e "  ${YELLOW}SKIP${NC}: $*"
  ((E2E_SKIP++)) || true
}

e2e_section() {
  echo ""
  echo -e "${CYAN}--- $* ---${NC}"
}

# ---- Docker helpers ------------------------------------------------------- #

# Build the test Docker image if not already present.
e2e_build_image() {
  local dockerfile_dir
  dockerfile_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../integration" && pwd)"
  if ! docker image inspect "$E2E_IMAGE" &>/dev/null; then
    e2e_log "Building Docker image $E2E_IMAGE ..."
    docker build -t "$E2E_IMAGE" "$dockerfile_dir"
  fi
}

# Start a fresh container. Sets E2E_SSH_PORT to the allocated host port.
# Usage: e2e_start_container <suffix>
e2e_start_container() {
  local suffix="${1:?container suffix required}"
  local name="${E2E_CONTAINER_PREFIX}-${suffix}"

  # Stop any leftover container with the same name.
  docker rm -f "$name" &>/dev/null || true

  e2e_log "Starting container $name ..."
  docker run -d --rm \
    --name "$name" \
    --cap-add NET_ADMIN \
    -p 0:22 \
    "$E2E_IMAGE" >/dev/null

  # Retrieve allocated port.
  E2E_SSH_PORT=$(docker port "$name" 22 2>/dev/null | head -1 | sed 's/.*://')
  E2E_CONTAINER_NAME="$name"

  # Wait for sshd to become reachable.
  local retries=30
  while ! e2e_ssh_cmd "true" &>/dev/null; do
    ((retries--)) || { e2e_fail "Container $name sshd did not start"; return 1; }
    sleep 0.3
  done

  e2e_log "Container $name ready on port $E2E_SSH_PORT"
}

# Stop the current container.
e2e_stop_container() {
  if [[ -n "${E2E_CONTAINER_NAME:-}" ]]; then
    e2e_log "Stopping container $E2E_CONTAINER_NAME ..."
    docker stop "$E2E_CONTAINER_NAME" &>/dev/null || true
    unset E2E_CONTAINER_NAME
    unset E2E_SSH_PORT
  fi
}

# ---- SSH helpers ---------------------------------------------------------- #

# Run a command on the test container via SSH with password auth.
# Usage: e2e_ssh_cmd "command"
e2e_ssh_cmd() {
  local cmd="${1:?command required}"
  sshpass -p "$E2E_SSH_PASS" \
    ssh $E2E_SSH_OPTS \
    -p "$E2E_SSH_PORT" \
    "${E2E_SSH_USER}@${E2E_SSH_HOST}" \
    "$cmd"
}

# Run a command on the test container via SSH with a timeout.
# Uses SSH ConnectTimeout + ServerAliveInterval instead of the `timeout`
# command, which cannot invoke shell functions.
# Usage: e2e_ssh_cmd_timeout <seconds> "command"
e2e_ssh_cmd_timeout() {
  local secs="${1:?timeout required}"
  local cmd="${2:?command required}"
  sshpass -p "$E2E_SSH_PASS" \
    ssh $E2E_SSH_OPTS \
    -o "ConnectTimeout=$secs" \
    -o "ServerAliveInterval=1" \
    -o "ServerAliveCountMax=$secs" \
    -p "$E2E_SSH_PORT" \
    "${E2E_SSH_USER}@${E2E_SSH_HOST}" \
    "$cmd"
}

# Run a command as root inside the container via docker exec.
# Useful for operations that need root (iptables, process management).
# Usage: e2e_docker_exec "command"
e2e_docker_exec() {
  local cmd="${1:?command required}"
  docker exec "$E2E_CONTAINER_NAME" bash -c "$cmd"
}

# ---- tmux helpers --------------------------------------------------------- #

# List all tmux sessions on the server.
# Returns one session name per line, or empty if no sessions.
e2e_tmux_list() {
  e2e_ssh_cmd "tmux list-sessions -F '#{session_name}' 2>/dev/null" || true
}

# Create a tmux session on the server.
# Usage: e2e_tmux_create <session-name> [initial-command]
e2e_tmux_create() {
  local name="${1:?session name required}"
  local cmd="${2:-}"
  if [[ -n "$cmd" ]]; then
    e2e_ssh_cmd "tmux new-session -d -s '$name' '$cmd'"
  else
    e2e_ssh_cmd "tmux new-session -d -s '$name'"
  fi
}

# Kill a tmux session on the server.
# Usage: e2e_tmux_kill <session-name>
e2e_tmux_kill() {
  local name="${1:?session name required}"
  e2e_ssh_cmd "tmux kill-session -t '$name'" || true
}

# Check if a tmux session exists.
# Usage: e2e_tmux_has_session <session-name>
e2e_tmux_has_session() {
  local name="${1:?session name required}"
  e2e_ssh_cmd "tmux has-session -t '$name' 2>/dev/null"
}

# Set an environment variable on a tmux session.
# Usage: e2e_tmux_set_env <session-name> <var> <value>
e2e_tmux_set_env() {
  local session="${1:?session required}"
  local var="${2:?var required}"
  local val="${3:?val required}"
  e2e_ssh_cmd "tmux set-environment -t '$session' '$var' '$val'"
}

# Get an environment variable from a tmux session.
# Usage: e2e_tmux_get_env <session-name> <var>
e2e_tmux_get_env() {
  local session="${1:?session required}"
  local var="${2:?var required}"
  e2e_ssh_cmd "tmux show-environment -t '$session' '$var' 2>/dev/null" | cut -d= -f2-
}

# ---- State file helpers --------------------------------------------------- #

# Write a JSON state file to the remote server.
# Usage: e2e_write_state <client-id> <json-content>
e2e_write_state() {
  local client_id="${1:?client-id required}"
  local json="${2:?json content required}"
  e2e_ssh_cmd "mkdir -p '$E2E_REMOTE_STATE_DIR' && cat > '$E2E_REMOTE_STATE_DIR/${client_id}.json' << 'STATEEOF'
${json}
STATEEOF"
}

# Read a state file from the remote server.
# Usage: e2e_read_state <client-id>
e2e_read_state() {
  local client_id="${1:?client-id required}"
  e2e_ssh_cmd "cat '$E2E_REMOTE_STATE_DIR/${client_id}.json' 2>/dev/null" || true
}

# Check if a state file exists.
# Usage: e2e_state_exists <client-id>
e2e_state_exists() {
  local client_id="${1:?client-id required}"
  e2e_ssh_cmd "test -f '$E2E_REMOTE_STATE_DIR/${client_id}.json'"
}

# ---- Lock helpers --------------------------------------------------------- #

# The lock session name follows the pattern: shellkeep-lock-<client-id>
E2E_LOCK_PREFIX="shellkeep-lock-"

# Create a lock session (simulates shellkeep lock acquisition).
# Usage: e2e_lock_acquire <client-id> <hostname>
e2e_lock_acquire() {
  local client_id="${1:?client-id required}"
  local hostname="${2:?hostname required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"
  local timestamp
  timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

  e2e_ssh_cmd "tmux new-session -d -s '${lock_name}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_CLIENT_ID '${client_id}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_HOSTNAME '${hostname}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_CONNECTED_AT '${timestamp}' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_PID '$$' \
    \\; set-environment -t '${lock_name}' SHELLKEEP_LOCK_VERSION '0.1.0'"
}

# Release a lock (kill the lock session).
# Usage: e2e_lock_release <client-id>
e2e_lock_release() {
  local client_id="${1:?client-id required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"
  e2e_tmux_kill "$lock_name"
}

# Check if a lock exists.
# Usage: e2e_lock_exists <client-id>
e2e_lock_exists() {
  local client_id="${1:?client-id required}"
  local lock_name="${E2E_LOCK_PREFIX}${client_id}"
  e2e_tmux_has_session "$lock_name"
}

# ---- Assertion helpers ---------------------------------------------------- #

# Assert a command succeeds (exit code 0).
# Usage: assert_ok <description> <command...>
assert_ok() {
  local desc="$1"; shift
  if "$@" &>/dev/null; then
    e2e_pass "$desc"
  else
    e2e_fail "$desc"
  fi
}

# Assert a command fails (non-zero exit code).
# Usage: assert_fail <description> <command...>
assert_fail() {
  local desc="$1"; shift
  if "$@" &>/dev/null; then
    e2e_fail "$desc (expected failure but succeeded)"
  else
    e2e_pass "$desc"
  fi
}

# Assert that a string contains a substring.
# Usage: assert_contains <description> <haystack> <needle>
assert_contains() {
  local desc="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    e2e_pass "$desc"
  else
    e2e_fail "$desc (expected '$needle' in output)"
  fi
}

# Assert that a string does NOT contain a substring.
# Usage: assert_not_contains <description> <haystack> <needle>
assert_not_contains() {
  local desc="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    e2e_pass "$desc"
  else
    e2e_fail "$desc (did not expect '$needle' in output)"
  fi
}

# Assert two strings are equal.
# Usage: assert_eq <description> <actual> <expected>
assert_eq() {
  local desc="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" == "$expected" ]]; then
    e2e_pass "$desc"
  else
    e2e_fail "$desc (expected '$expected', got '$actual')"
  fi
}

# Assert a numeric value.
# Usage: assert_num_eq <description> <actual> <expected>
assert_num_eq() {
  local desc="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" -eq "$expected" ]]; then
    e2e_pass "$desc"
  else
    e2e_fail "$desc (expected $expected, got $actual)"
  fi
}

# ---- Cleanup & summary --------------------------------------------------- #

# Register cleanup on exit.
e2e_register_cleanup() {
  trap 'e2e_stop_container' EXIT
}

# Print test summary and exit with appropriate code.
e2e_summary() {
  echo ""
  echo -e "${CYAN}========================================${NC}"
  echo -e "  ${GREEN}Passed:${NC}  $E2E_PASS"
  echo -e "  ${RED}Failed:${NC}  $E2E_FAIL"
  echo -e "  ${YELLOW}Skipped:${NC} $E2E_SKIP"
  echo -e "${CYAN}========================================${NC}"

  if [[ "$E2E_FAIL" -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

# ---- Prerequisite checks ------------------------------------------------- #

# Verify that required tools are available.
e2e_check_prereqs() {
  local missing=0

  for tool in docker sshpass ssh; do
    if ! command -v "$tool" &>/dev/null; then
      e2e_fail "Required tool not found: $tool"
      ((missing++)) || true
    fi
  done

  if [[ "$missing" -gt 0 ]]; then
    echo "Install missing tools and retry."
    exit 1
  fi
}
