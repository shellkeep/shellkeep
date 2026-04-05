#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# One-time setup script for the GUI test droplet.
# Installs all packages needed to run shellkeep under Xvfb with xdotool automation.
#
# Usage: Run on the droplet (or via ssh):
#   bash setup_droplet.sh

set -euo pipefail

echo "=== shellkeep GUI test droplet setup ==="

# ---- System packages -------------------------------------------------------- #

export DEBIAN_FRONTEND=noninteractive

apt-get update -qq

apt-get install -y --no-install-recommends \
  xvfb \
  xdotool \
  x11-utils \
  dbus \
  dbus-x11 \
  imagemagick \
  mesa-vulkan-drivers \
  libxkbcommon0 \
  libwayland-client0 \
  libvulkan1 \
  libfontconfig1 \
  libgtk-3-0 \
  libayatana-appindicator3-1 \
  libxdo3 \
  fonts-dejavu \
  jq \
  tmux \
  openssh-server \
  procps

# ---- Test directory --------------------------------------------------------- #

mkdir -p /opt/shellkeep-gui-test
chmod 755 /opt/shellkeep-gui-test

# ---- Verify sshd is running ------------------------------------------------ #

if ! systemctl is-active --quiet sshd 2>/dev/null && \
   ! systemctl is-active --quiet ssh 2>/dev/null; then
  systemctl start ssh 2>/dev/null || systemctl start sshd 2>/dev/null || true
fi

# ---- Verify ---------------------------------------------------------------- #

echo ""
echo "Verifying installed packages..."

declare -a REQUIRED_CMDS=(xvfb-run xdotool xdpyinfo import jq tmux dbus-launch)
MISSING=0

for cmd in "${REQUIRED_CMDS[@]}"; do
  if command -v "$cmd" &>/dev/null; then
    echo "  OK: $cmd"
  else
    echo "  MISSING: $cmd"
    ((MISSING++)) || true
  fi
done

if [[ "$MISSING" -gt 0 ]]; then
  echo ""
  echo "ERROR: $MISSING required command(s) not found."
  exit 1
fi

echo ""
echo "=== Setup complete ==="
echo "Test directory: /opt/shellkeep-gui-test"
