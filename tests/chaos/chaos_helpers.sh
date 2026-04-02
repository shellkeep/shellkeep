#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Shared helper functions for shellkeep network chaos tests.
# Source this file from individual test scripts.
#
# These tests verify that SSH connections and tmux sessions survive adverse
# network conditions -- the core resilience promise of shellkeep.
#
# Since full GUI testing requires a display server, these scripts test the
# underlying SSH/tmux session layer directly. GUI-level verification items
# (spinner display, color indicators, cursor responsiveness) are documented
# in CHAOS-REPORT.md as requiring manual display-server testing.

set -euo pipefail

# ---- Color output --------------------------------------------------------- #

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# ---- Counters ------------------------------------------------------------- #

CHAOS_PASS=0
CHAOS_FAIL=0
CHAOS_SKIP=0

# ---- Configuration -------------------------------------------------------- #

CHAOS_IMAGE="${CHAOS_IMAGE:-shellkeep-chaos-sshd}"
CHAOS_CONTAINER_PREFIX="${CHAOS_CONTAINER_PREFIX:-sk-chaos}"

CHAOS_SSH_HOST="${CHAOS_SSH_HOST:-127.0.0.1}"
CHAOS_SSH_PORT="${CHAOS_SSH_PORT:-}"
CHAOS_SSH_USER="${CHAOS_SSH_USER:-testuser}"
CHAOS_SSH_PASS="${CHAOS_SSH_PASS:-testpass}"

# SSH options: disable host key checking for ephemeral containers.
# ServerAliveInterval/CountMax provide client-side keepalive detection.
CHAOS_SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=3"

CHAOS_REMOTE_STATE_DIR="/home/testuser/.shellkeep"

# ---- Logging -------------------------------------------------------------- #

chaos_log() {
  local timestamp
  timestamp=$(date +"%H:%M:%S")
  echo -e "${CYAN}[chaos ${timestamp}]${NC} $*"
}

chaos_pass() {
  echo -e "  ${GREEN}PASS${NC}: $*"
  ((CHAOS_PASS++)) || true
}

chaos_fail() {
  echo -e "  ${RED}FAIL${NC}: $*"
  ((CHAOS_FAIL++)) || true
}

chaos_skip() {
  echo -e "  ${YELLOW}SKIP${NC}: $*"
  ((CHAOS_SKIP++)) || true
}

chaos_section() {
  echo ""
  echo -e "${BOLD}${CYAN}--- $* ---${NC}"
}

# ---- Docker helpers ------------------------------------------------------- #

# Build the chaos Docker image if not already present.
chaos_build_image() {
  local dockerfile_dir
  dockerfile_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  if ! docker image inspect "$CHAOS_IMAGE" &>/dev/null; then
    chaos_log "Building Docker image $CHAOS_IMAGE ..."
    docker build -t "$CHAOS_IMAGE" "$dockerfile_dir"
  fi
}

# Start a fresh container with NET_ADMIN capability.
# Usage: chaos_start_container <suffix>
chaos_start_container() {
  local suffix="${1:?container suffix required}"
  local name="${CHAOS_CONTAINER_PREFIX}-${suffix}"

  # Stop any leftover container with the same name.
  docker rm -f "$name" &>/dev/null || true

  chaos_log "Starting container $name with NET_ADMIN ..."
  docker run -d --rm \
    --name "$name" \
    --cap-add NET_ADMIN \
    -p 0:22 \
    "$CHAOS_IMAGE" >/dev/null

  # Retrieve allocated port.
  CHAOS_SSH_PORT=$(docker port "$name" 22 2>/dev/null | head -1 | sed 's/.*://')
  CHAOS_CONTAINER_NAME="$name"

  # Wait for sshd to become reachable.
  local retries=30
  while ! chaos_ssh_cmd "true" &>/dev/null; do
    ((retries--)) || { chaos_fail "Container $name sshd did not start"; return 1; }
    sleep 0.3
  done

  chaos_log "Container $name ready on port $CHAOS_SSH_PORT"
}

