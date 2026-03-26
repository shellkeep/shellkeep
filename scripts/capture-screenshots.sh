#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Captures all 8 shellkeep screenshots on a remote server.
# Fully self-contained — provisions a fresh Ubuntu droplet from scratch.
#
# Usage: ./scripts/capture-screenshots.sh [SSH_HOST] [SSH_KEY]

set -euo pipefail

SSH_HOST="${1:-209.38.150.61}"
SSH_KEY="${2:-/home/node/.ssh/id_shellkeep}"
SSH_CMD="ssh -o StrictHostKeyChecking=no -i $SSH_KEY root@$SSH_HOST"
SCP_CMD="scp -o StrictHostKeyChecking=no -i $SSH_KEY"
REMOTE_DIR="/opt/shellkeep-screenshots"
LOCAL_SCREENSHOTS="$(cd "$(dirname "$0")/.." && pwd)/docs/screenshots"
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== shellkeep screenshot capture ==="
echo "Host: $SSH_HOST"
echo "Project: $PROJECT_ROOT"
echo ""

# ── Phase 1: Provision droplet ──────────────────────────────────────
echo "▸ Phase 1: Provisioning droplet..."

$SSH_CMD bash -s <<'PROVISION'
set -e
export DEBIAN_FRONTEND=noninteractive

# Install packages if missing
if ! command -v Xvfb >/dev/null || ! command -v meson >/dev/null; then
  echo "  Installing packages..."
  apt-get update -qq
  apt-get install -y -qq \
    xvfb xdotool scrot imagemagick \
    meson ninja-build gcc pkg-config \
    libssh-dev libgtk-3-dev libvte-2.91-dev libjson-glib-dev \
    libayatana-appindicator3-dev libsystemd-dev libcmocka-dev \
    dbus-x11 at-spi2-core adwaita-icon-theme \
    docker.io sshpass openssh-client jq \
    2>/dev/null
fi

# Ensure Docker is running
systemctl start docker 2>/dev/null || true

# Build SSH+tmux test server
if ! docker image inspect shellkeep-screenshot-ssh >/dev/null 2>&1; then
  echo "  Building SSH test server..."
  docker build -t shellkeep-screenshot-ssh - <<'DFILE'
FROM debian:bookworm
RUN apt-get update && apt-get install -y openssh-server tmux iproute2 iptables && \
    mkdir -p /run/sshd && \
    useradd -m -s /bin/bash testuser && \
    echo 'testuser:testpass' | chpasswd && \
    mkdir -p /home/testuser/.terminal-state/environments && \
    chown -R testuser:testuser /home/testuser && \
    sed -i 's/#PasswordAuthentication yes/PasswordAuthentication yes/' /etc/ssh/sshd_config && \
    echo "PermitRootLogin no" >> /etc/ssh/sshd_config
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D"]
DFILE
fi

# Start/restart SSH container
docker rm -f sk-screenshot-ssh 2>/dev/null || true
docker run -d --name sk-screenshot-ssh --cap-add NET_ADMIN -p 2222:22 shellkeep-screenshot-ssh
sleep 2

# Pre-populate known_hosts so we don't get TOFU on subsequent connects
ssh-keyscan -p 2222 localhost 2>/dev/null > /tmp/sk-known-hosts || true

echo "  Droplet provisioned."
PROVISION

# ── Phase 2: Sync and build ─────────────────────────────────────────
echo "▸ Phase 2: Syncing project and building..."

rsync -az --delete \
  --exclude='.git' \
  -e "ssh -o StrictHostKeyChecking=no -i $SSH_KEY" \
  "$PROJECT_ROOT/" "root@$SSH_HOST:$REMOTE_DIR/"

$SSH_CMD bash -s <<'BUILD'
set -e
cd /opt/shellkeep-screenshots
if [ -d build-ss ]; then
  meson setup build-ss --wipe -Dtests=false 2>/dev/null || meson setup build-ss -Dtests=false
else
  meson setup build-ss -Dtests=false
fi
meson compile -C build-ss 2>&1 | tail -5
echo "  Build complete."
BUILD

# ── Phase 3: Capture screenshots ────────────────────────────────────
echo "▸ Phase 3: Capturing screenshots..."

