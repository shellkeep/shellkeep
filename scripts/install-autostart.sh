#!/bin/sh
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# NFR-DIST-08: Optional autostart — copies .desktop to ~/.config/autostart/
# with --minimized flag. Never installed by default by the package.
#
# Usage:
#   scripts/install-autostart.sh [--remove]

set -e

AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
DESKTOP_FILE="shellkeep.desktop"
AUTOSTART_FILE="$AUTOSTART_DIR/$DESKTOP_FILE"

# Locate the autostart desktop template
# Check installed location first, then source tree
if [ -f "/usr/share/shellkeep/shellkeep-autostart.desktop" ]; then
    SOURCE="/usr/share/shellkeep/shellkeep-autostart.desktop"
elif [ -f "$(dirname "$0")/../data/shellkeep-autostart.desktop" ]; then
    SOURCE="$(dirname "$0")/../data/shellkeep-autostart.desktop"
else
    echo "Error: shellkeep-autostart.desktop not found" >&2
    exit 1
fi

if [ "$1" = "--remove" ]; then
    if [ -f "$AUTOSTART_FILE" ]; then
        rm -f "$AUTOSTART_FILE"
        echo "Autostart removed: $AUTOSTART_FILE"
    else
        echo "Autostart not configured (nothing to remove)."
    fi
    exit 0
fi

mkdir -p "$AUTOSTART_DIR"
cp "$SOURCE" "$AUTOSTART_FILE"
chmod 0644 "$AUTOSTART_FILE"
echo "Autostart installed: $AUTOSTART_FILE"
echo "ShellKeep will start minimized on next login."
