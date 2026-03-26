#!/bin/sh
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# NFR-DIST-01: AppImage build script
# Builds an AppImage using linuxdeploy with the GTK plugin.
#
# Prerequisites:
#   - linuxdeploy (https://github.com/linuxdeploy/linuxdeploy)
#   - linuxdeploy-plugin-gtk (https://github.com/linuxdeploy/linuxdeploy-plugin-gtk)
#   - Meson build completed with: meson setup build --prefix=/usr
#
# Usage:
#   scripts/build-appimage.sh [build-dir]
#
# Output:
#   ShellKeep-<version>-<arch>.AppImage in current directory

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

BUILD_DIR="${1:-$PROJECT_DIR/build}"

if [ ! -d "$BUILD_DIR" ]; then
    echo "Error: Build directory '$BUILD_DIR' not found." >&2
    echo "Run 'meson setup $BUILD_DIR --prefix=/usr' first." >&2
    exit 1
fi

# Install to AppDir
APPDIR="$BUILD_DIR/AppDir"
rm -rf "$APPDIR"
DESTDIR="$APPDIR" meson install -C "$BUILD_DIR" --no-rebuild

# Fetch linuxdeploy if not available
LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy}"
if ! command -v "$LINUXDEPLOY" >/dev/null 2>&1; then
    ARCH="$(uname -m)"
    LINUXDEPLOY="$BUILD_DIR/linuxdeploy-$ARCH.AppImage"
    if [ ! -x "$LINUXDEPLOY" ]; then
        echo "Downloading linuxdeploy..."
        wget -q -O "$LINUXDEPLOY" \
            "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"
        chmod +x "$LINUXDEPLOY"
    fi
fi

# Fetch GTK plugin if not available
LINUXDEPLOY_PLUGIN_GTK="${LINUXDEPLOY_PLUGIN_GTK:-}"
if [ -z "$LINUXDEPLOY_PLUGIN_GTK" ]; then
    ARCH="$(uname -m)"
    LINUXDEPLOY_PLUGIN_GTK="$BUILD_DIR/linuxdeploy-plugin-gtk.sh"
    if [ ! -x "$LINUXDEPLOY_PLUGIN_GTK" ]; then
        echo "Downloading linuxdeploy-plugin-gtk..."
        wget -q -O "$LINUXDEPLOY_PLUGIN_GTK" \
            "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
        chmod +x "$LINUXDEPLOY_PLUGIN_GTK"
    fi
fi

# Read version from meson.build
VERSION="$(meson introspect "$BUILD_DIR" --projectinfo 2>/dev/null | \
    python3 -c 'import sys,json; print(json.load(sys.stdin)["version"])' 2>/dev/null || echo "0.1.0")"

export VERSION
export ARCH="${ARCH:-$(uname -m)}"

# Write AppImage update information (NFR-DIST-09)
# This enables appimageupdatetool to check for updates
export UPDATE_INFORMATION="gh-releases-zsync|shellkeep|shellkeep|latest|ShellKeep-*$ARCH.AppImage.zsync"

echo "Building AppImage v${VERSION} for ${ARCH}..."

# Run linuxdeploy with GTK plugin
# Bundles libvte, libssh, libayatana-appindicator as required by NFR-DIST-01
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --desktop-file "$APPDIR/usr/share/applications/shellkeep.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/shellkeep.png" \
    --plugin gtk \
    --output appimage

APPIMAGE_NAME="ShellKeep-${VERSION}-${ARCH}.AppImage"
if [ -f "ShellKeep-${VERSION}-${ARCH}.AppImage" ]; then
    echo "AppImage created: $APPIMAGE_NAME"
elif ls ShellKeep*.AppImage 1>/dev/null 2>&1; then
    # linuxdeploy may use a slightly different naming scheme
    ACTUAL="$(ls ShellKeep*.AppImage | head -1)"
    echo "AppImage created: $ACTUAL"
else
    echo "Error: AppImage was not created." >&2
    exit 1
fi

echo "Done."