# Stop the current container.
chaos_stop_container() {
  if [[ -n "${CHAOS_CONTAINER_NAME:-}" ]]; then
    chaos_log "Stopping container $CHAOS_CONTAINER_NAME ..."
    docker stop "$CHAOS_CONTAINER_NAME" &>/dev/null || true
    unset CHAOS_CONTAINER_NAME
    unset CHAOS_SSH_PORT
  fi
}

# ---- SSH helpers ---------------------------------------------------------- #

# Run a command on the test container via SSH.
# Usage: chaos_ssh_cmd "command" [extra_ssh_opts...]
chaos_ssh_cmd() {
  local cmd="${1:?command required}"
  shift
  sshpass -p "$CHAOS_SSH_PASS" \
    ssh $CHAOS_SSH_OPTS "$@" \
    -p "$CHAOS_SSH_PORT" \
    "${CHAOS_SSH_USER}@${CHAOS_SSH_HOST}" \
    "$cmd"
}

# Run a command as root inside the container via docker exec.
# Required for tc/iptables operations (need root).
# Usage: chaos_docker_exec "command"
chaos_docker_exec() {
  local cmd="${1:?command required}"
  docker exec "$CHAOS_CONTAINER_NAME" bash -c "$cmd"
}

# Test if SSH connection works (returns 0 on success).
# Usage: chaos_ssh_probe [timeout_seconds]
chaos_ssh_probe() {
  local timeout="${1:-5}"
  timeout "$timeout" sshpass -p "$CHAOS_SSH_PASS" \
    ssh $CHAOS_SSH_OPTS \
    -p "$CHAOS_SSH_PORT" \
    "${CHAOS_SSH_USER}@${CHAOS_SSH_HOST}" \
    "echo ok" &>/dev/null
}

# Measure SSH round-trip time in milliseconds.
# Usage: chaos_ssh_rtt -> prints ms value
chaos_ssh_rtt() {
  local start end elapsed_ms
  start=$(date +%s%N)
  chaos_ssh_cmd "echo rtt-probe" &>/dev/null
  end=$(date +%s%N)
  elapsed_ms=$(( (end - start) / 1000000 ))
  echo "$elapsed_ms"
}

# ---- Network chaos helpers ------------------------------------------------ #

# Apply tc netem delay to eth0 inside the container.
# Usage: chaos_apply_delay <delay_ms> [jitter_ms]
chaos_apply_delay() {
  local delay="${1:?delay required}"
  local jitter="${2:-0}"
  chaos_docker_exec "tc qdisc del dev eth0 root 2>/dev/null; tc qdisc add dev eth0 root netem delay ${delay}ms ${jitter}ms"
  chaos_log "Applied delay ${delay}ms jitter ${jitter}ms"
}

# Apply tc netem packet loss.
# Usage: chaos_apply_loss <percent>
chaos_apply_loss() {
  local pct="${1:?loss percent required}"
  chaos_docker_exec "tc qdisc del dev eth0 root 2>/dev/null; tc qdisc add dev eth0 root netem loss ${pct}%"
  chaos_log "Applied packet loss ${pct}%"
}

# Apply tc tbf bandwidth limit.
# Usage: chaos_apply_bandwidth <rate> [burst] [latency]
chaos_apply_bandwidth() {
  local rate="${1:?rate required}"
  local burst="${2:-1540}"
  local latency="${3:-50ms}"
  chaos_docker_exec "tc qdisc del dev eth0 root 2>/dev/null; tc qdisc add dev eth0 root tbf rate ${rate} burst ${burst} latency ${latency}"
  chaos_log "Applied bandwidth limit ${rate}"
}

# Block outbound traffic on a port via iptables (simulate disconnect).
# Usage: chaos_block_port <port>
chaos_block_port() {
  local port="${1:?port required}"
  chaos_docker_exec "iptables -A OUTPUT -p tcp --sport ${port} -j DROP && iptables -A INPUT -p tcp --dport ${port} -j DROP"
  chaos_log "Blocked port ${port} via iptables"
}

