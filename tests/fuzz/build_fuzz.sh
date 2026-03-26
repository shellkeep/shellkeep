#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Build script for shellkeep fuzz targets.
#
# Usage:
#   ./tests/fuzz/build_fuzz.sh          # Build all fuzz targets
#   ./tests/fuzz/build_fuzz.sh clean    # Clean build directory
#
# Requirements:
#   - clang with libFuzzer support (clang >= 6.0)
#   - All shellkeep build dependencies (glib, json-glib, libssh)
#
# The script configures meson with:
#   - CC=clang (required for libFuzzer)
#   - -fsanitize=address,undefined (ASan + UBSan)
#   - -fsanitize=fuzzer (libFuzzer instrumentation)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/build-fuzz"

if [ "${1:-}" = "clean" ]; then
    echo "Cleaning fuzz build directory..."
    rm -rf "$BUILD_DIR"
    echo "Done."
    exit 0
fi

# Check for clang.
if ! command -v clang &>/dev/null; then
    echo "ERROR: clang is required for libFuzzer. Install clang >= 6.0."
    exit 1
fi

CLANG_VERSION=$(clang --version | head -1)
echo "Using: $CLANG_VERSION"

# Configure with meson.
if [ ! -f "$BUILD_DIR/build.ninja" ]; then
    echo "Configuring meson build..."
    CC=clang CXX=clang++ meson setup "$BUILD_DIR" "$PROJECT_ROOT" \
        -Dfuzz=true \
        -Dtests=false \
        -Dbuildtype=debug \
        -Dc_args="-fsanitize=address,undefined,fuzzer-no-link -fno-omit-frame-pointer" \
        -Dc_link_args="-fsanitize=address,undefined"
fi

# Build fuzz targets.
echo "Building fuzz targets..."
ninja -C "$BUILD_DIR"

echo ""
echo "Fuzz targets built successfully in $BUILD_DIR/"
echo ""
echo "Run a target with:"
echo "  $BUILD_DIR/tests/fuzz/fuzz_state_load $SCRIPT_DIR/corpus/state/ -max_total_time=600"
echo "  $BUILD_DIR/tests/fuzz/fuzz_config_load $SCRIPT_DIR/corpus/config/ -max_total_time=600"
echo "  $BUILD_DIR/tests/fuzz/fuzz_history_read $SCRIPT_DIR/corpus/history/ -max_total_time=600"
echo "  $BUILD_DIR/tests/fuzz/fuzz_tmux_control_parse $SCRIPT_DIR/corpus/tmux_control/ -max_total_time=600"
echo "  $BUILD_DIR/tests/fuzz/fuzz_ssh_config_parse $SCRIPT_DIR/corpus/ssh_config/ -max_total_time=600"
