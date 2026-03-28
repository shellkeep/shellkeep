#!/bin/sh
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Wrapper script for the portable shellkeep tarball.
# Checks for required shared libraries and gives actionable
# install instructions before launching the binary.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN="$SCRIPT_DIR/shellkeep-bin"

if [ ! -x "$BIN" ]; then
    echo "error: shellkeep binary not found at $BIN" >&2
    exit 1
fi

# Map library sonames to package names per distro family
check_deps() {
    missing=""
    missing_libs=""

    for lib in \
        libQt6Widgets.so.6 \
        libQt6Core.so.6 \
        libQt6Gui.so.6 \
        libQt6Network.so.6 \
        libssh.so.4 \
        libglib-2.0.so.0 \
        libgio-2.0.so.0 \
        libjson-glib-1.0.so.0; do
        if ! ldconfig -p 2>/dev/null | grep -q "$lib"; then
            missing_libs="$missing_libs $lib"
        fi
    done

    if [ -z "$missing_libs" ]; then
        return 0
    fi

    echo "shellkeep: missing system libraries:" >&2
    for lib in $missing_libs; do
        echo "  - $lib" >&2
    done
    echo "" >&2

    # Detect distro and suggest install command
    if [ -f /etc/os-release ]; then
        . /etc/os-release
    fi

    case "${ID:-}${ID_LIKE:-}" in
        *debian*|*ubuntu*|*mint*|*pop*)
            echo "Install on Debian/Ubuntu/Mint:" >&2
            echo "  sudo apt install qt6-base-dev libssh-dev libglib2.0-dev libjson-glib-dev" >&2
            ;;
        *fedora*|*rhel*|*centos*)
            echo "Install on Fedora/RHEL:" >&2
            echo "  sudo dnf install qt6-qtbase-devel libssh-devel glib2-devel json-glib-devel" >&2
            ;;
        *arch*|*manjaro*)
            echo "Install on Arch/Manjaro:" >&2
            echo "  sudo pacman -S qt6-base libssh glib2 json-glib" >&2
            ;;
        *suse*|*opensuse*)
            echo "Install on openSUSE:" >&2
            echo "  sudo zypper install qt6-base-devel libssh-devel glib2-devel json-glib-devel" >&2
            ;;
        *)
            echo "Install the Qt6, libssh, GLib, and json-glib development packages for your distro." >&2
            ;;
    esac

    echo "" >&2
    echo "Or use the AppImage instead — no dependencies needed:" >&2
    echo "  https://github.com/shellkeep/shellkeep/releases" >&2
    return 1
}

if ! check_deps; then
    exit 1
fi

exec "$BIN" "$@"
