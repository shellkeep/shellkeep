#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Shared helper functions for shellkeep first-run-experience tests.
# Source this file from individual scenario scripts.
#
# These tests verify the 6 first-use combinations from WAVE-09 (Agent 28).
# They test the underlying connection flow logic (SSH connect, tmux detect,
# auth, host key verification) via real Docker containers.
#
# GUI-specific behaviors (TOFU dialogs, toast notifications, visual feedback)
# require a display server and are documented for manual verification in each
# scenario script.

set -euo pipefail

# ---- Color output --------------------------------------------------------- #

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# ---- Counters ------------------------------------------------------------- #

FR_PASS=0
FR_FAIL=0
FR_SKIP=0
FR_SCENARIO=""

# ---- Configuration -------------------------------------------------------- #

# Docker images for different scenarios.
FR_IMAGE_FULL="${FR_IMAGE_FULL:-sk-fr-full}"
FR_IMAGE_NO_TMUX="${FR_IMAGE_NO_TMUX:-sk-fr-no-tmux}"
FR_IMAGE_TMUX2="${FR_IMAGE_TMUX2:-sk-fr-tmux2}"

FR_CONTAINER_PREFIX="${FR_CONTAINER_PREFIX:-sk-fr}"

# SSH credentials matching the integration Dockerfile.
FR_SSH_HOST="${FR_SSH_HOST:-127.0.0.1}"
FR_SSH_PORT="${FR_SSH_PORT:-}"
FR_SSH_USER="${FR_SSH_USER:-testuser}"
FR_SSH_PASS="${FR_SSH_PASS:-testpass}"

# Base SSH options (disable host key checking for container management).
FR_SSH_OPTS_NOCHECK="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"

# State directory on the remote server.
FR_REMOTE_STATE_DIR="/home/testuser/.terminal-state"

# ---- Logging -------------------------------------------------------------- #

fr_log() {
  echo -e "${CYAN}[first-run:${FR_SCENARIO}]${NC} $*"
}

fr_pass() {
  echo -e "  ${GREEN}PASS${NC}: $*"
  ((FR_PASS++)) || true
}

fr_fail() {
  echo -e "  ${RED}FAIL${NC}: $*"
  ((FR_FAIL++)) || true
}

fr_skip() {
  echo -e "  ${YELLOW}SKIP${NC}: $*"
  ((FR_SKIP++)) || true
}

fr_section() {
  echo ""
  echo -e "${CYAN}--- $* ---${NC}"
}

fr_manual_note() {
  echo -e "  ${YELLOW}MANUAL${NC}: $*"
}

# ---- Docker image builders ------------------------------------------------ #

# Build the full test image (tmux 3.x, sshd, key auth + password).
fr_build_image_full() {
  local dockerfile_dir
  dockerfile_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../integration" && pwd)"
  if ! docker image inspect "$FR_IMAGE_FULL" &>/dev/null; then
    fr_log "Building full Docker image $FR_IMAGE_FULL ..."
    docker build -t "$FR_IMAGE_FULL" "$dockerfile_dir"
  fi
}

# Build the no-tmux image.
fr_build_image_no_tmux() {
  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  if ! docker image inspect "$FR_IMAGE_NO_TMUX" &>/dev/null; then
    fr_log "Building no-tmux Docker image $FR_IMAGE_NO_TMUX ..."
    docker build -t "$FR_IMAGE_NO_TMUX" -f "$script_dir/Dockerfile.no-tmux" "$script_dir"
  fi
}

# Build the tmux 2.x image.
fr_build_image_tmux2() {
  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  if ! docker image inspect "$FR_IMAGE_TMUX2" &>/dev/null; then
    fr_log "Building tmux2 Docker image $FR_IMAGE_TMUX2 ..."
    docker build -t "$FR_IMAGE_TMUX2" -f "$script_dir/Dockerfile.tmux2" "$script_dir"
  fi
}

# ---- Container helpers ---------------------------------------------------- #

# Start a container from a given image.
# Usage: fr_start_container <suffix> <image>
fr_start_container() {
  local suffix="${1:?container suffix required}"
  local image="${2:?image required}"
  local name="${FR_CONTAINER_PREFIX}-${suffix}"

  # Stop any leftover container with the same name.
  docker rm -f "$name" &>/dev/null || true

  fr_log "Starting container $name from $image ..."
  docker run -d --rm \
    --name "$name" \
    --cap-add NET_ADMIN \
    -p 0:22 \
    "$image" >/dev/null

  # Retrieve allocated port.
  FR_SSH_PORT=$(docker port "$name" 22 2>/dev/null | head -1 | sed 's/.*://')
  FR_CONTAINER_NAME="$name"

  # Wait for sshd to become reachable.
  local retries=30
  while ! fr_ssh_cmd_nocheck "true" &>/dev/null; do
    ((retries--)) || { fr_fail "Container $name sshd did not start"; return 1; }
    sleep 0.3
  done

  fr_log "Container $name ready on port $FR_SSH_PORT"
}

