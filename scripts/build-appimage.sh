#!/bin/sh
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# NFR-DIST-01: AppImage build script for Rust binary.
#
# Prerequisites:
#   - cargo build --release completed
#   - linuxdeploy available (downloaded automatically if missing)
#
# Usage:
#   scripts/build-appimage.sh
#
# Output:
#   ShellKeep-<version>-<arch>.AppImage in current directory

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

BINARY="$PROJECT_DIR/target/release/shellkeep"
if [ ! -x "$BINARY" ]; then
    echo "Error: Release binary not found at $BINARY" >&2
    echo "Run 'cargo build --release' first." >&2
    exit 1
fi

# Read version from Cargo.toml
VERSION="$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(uname -m)"

echo "Building AppImage v${VERSION} for ${ARCH}..."

# Create AppDir structure
APPDIR="$PROJECT_DIR/target/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$BINARY" "$APPDIR/usr/bin/shellkeep"
cp "$PROJECT_DIR/data/shellkeep.desktop" "$APPDIR/usr/share/applications/"

# Generate a simple icon if none exists
if [ -f "$PROJECT_DIR/data/shellkeep.png" ]; then
    cp "$PROJECT_DIR/data/shellkeep.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/"
else
    # Create a minimal 1x1 PNG placeholder
    printf '\x89PNG\r\n\x1a\n' > "$APPDIR/usr/share/icons/hicolor/256x256/apps/shellkeep.png"
    echo "Warning: No icon found, using placeholder."
fi

# Fetch linuxdeploy if not available
LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy}"
if ! command -v "$LINUXDEPLOY" >/dev/null 2>&1; then
    LINUXDEPLOY="$PROJECT_DIR/target/linuxdeploy-$ARCH.AppImage"
    if [ ! -x "$LINUXDEPLOY" ]; then
        echo "Downloading linuxdeploy..."
        wget -q -O "$LINUXDEPLOY" \
            "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"
        chmod +x "$LINUXDEPLOY"
    fi
fi

export VERSION
export ARCH
export APPIMAGE_EXTRACT_AND_RUN=1

# Run linuxdeploy — bundles shared libraries automatically
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --desktop-file "$APPDIR/usr/share/applications/shellkeep.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/shellkeep.png" \
    --output appimage

if ls ShellKeep*.AppImage 1>/dev/null 2>&1; then
    ACTUAL="$(ls ShellKeep*.AppImage | head -1)"
    echo "AppImage created: $ACTUAL"
else
    echo "Error: AppImage was not created." >&2
    exit 1
fi

echo "Done."
