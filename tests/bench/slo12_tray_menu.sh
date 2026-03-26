#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 12: Tray menu opening (50 windows) < 100ms
#
# Method: Launch shellkeep with 50 windows/sessions, click tray icon,
# measure time until menu is rendered. Uses xdotool for click and
# window detection for menu appearance.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=12
TARGET_MS=100
NUM_WINDOWS=50

# ---------------------------------------------------------------------------
# Setup: launch shellkeep with many sessions
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep and opening $NUM_WINDOWS sessions..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 5

for i in $(seq 2 "$NUM_WINDOWS"); do
    xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
    sleep 0.2
done
sleep 5

# ---------------------------------------------------------------------------
# Benchmark: measure tray menu open time
# ---------------------------------------------------------------------------
RESULTS_FILE="$BENCH_RESULTS_DIR/slo12_tray.dat"
> "$RESULTS_FILE"

ITERATIONS="${BENCH_ITERATIONS:-10}"

for iter in $(seq 1 "$ITERATIONS"); do
    # Find tray icon location (AppIndicator / StatusNotifierItem)
    # This is desktop-environment specific. Try common approaches.
    tray_wid=$(xdotool search --name "shellkeep" --class "tray" 2>/dev/null | head -1)

    if [[ -z "$tray_wid" ]]; then
        # Alternative: find the system tray area
        tray_wid=$(xdotool search --name "shellkeep" 2>/dev/null | head -1)
    fi

    if [[ -n "$tray_wid" ]]; then
        ts_click=$(now_ns)
        xdotool click --window "$tray_wid" 1 2>/dev/null || true

        # Wait for menu to appear
        for attempt in $(seq 1 100); do
            if xdotool search --name "shellkeep" --class "menu" 2>/dev/null | head -1 >/dev/null; then
                ts_menu=$(now_ns)
                latency=$(elapsed_ms "$ts_click" "$ts_menu")
                echo "$latency" >> "$RESULTS_FILE"
                break
            fi
            sleep 0.001
        done

        # Close menu
        xdotool key Escape 2>/dev/null || true
        sleep 0.5
    fi
done

# ---------------------------------------------------------------------------
# Analyze
# ---------------------------------------------------------------------------
if [[ -s "$RESULTS_FILE" ]]; then
    p95=$(cat "$RESULTS_FILE" | percentile_95)
    bench_log "Tray menu open time p95: ${p95}ms"

    if (( p95 <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "tray_menu_50_windows" "$TARGET_MS" "$p95" "ms" "$status"
else
    bench_log "Could not measure tray menu — tray icon not found"
    record_result "$SLO_NUM" "tray_menu_50_windows" "$TARGET_MS" "N/A" "ms" "ERROR"
fi

kill "$SK_PID" 2>/dev/null || true
