#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 15: CPU idle with 10 sessions < 0.5% of one core
#
# Method: Launch shellkeep with 10 active sessions, let it idle, sample CPU
# usage over 30 seconds using /proc/stat or top.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=15
TARGET_PERCENT="0.5"
NUM_SESSIONS=10
SAMPLE_DURATION=30  # seconds

# ---------------------------------------------------------------------------
# Setup: launch with 10 sessions
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep with $NUM_SESSIONS sessions..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 5

for i in $(seq 2 "$NUM_SESSIONS"); do
    xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
    sleep 1
done
sleep 5  # Let everything settle

# ---------------------------------------------------------------------------
# Measure CPU over extended period using /proc/stat
# ---------------------------------------------------------------------------
bench_log "Sampling CPU for ${SAMPLE_DURATION}s with $NUM_SESSIONS idle sessions..."

SAMPLE_FILE="$BENCH_RESULTS_DIR/slo15_cpu.dat"
> "$SAMPLE_FILE"

# Use /proc/<pid>/stat for precise measurement
if [[ -f "/proc/$SK_PID/stat" ]]; then
    # Read initial CPU time
    read_proc_cpu() {
        awk '{ print $14 + $15 }' "/proc/$1/stat" 2>/dev/null || echo 0
    }

    clk_tck=$(getconf CLK_TCK)
    cpu_start=$(read_proc_cpu "$SK_PID")
    wall_start=$(now_ns)

    sleep "$SAMPLE_DURATION"

    cpu_end=$(read_proc_cpu "$SK_PID")
    wall_end=$(now_ns)

    cpu_ticks=$(( cpu_end - cpu_start ))
    cpu_secs=$(awk "BEGIN { printf \"%.4f\", $cpu_ticks / $clk_tck }")
    wall_secs=$(awk "BEGIN { printf \"%.4f\", ($wall_end - $wall_start) / 1000000000 }")
    cpu_percent=$(awk "BEGIN { printf \"%.2f\", ($cpu_secs / $wall_secs) * 100 }")

    bench_log "CPU usage: ${cpu_percent}% (${cpu_secs}s CPU / ${wall_secs}s wall)"
else
    # Fallback to top
    cpu_percent=$(sample_cpu_percent "$SK_PID" "$SAMPLE_DURATION")
    bench_log "CPU usage (via top): ${cpu_percent}%"
fi

if awk "BEGIN { exit !($cpu_percent <= $TARGET_PERCENT) }"; then
    status="PASS"
else
    status="FAIL"
fi

record_result "$SLO_NUM" "cpu_idle_10_sessions" "$TARGET_PERCENT" "$cpu_percent" "%" "$status"

kill "$SK_PID" 2>/dev/null || true