# Stop the current container.
fr_stop_container() {
  if [[ -n "${FR_CONTAINER_NAME:-}" ]]; then
    fr_log "Stopping container $FR_CONTAINER_NAME ..."
    docker stop "$FR_CONTAINER_NAME" &>/dev/null || true
    unset FR_CONTAINER_NAME
    unset FR_SSH_PORT
  fi
}

# Run a command as root inside the container via docker exec.
fr_docker_exec() {
  local cmd="${1:?command required}"
  docker exec "$FR_CONTAINER_NAME" bash -c "$cmd"
}

# ---- SSH helpers ---------------------------------------------------------- #

# Run a command via SSH with host key checking DISABLED (for management).
fr_ssh_cmd_nocheck() {
  local cmd="${1:?command required}"
  sshpass -p "$FR_SSH_PASS" \
    ssh $FR_SSH_OPTS_NOCHECK \
    -p "$FR_SSH_PORT" \
    "${FR_SSH_USER}@${FR_SSH_HOST}" \
    "$cmd"
}

# Run a command via SSH with STRICT host key checking (tests TOFU logic).
# Uses a specific known_hosts file to control host key state.
# Usage: fr_ssh_cmd_strict <known_hosts_file> <command>
fr_ssh_cmd_strict() {
  local known_hosts="${1:?known_hosts file required}"
  local cmd="${2:?command required}"
  sshpass -p "$FR_SSH_PASS" \
    ssh -o StrictHostKeyChecking=ask \
        -o UserKnownHostsFile="$known_hosts" \
        -o LogLevel=ERROR \
        -o BatchMode=yes \
    -p "$FR_SSH_PORT" \
    "${FR_SSH_USER}@${FR_SSH_HOST}" \
    "$cmd" 2>&1
}

# Attempt SSH with key-based auth only (no password fallback).
# Usage: fr_ssh_cmd_keyauth <identity_file> <known_hosts_file> <command>
fr_ssh_cmd_keyauth() {
  local identity="${1:?identity file required}"
  local known_hosts="${2:?known_hosts file required}"
  local cmd="${3:?command required}"
  ssh -o StrictHostKeyChecking=no \
      -o UserKnownHostsFile="$known_hosts" \
      -o PasswordAuthentication=no \
      -o PubkeyAuthentication=yes \
      -o IdentityFile="$identity" \
      -o IdentitiesOnly=yes \
      -o LogLevel=ERROR \
      -o BatchMode=yes \
      -o ConnectTimeout=5 \
  -p "$FR_SSH_PORT" \
  "${FR_SSH_USER}@${FR_SSH_HOST}" \
  "$cmd" 2>&1
}

# Attempt SSH connection and return the exit code + output (capture errors).
# Usage: result=$(fr_ssh_attempt <options...>); echo "exit=$?"
fr_ssh_attempt() {
  ssh "$@" 2>&1
}

# ---- SSH key helpers ------------------------------------------------------ #

# Generate a temporary SSH key pair for testing.
# Usage: fr_generate_ssh_key <output_path>
fr_generate_ssh_key() {
  local path="${1:?output path required}"
  ssh-keygen -t ed25519 -f "$path" -N "" -q
}

# Install a public key in the container for a user.
# Usage: fr_install_pubkey <pubkey_path>
fr_install_pubkey() {
  local pubkey_path="${1:?pubkey path required}"
  local pubkey
  pubkey=$(cat "$pubkey_path")
  fr_docker_exec "mkdir -p /home/$FR_SSH_USER/.ssh && \
    chmod 700 /home/$FR_SSH_USER/.ssh && \
    echo '$pubkey' >> /home/$FR_SSH_USER/.ssh/authorized_keys && \
    chmod 600 /home/$FR_SSH_USER/.ssh/authorized_keys && \
    chown -R $FR_SSH_USER:$FR_SSH_USER /home/$FR_SSH_USER/.ssh"
}

# ---- ssh-agent helpers ---------------------------------------------------- #

# Start a temporary ssh-agent and load a key.
# Usage: eval $(fr_start_agent <key_path>)
# Sets SSH_AUTH_SOCK and SSH_AGENT_PID.
fr_start_agent() {
  local key_path="${1:?key path required}"
  eval "$(ssh-agent -s)" >/dev/null 2>&1
  ssh-add "$key_path" 2>/dev/null
  echo "SSH_AUTH_SOCK=$SSH_AUTH_SOCK; SSH_AGENT_PID=$SSH_AGENT_PID"
}

