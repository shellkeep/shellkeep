#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Run all shellkeep fuzz targets.
#
# Usage:
#   ./tests/fuzz/run_all_fuzz.sh              # 10 min per target (default)
#   ./tests/fuzz/run_all_fuzz.sh 60           # 60 sec per target (quick)
#   ./tests/fuzz/run_all_fuzz.sh 0            # Run until manually stopped
#
# Crashes are saved to tests/fuzz/crashes/<target>/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/build-fuzz"
CRASH_DIR="$SCRIPT_DIR/crashes"

# Seconds per target (default: 600 = 10 minutes).
MAX_TIME="${1:-600}"

if [ ! -d "$BUILD_DIR/tests/fuzz" ]; then
    echo "ERROR: Fuzz targets not built. Run ./tests/fuzz/build_fuzz.sh first."
    exit 1
fi

# Define targets: (executable, corpus_dir, name)
declare -A TARGETS=(
    ["fuzz_state_load"]="corpus/state"
    ["fuzz_config_load"]="corpus/config"
    ["fuzz_history_read"]="corpus/history"
    ["fuzz_tmux_control_parse"]="corpus/tmux_control"
    ["fuzz_ssh_config_parse"]="corpus/ssh_config"
)

TOTAL_CRASHES=0

for target in "${!TARGETS[@]}"; do
    corpus="${TARGETS[$target]}"
    target_crash_dir="$CRASH_DIR/$target"
    mkdir -p "$target_crash_dir"

    echo "========================================"
    echo "Running: $target"
    echo "  Corpus:  $SCRIPT_DIR/$corpus/"
    echo "  Crashes: $target_crash_dir/"
    if [ "$MAX_TIME" -gt 0 ] 2>/dev/null; then
        echo "  Timeout: ${MAX_TIME}s"
    else
        echo "  Timeout: unlimited (Ctrl+C to stop)"
    fi
    echo "========================================"

    FUZZ_ARGS=(
        "$SCRIPT_DIR/$corpus/"
        "-artifact_prefix=$target_crash_dir/"
        "-print_final_stats=1"
        "-max_len=65536"
    )

    if [ "$MAX_TIME" -gt 0 ] 2>/dev/null; then
        FUZZ_ARGS+=("-max_total_time=$MAX_TIME")
    fi

    set +e
    "$BUILD_DIR/tests/fuzz/$target" "${FUZZ_ARGS[@]}"
    EXIT_CODE=$?
    set -e

    # libFuzzer returns 77 on crash found, 0 on clean exit.
    if [ "$EXIT_CODE" -ne 0 ]; then
        CRASH_COUNT=$(find "$target_crash_dir" -name "crash-*" -o -name "oom-*" -o -name "timeout-*" 2>/dev/null | wc -l)
        echo "  CRASHES FOUND: $CRASH_COUNT in $target"
        TOTAL_CRASHES=$((TOTAL_CRASHES + CRASH_COUNT))
    else
        echo "  No crashes found in $target"
    fi
    echo ""
done

echo "========================================"
echo "Fuzzing complete."
echo "Total crashes found: $TOTAL_CRASHES"
echo "Crash artifacts: $CRASH_DIR/"
echo "========================================"

if [ "$TOTAL_CRASHES" -gt 0 ]; then
    exit 1
fi