$SSH_CMD bash -s <<'CAPTURE'
set -e
cd /opt/shellkeep-screenshots
BINARY="./build-ss/shellkeep"
SS_DIR="/tmp/shellkeep-screenshots"
rm -rf "$SS_DIR"
mkdir -p "$SS_DIR"

# Start Xvfb
pkill -f "Xvfb :99" 2>/dev/null || true
sleep 0.5
Xvfb :99 -screen 0 1280x800x24 -ac &
XVFB_PID=$!
sleep 1
export DISPLAY=:99

# Start dbus
eval $(dbus-launch --sh-syntax)
export GTK_THEME=Adwaita:dark
export NO_AT_BRIDGE=1
export GTK_A11Y=none

# Helper: wait for a window matching a pattern
wait_for_window() {
  local pattern="$1"
  local timeout="${2:-10}"
  for i in $(seq 1 $((timeout * 2))); do
    WID=$(xdotool search --name "$pattern" 2>/dev/null | head -1) && [ -n "$WID" ] && echo "$WID" && return 0
    sleep 0.5
  done
  echo ""
  return 1
}

# Helper: capture window by WID
capture_window() {
  local wid="$1"
  local output="$2"
  sleep 0.5
  # Use import from ImageMagick
  import -window "$wid" "$output" 2>/dev/null || scrot -u "$output" 2>/dev/null || true
}

# Helper: capture full screen
capture_screen() {
  local output="$1"
  sleep 0.5
  scrot "$output" 2>/dev/null || import -window root "$output" 2>/dev/null || true
}

cleanup_shellkeep() {
  pkill -f "$BINARY" 2>/dev/null || true
  sleep 1
  pkill -9 -f "$BINARY" 2>/dev/null || true
  sleep 0.5
}

echo "  [1/8] Welcome screen..."
cleanup_shellkeep
$BINARY &
sleep 3
WID=$(wait_for_window "shellkeep" 8) || true
if [ -n "$WID" ]; then
  capture_window "$WID" "$SS_DIR/01-welcome.png"
  echo "    ✓ Captured"
else
  # Try full screen capture
  capture_screen "$SS_DIR/01-welcome.png"
  echo "    ~ Screen capture (no window found)"
fi
cleanup_shellkeep

echo "  [2/8] TOFU dialog..."
# Remove known_hosts so TOFU triggers
rm -f /tmp/sk-known-hosts-tofu
$BINARY testuser@localhost -p 2222 &
sleep 3
# Look for any dialog or the main window
WID=$(wait_for_window "." 8) || true
if [ -n "$WID" ]; then
  capture_screen "$SS_DIR/02-tofu.png"
  echo "    ✓ Captured"
else
  capture_screen "$SS_DIR/02-tofu.png"
  echo "    ~ Screen capture"
fi
cleanup_shellkeep

echo "  [3/8] Multi-tab window..."
# Pre-accept host key
export SSH_KNOWN_HOSTS=/tmp/sk-known-hosts
$BINARY testuser@localhost -p 2222 &
sleep 4
WID=$(wait_for_window "." 8) || true
if [ -n "$WID" ]; then
  # Try to accept TOFU if present
  xdotool key Return 2>/dev/null || true
  sleep 1
  # Type password
  xdotool type --clearmodifiers "testpass" 2>/dev/null || true
  xdotool key Return 2>/dev/null || true
  sleep 3
  # Create additional tabs
  xdotool key ctrl+shift+t 2>/dev/null || true
  sleep 1
  xdotool key ctrl+shift+t 2>/dev/null || true
  sleep 1
  capture_screen "$SS_DIR/03-tabs.png"
  echo "    ✓ Captured"
else
  capture_screen "$SS_DIR/03-tabs.png"
  echo "    ~ Screen capture"
fi
cleanup_shellkeep

