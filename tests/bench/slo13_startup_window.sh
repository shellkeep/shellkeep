#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 13: Startup — window visible (with local cache) < 500ms
#
# Method: Ensure local state cache exists, launch shellkeep, measure time
# until GTK window is mapped (visible via xdotool search).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=13
TARGET_MS=500

RESULTS_FILE="$BENCH_RESULTS_DIR/slo13_startup.dat"
> "$RESULTS_FILE"

ITERATIONS="${BENCH_ITERATIONS:-5}"

# ---------------------------------------------------------------------------
# Ensure local cache exists (warm start)
# ---------------------------------------------------------------------------
STATE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/shellkeep"
mkdir -p "$STATE_DIR"
if [[ ! -f "$STATE_DIR/state.json" ]]; then
    echo '{"schema_version":1,"sessions":[]}' > "$STATE_DIR/state.json"
fi

# ---------------------------------------------------------------------------
# Benchmark: multiple startup measurements
# ---------------------------------------------------------------------------
for iter in $(seq 1 "$ITERATIONS"); do
    # Kill any existing instance
    pkill -f "$SHELLKEEP_BIN" 2>/dev/null || true
    sleep 1

    ts_start=$(now_ns)
    $SHELLKEEP_BIN &
    SK_PID=$!

    visible=false
    for attempt in $(seq 1 500); do  # up to 500ms at 1ms intervals
        if xdotool search --name "shellkeep" --limit 1 &>/dev/null; then
            ts_visible=$(now_ns)
            visible=true
            break
        fi
        sleep 0.001
    done

    if $visible; then
        elapsed=$(elapsed_ms "$ts_start" "$ts_visible")
        echo "$elapsed" >> "$RESULTS_FILE"
        bench_log "Startup iteration $iter: ${elapsed}ms"
    else
        bench_log "Startup iteration $iter: window not visible within timeout"
    fi

    kill "$SK_PID" 2>/dev/null || true
    sleep 1
done

# ---------------------------------------------------------------------------
# Analyze
# ---------------------------------------------------------------------------
if [[ -s "$RESULTS_FILE" ]]; then
    p95=$(cat "$RESULTS_FILE" | percentile_95)
    avg=$(cat "$RESULTS_FILE" | mean_value)
    bench_log "Startup to visible: p95=${p95}ms, avg=${avg}ms"

    if (( p95 <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "startup_window_visible" "$TARGET_MS" "$p95" "ms" "$status"
else
    record_result "$SLO_NUM" "startup_window_visible" "$TARGET_MS" "N/A" "ms" "ERROR"
fi
