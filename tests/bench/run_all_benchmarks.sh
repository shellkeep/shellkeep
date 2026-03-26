#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# run_all_benchmarks.sh — Run all 15 SLO benchmarks and produce results CSV.
#
# Usage:
#   sudo ./run_all_benchmarks.sh           # Run all benchmarks
#   sudo ./run_all_benchmarks.sh 1 3 15    # Run specific SLOs
#
# Prerequisites:
#   - shellkeep built and in PATH (or set SHELLKEEP_BIN)
#   - Running X11/Wayland display server
#   - Root access (for tc netem / iptables)
#   - xdotool, tmux, python3, strace installed
#
# Output: /tmp/shellkeep-bench-results/results.csv

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

# ---------------------------------------------------------------------------
# Check prerequisites
# ---------------------------------------------------------------------------
check_prereqs() {
    local missing=()
    for cmd in xdotool tmux python3 strace ss awk; do
        if ! command -v "$cmd" &>/dev/null; then
            missing+=("$cmd")
        fi
    done
    if (( ${#missing[@]} > 0 )); then
        bench_fail "Missing required tools: ${missing[*]}"
    fi
}

check_prereqs

# ---------------------------------------------------------------------------
# Determine which SLOs to run
# ---------------------------------------------------------------------------
ALL_SLOS=(01 02 03 04 05 06 07 08 09 10 11 12 13 14 15)
REQUESTED_SLOS=()

if (( $# > 0 )); then
    for arg in "$@"; do
        REQUESTED_SLOS+=("$(printf '%02d' "$arg")")
    done
else
    REQUESTED_SLOS=("${ALL_SLOS[@]}")
fi

# ---------------------------------------------------------------------------
# Clean previous results
# ---------------------------------------------------------------------------
mkdir_results
rm -f "$BENCH_RESULTS_DIR/results.csv"

# ---------------------------------------------------------------------------
# Run benchmarks
# ---------------------------------------------------------------------------
PASS_COUNT=0
FAIL_COUNT=0
ERROR_COUNT=0

for slo in "${REQUESTED_SLOS[@]}"; do
    script="$SCRIPT_DIR/slo${slo}_*.sh"
    # Expand glob
    script_file=$(ls $script 2>/dev/null | head -1)

    if [[ -z "$script_file" ]]; then
        bench_log "No benchmark script found for SLO $slo — skipping"
        continue
    fi

    bench_log "=========================================="
    bench_log "Running SLO $slo: $script_file"
    bench_log "=========================================="

    if bash "$script_file"; then
        bench_log "SLO $slo completed"
    else
        bench_log "SLO $slo script exited with error"
        ERROR_COUNT=$(( ERROR_COUNT + 1 ))
    fi

    # Brief pause between benchmarks to let system settle
    sleep 2
done

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
bench_log "=========================================="
bench_log "BENCHMARK RESULTS SUMMARY"
bench_log "=========================================="

if [[ -f "$BENCH_RESULTS_DIR/results.csv" ]]; then
    # Print results table
    echo ""
    printf "%-5s %-35s %-12s %-12s %-8s\n" "SLO" "Metric" "Target" "Measured" "Status"
    printf "%-5s %-35s %-12s %-12s %-8s\n" "---" "---" "---" "---" "---"
    tail -n +2 "$BENCH_RESULTS_DIR/results.csv" | while IFS=',' read -r slo metric target measured unit status; do
        printf "%-5s %-35s %-12s %-12s %-8s\n" "$slo" "$metric" "${target}${unit}" "${measured}${unit}" "$status"
        case "$status" in
            PASS) ;;
            FAIL) ;;
            *)    ;;
        esac
    done
    echo ""
    bench_log "Full results: $BENCH_RESULTS_DIR/results.csv"
else
    bench_log "No results file generated."
fi