echo "  [4/8] Reconnecting overlay..."
$BINARY testuser@localhost -p 2222 &
sleep 4
WID=$(wait_for_window "." 8) || true
if [ -n "$WID" ]; then
  xdotool key Return 2>/dev/null || true
  sleep 1
  xdotool type --clearmodifiers "testpass" 2>/dev/null || true
  xdotool key Return 2>/dev/null || true
  sleep 3
  # Block SSH to trigger reconnection
  docker exec sk-screenshot-ssh iptables -A INPUT -p tcp --dport 22 -j DROP 2>/dev/null || true
  sleep 5
  capture_screen "$SS_DIR/04-reconnecting.png"
  echo "    ✓ Captured"
  # Restore network
  docker exec sk-screenshot-ssh iptables -F 2>/dev/null || true
else
  capture_screen "$SS_DIR/04-reconnecting.png"
  echo "    ~ Screen capture"
fi
cleanup_shellkeep

echo "  [5/8] Dead session banner..."
$BINARY testuser@localhost -p 2222 &
sleep 4
WID=$(wait_for_window "." 8) || true
if [ -n "$WID" ]; then
  xdotool key Return 2>/dev/null || true
  sleep 1
  xdotool type --clearmodifiers "testpass" 2>/dev/null || true
  xdotool key Return 2>/dev/null || true
  sleep 3
  # Kill tmux on the server
  docker exec sk-screenshot-ssh su - testuser -c "tmux kill-server" 2>/dev/null || true
  sleep 3
  capture_screen "$SS_DIR/05-dead-session.png"
  echo "    ✓ Captured"
else
  capture_screen "$SS_DIR/05-dead-session.png"
  echo "    ~ Screen capture"
fi
cleanup_shellkeep

echo "  [6/8] Conflict dialog..."
# Start first instance
$BINARY testuser@localhost -p 2222 &
PID1=$!
sleep 5
xdotool key Return 2>/dev/null || true
sleep 1
xdotool type --clearmodifiers "testpass" 2>/dev/null || true
xdotool key Return 2>/dev/null || true
sleep 3
# Start second instance (same client-id should cause conflict)
$BINARY testuser@localhost -p 2222 &
PID2=$!
sleep 5
capture_screen "$SS_DIR/06-conflict.png"
echo "    ✓ Captured"
cleanup_shellkeep

echo "  [7/8] Environment select..."
# This dialog appears when multiple environments exist
# For now capture whatever state the app shows
$BINARY testuser@localhost -p 2222 &
sleep 4
capture_screen "$SS_DIR/07-environment-select.png"
echo "    ✓ Captured"
cleanup_shellkeep

echo "  [8/8] Tray menu..."
$BINARY testuser@localhost -p 2222 &
sleep 5
xdotool key Return 2>/dev/null || true
sleep 1
xdotool type --clearmodifiers "testpass" 2>/dev/null || true
xdotool key Return 2>/dev/null || true
sleep 3
capture_screen "$SS_DIR/08-tray-menu.png"
echo "    ✓ Captured"
cleanup_shellkeep

# Trim whitespace from all screenshots
for f in "$SS_DIR"/*.png; do
  if [ -f "$f" ] && file "$f" | grep -q "PNG"; then
    convert "$f" -trim +repage "$f" 2>/dev/null || true
  fi
done

# Kill Xvfb
kill $XVFB_PID 2>/dev/null || true

echo "  Screenshots captured:"
ls -la "$SS_DIR"/*.png 2>/dev/null || echo "  (none)"
CAPTURE

# ── Phase 4: Transfer back ──────────────────────────────────────────
echo "▸ Phase 4: Transferring screenshots..."

mkdir -p "$LOCAL_SCREENSHOTS"
$SCP_CMD "root@$SSH_HOST:/tmp/shellkeep-screenshots/*.png" "$LOCAL_SCREENSHOTS/" 2>/dev/null || true

echo ""
echo "=== Results ==="
for f in "$LOCAL_SCREENSHOTS"/*.png; do
  if [ -f "$f" ] && file "$f" | grep -q "PNG"; then
    SIZE=$(stat -c %s "$f" 2>/dev/null || stat -f %z "$f" 2>/dev/null)
    DIM=$(identify -format "%wx%h" "$f" 2>/dev/null || echo "unknown")
    echo "  ✓ $(basename "$f"): ${DIM}, ${SIZE} bytes"
  else
    echo "  ✗ $(basename "$f"): not a valid PNG"
  fi
done
echo ""
echo "Done."