# Kill the temporary ssh-agent.
fr_stop_agent() {
  if [[ -n "${SSH_AGENT_PID:-}" ]]; then
    kill "$SSH_AGENT_PID" 2>/dev/null || true
    unset SSH_AUTH_SOCK
    unset SSH_AGENT_PID
  fi
}

# ---- tmux helpers --------------------------------------------------------- #

# Check tmux version on the remote server.
# Returns version string or empty on failure.
fr_tmux_version() {
  fr_ssh_cmd_nocheck "tmux -V 2>/dev/null" || echo ""
}

# Check if tmux is installed on the remote server.
fr_tmux_installed() {
  fr_ssh_cmd_nocheck "which tmux >/dev/null 2>&1"
}

# List tmux sessions on the remote server.
fr_tmux_list() {
  fr_ssh_cmd_nocheck "tmux list-sessions -F '#{session_name}' 2>/dev/null" || true
}

# ---- Host key helpers ----------------------------------------------------- #

# Get the server's host key fingerprint.
# Usage: fr_get_host_fingerprint <port>
fr_get_host_fingerprint() {
  ssh-keyscan -p "$FR_SSH_PORT" "$FR_SSH_HOST" 2>/dev/null | \
    ssh-keygen -l -f - 2>/dev/null | head -1
}

# Populate a known_hosts file with the current server's key.
# Usage: fr_populate_known_hosts <output_file>
fr_populate_known_hosts() {
  local output="${1:?output file required}"
  ssh-keyscan -p "$FR_SSH_PORT" "$FR_SSH_HOST" 2>/dev/null > "$output"
}

# ---- Assertion helpers ---------------------------------------------------- #

assert_ok() {
  local desc="$1"; shift
  if "$@" &>/dev/null; then
    fr_pass "$desc"
  else
    fr_fail "$desc"
  fi
}

assert_fail() {
  local desc="$1"; shift
  if "$@" &>/dev/null; then
    fr_fail "$desc (expected failure but succeeded)"
  else
    fr_pass "$desc"
  fi
}

assert_contains() {
  local desc="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    fr_pass "$desc"
  else
    fr_fail "$desc (expected '$needle' in output)"
  fi
}

assert_not_contains() {
  local desc="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    fr_pass "$desc"
  else
    fr_fail "$desc (did not expect '$needle' in output)"
  fi
}

assert_eq() {
  local desc="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" == "$expected" ]]; then
    fr_pass "$desc"
  else
    fr_fail "$desc (expected '$expected', got '$actual')"
  fi
}

assert_num_eq() {
  local desc="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" -eq "$expected" ]]; then
    fr_pass "$desc"
  else
    fr_fail "$desc (expected $expected, got $actual)"
  fi
}

assert_exit_code() {
  local desc="$1"
  local expected_code="$2"
  local actual_code="$3"
  if [[ "$actual_code" -eq "$expected_code" ]]; then
    fr_pass "$desc"
  else
    fr_fail "$desc (expected exit code $expected_code, got $actual_code)"
  fi
}

# ---- Cleanup & summary --------------------------------------------------- #

fr_register_cleanup() {
  trap '_fr_cleanup' EXIT
}

_fr_cleanup() {
  fr_stop_container
  fr_stop_agent
  # Clean up temp directory if set.
  if [[ -n "${FR_TMPDIR:-}" && -d "${FR_TMPDIR:-}" ]]; then
    rm -rf "$FR_TMPDIR"
  fi
}

fr_summary() {
  echo ""
  echo -e "${CYAN}========================================${NC}"
  echo -e "  Scenario: ${FR_SCENARIO}"
  echo -e "  ${GREEN}Passed:${NC}  $FR_PASS"
  echo -e "  ${RED}Failed:${NC}  $FR_FAIL"
  echo -e "  ${YELLOW}Skipped:${NC} $FR_SKIP"
  echo -e "${CYAN}========================================${NC}"

  if [[ "$FR_FAIL" -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

# ---- Prerequisite checks ------------------------------------------------- #

fr_check_prereqs() {
  local missing=0

  for tool in docker sshpass ssh ssh-keygen ssh-keyscan ssh-agent ssh-add; do
    if ! command -v "$tool" &>/dev/null; then
      fr_fail "Required tool not found: $tool"
      ((missing++)) || true
    fi
  done

  if [[ "$missing" -gt 0 ]]; then
    echo "Install missing tools and retry."
    exit 1
  fi
}

# ---- Temp directory ------------------------------------------------------- #

# Create a temporary directory for test artifacts (keys, known_hosts, etc.).
# Sets FR_TMPDIR.
fr_create_tmpdir() {
  FR_TMPDIR=$(mktemp -d /tmp/sk-first-run-XXXXXX)
  chmod 700 "$FR_TMPDIR"
}