# Unblock traffic (flush iptables rules).
# Usage: chaos_unblock_all
chaos_unblock_all() {
  chaos_docker_exec "iptables -F INPUT; iptables -F OUTPUT"
  chaos_log "Flushed all iptables rules"
}

# Remove all tc qdisc rules.
# Usage: chaos_clear_tc
chaos_clear_tc() {
  chaos_docker_exec "tc qdisc del dev eth0 root 2>/dev/null || true"
  chaos_log "Cleared tc rules"
}

# Full network cleanup.
# Usage: chaos_network_cleanup
chaos_network_cleanup() {
  chaos_docker_exec "tc qdisc del dev eth0 root 2>/dev/null; iptables -F INPUT 2>/dev/null; iptables -F OUTPUT 2>/dev/null" || true
  chaos_log "Network conditions reset"
}

# ---- tmux helpers --------------------------------------------------------- #

# List all tmux sessions on the server.
chaos_tmux_list() {
  chaos_ssh_cmd "tmux list-sessions -F '#{session_name}' 2>/dev/null" || true
}

# Create a tmux session. Uses docker exec to avoid SSH interference from chaos.
# Usage: chaos_tmux_create <session-name> [initial-command]
chaos_tmux_create() {
  local name="${1:?session name required}"
  local cmd="${2:-}"
  if [[ -n "$cmd" ]]; then
    chaos_docker_exec "su - testuser -c \"tmux new-session -d -s '${name}' '${cmd}'\""
  else
    chaos_docker_exec "su - testuser -c \"tmux new-session -d -s '${name}'\""
  fi
}

# Kill a tmux session via docker exec (bypass network chaos).
# Usage: chaos_tmux_kill <session-name>
chaos_tmux_kill() {
  local name="${1:?session name required}"
  chaos_docker_exec "su - testuser -c \"tmux kill-session -t '${name}'\"" || true
}

# Check if a tmux session exists (via docker exec to bypass chaos).
# Usage: chaos_tmux_has_session <session-name>
chaos_tmux_has_session() {
  local name="${1:?session name required}"
  chaos_docker_exec "su - testuser -c \"tmux has-session -t '${name}' 2>/dev/null\""
}

# List tmux sessions via docker exec (bypasses network chaos).
chaos_tmux_list_direct() {
  chaos_docker_exec "su - testuser -c 'tmux list-sessions -F \"#{session_name}\" 2>/dev/null'" || true
}

# Count tmux sessions via docker exec.
chaos_tmux_count_direct() {
  local count
  count=$(chaos_docker_exec "su - testuser -c 'tmux list-sessions 2>/dev/null | wc -l'" || echo "0")
  echo "${count// /}"
}

# Set a tmux environment variable (via docker exec).
chaos_tmux_set_env() {
  local session="${1:?session required}"
  local var="${2:?var required}"
  local val="${3:?val required}"
  chaos_docker_exec "su - testuser -c \"tmux set-environment -t '${session}' '${var}' '${val}'\""
}

# Get a tmux environment variable (via docker exec).
chaos_tmux_get_env() {
  local session="${1:?session required}"
  local var="${2:?var required}"
  chaos_docker_exec "su - testuser -c \"tmux show-environment -t '${session}' '${var}' 2>/dev/null\"" | cut -d= -f2-
}

# Send keys to a tmux session (via docker exec).
chaos_tmux_send_keys() {
  local session="${1:?session required}"
  local keys="${2:?keys required}"
  chaos_docker_exec "su - testuser -c \"tmux send-keys -t '${session}' '${keys}' Enter\""
}

# Capture tmux pane content (via docker exec).
chaos_tmux_capture() {
  local session="${1:?session required}"
  chaos_docker_exec "su - testuser -c \"tmux capture-pane -t '${session}' -p\""
}

# ---- State file helpers --------------------------------------------------- #

