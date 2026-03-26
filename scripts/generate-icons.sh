#!/bin/sh
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# NFR-DIST-04: Generate PNG icons from the SVG source at standard sizes.
#
# Requires: rsvg-convert (from librsvg2-bin) or inkscape
#
# Usage:
#   scripts/generate-icons.sh [svg-source]

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

SVG_SOURCE="${1:-$PROJECT_DIR/data/icons/shellkeep.svg}"
ICON_DIR="$PROJECT_DIR/data/icons"

if [ ! -f "$SVG_SOURCE" ]; then
    echo "Error: SVG source not found: $SVG_SOURCE" >&2
    exit 1
fi

mkdir -p "$ICON_DIR"

SIZES="48 128 256"

if command -v rsvg-convert >/dev/null 2>&1; then
    for size in $SIZES; do
        echo "Generating ${size}x${size} PNG..."
        rsvg-convert -w "$size" -h "$size" \
            "$SVG_SOURCE" \
            -o "$ICON_DIR/shellkeep-${size}.png"
    done
elif command -v inkscape >/dev/null 2>&1; then
    for size in $SIZES; do
        echo "Generating ${size}x${size} PNG..."
        inkscape "$SVG_SOURCE" \
            --export-type=png \
            --export-filename="$ICON_DIR/shellkeep-${size}.png" \
            --export-width="$size" \
            --export-height="$size" \
            2>/dev/null
    done
else
    echo "Warning: Neither rsvg-convert nor inkscape found." >&2
    echo "Creating minimal placeholder PNGs instead." >&2
    # Create minimal valid 1x1 PNG files as placeholders.
    # These should be regenerated from the SVG before release.
    for size in $SIZES; do
        printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\x0f\x00\x00\x01\x01\x00\x05\x18\xd8N\x00\x00\x00\x00IEND\xaeB`\x82' \
            > "$ICON_DIR/shellkeep-${size}.png"
    done
    echo "Placeholder PNGs created. Regenerate from SVG before release."
fi

echo "Icons generated in $ICON_DIR:"
ls -la "$ICON_DIR"/shellkeep-*.png
