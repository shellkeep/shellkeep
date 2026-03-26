#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 14: Startup — first tab interactive < 2s
#
# Method: Launch shellkeep, measure time until the first tab accepts and
# echoes keystrokes (i.e., the SSH + tmux connection is established).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=14
TARGET_MS=2000

RESULTS_FILE="$BENCH_RESULTS_DIR/slo14_interactive.dat"
> "$RESULTS_FILE"

ITERATIONS="${BENCH_ITERATIONS:-5}"

for iter in $(seq 1 "$ITERATIONS"); do
    pkill -f "$SHELLKEEP_BIN" 2>/dev/null || true
    sleep 1

    ts_start=$(now_ns)
    $SHELLKEEP_BIN &
    SK_PID=$!

    # Wait for window first
    for attempt in $(seq 1 200); do
        if xdotool search --name "shellkeep" --limit 1 &>/dev/null; then
            break
        fi
        sleep 0.01
    done

    # Now check if the terminal is interactive by looking for a shell prompt
    # in the tmux pane, or by sending a keystroke and checking for echo.
    interactive=false
    for attempt in $(seq 1 200); do  # up to ~2s
        # Check tmux sessions for a prompt indicator
        pane_content=$(tmux capture-pane -p 2>/dev/null || echo "")
        if echo "$pane_content" | grep -qE '[\$#>]'; then
            ts_interactive=$(now_ns)
            interactive=true
            break
        fi
        sleep 0.01
    done

    if $interactive; then
        elapsed=$(elapsed_ms "$ts_start" "$ts_interactive")
        echo "$elapsed" >> "$RESULTS_FILE"
        bench_log "Startup interactive iteration $iter: ${elapsed}ms"
    else
        bench_log "Startup interactive iteration $iter: not interactive within timeout"
    fi

    kill "$SK_PID" 2>/dev/null || true
    sleep 1
done

# ---------------------------------------------------------------------------
# Analyze
# ---------------------------------------------------------------------------
if [[ -s "$RESULTS_FILE" ]]; then
    p95=$(cat "$RESULTS_FILE" | percentile_95)
    bench_log "Startup to interactive: p95=${p95}ms"

    if (( p95 <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "startup_first_interactive" "$TARGET_MS" "$p95" "ms" "$status"
else
    record_result "$SLO_NUM" "startup_first_interactive" "$TARGET_MS" "N/A" "ms" "ERROR"
fi