# Write a JSON state file via docker exec (bypass network).
chaos_write_state() {
  local client_id="${1:?client-id required}"
  local json="${2:?json content required}"
  chaos_docker_exec "su - testuser -c \"mkdir -p '${CHAOS_REMOTE_STATE_DIR}' && cat > '${CHAOS_REMOTE_STATE_DIR}/${client_id}.json'\" << 'STATEEOF'
${json}
STATEEOF"
}

# Read a state file via docker exec (bypass network).
chaos_read_state() {
  local client_id="${1:?client-id required}"
  chaos_docker_exec "cat '${CHAOS_REMOTE_STATE_DIR}/${client_id}.json' 2>/dev/null" || true
}

# Check state file integrity (valid JSON, expected fields).
chaos_verify_state() {
  local client_id="${1:?client-id required}"
  local content
  content=$(chaos_read_state "$client_id")
  if [[ -z "$content" ]]; then
    echo "MISSING"
    return 1
  fi
  if echo "$content" | jq empty 2>/dev/null; then
    echo "VALID"
    return 0
  else
    echo "CORRUPT"
    return 1
  fi
}

# ---- Process verification ------------------------------------------------- #

# Check that sshd is still running inside the container.
chaos_verify_sshd_running() {
  chaos_docker_exec "pgrep -x sshd >/dev/null"
}

# Check that no crash dumps or core files exist.
chaos_verify_no_crashes() {
  local cores
  cores=$(chaos_docker_exec "find /home/testuser -name 'core*' -o -name '*.crash' 2>/dev/null | wc -l")
  [[ "${cores// /}" -eq 0 ]]
}

# ---- Assertion helpers ---------------------------------------------------- #

assert_ok() {
  local desc="$1"; shift
  if "$@" &>/dev/null; then
    chaos_pass "$desc"
  else
    chaos_fail "$desc"
  fi
}

assert_fail() {
  local desc="$1"; shift
  if "$@" &>/dev/null; then
    chaos_fail "$desc (expected failure but succeeded)"
  else
    chaos_pass "$desc"
  fi
}

assert_contains() {
  local desc="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    chaos_pass "$desc"
  else
    chaos_fail "$desc (expected '$needle' in output)"
  fi
}

assert_eq() {
  local desc="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" == "$expected" ]]; then
    chaos_pass "$desc"
  else
    chaos_fail "$desc (expected '$expected', got '$actual')"
  fi
}

assert_num_ge() {
  local desc="$1"
  local actual="$2"
  local threshold="$3"
  if [[ "$actual" -ge "$threshold" ]]; then
    chaos_pass "$desc"
  else
    chaos_fail "$desc (expected >= $threshold, got $actual)"
  fi
}

assert_num_le() {
  local desc="$1"
  local actual="$2"
  local threshold="$3"
  if [[ "$actual" -le "$threshold" ]]; then
    chaos_pass "$desc"
  else
    chaos_fail "$desc (expected <= $threshold, got $actual)"
  fi
}

# ---- Cleanup & summary --------------------------------------------------- #

chaos_register_cleanup() {
  trap 'chaos_network_cleanup 2>/dev/null; chaos_stop_container' EXIT
}

chaos_summary() {
  echo ""
  echo -e "${CYAN}========================================${NC}"
  echo -e "  ${GREEN}Passed:${NC}  $CHAOS_PASS"
  echo -e "  ${RED}Failed:${NC}  $CHAOS_FAIL"
  echo -e "  ${YELLOW}Skipped:${NC} $CHAOS_SKIP"
  echo -e "${CYAN}========================================${NC}"

  if [[ "$CHAOS_FAIL" -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

# ---- Prerequisite checks ------------------------------------------------- #

chaos_check_prereqs() {
  local missing=0

  for tool in docker sshpass ssh timeout; do
    if ! command -v "$tool" &>/dev/null; then
      chaos_fail "Required tool not found: $tool"
      ((missing++)) || true
    fi
  done

  if ! docker info &>/dev/null 2>&1; then
    chaos_fail "Docker daemon not running or insufficient permissions"
    ((missing++)) || true
  fi

  if [[ "$missing" -gt 0 ]]; then
    echo "Install missing tools and ensure Docker is running."
    exit 1
  fi
}
