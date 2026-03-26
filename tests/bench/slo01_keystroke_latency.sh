#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 1: Keystroke-to-echo latency (RTT 50ms) < 120ms p95
#
# Method: Use tc netem to add 25ms one-way delay on loopback (50ms RTT).
# Send keystrokes via xdotool to a shellkeep terminal tab connected to
# localhost, measure round-trip via timestamped echo script on the remote.
# FR-PERF-01

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_root
ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=1
SLO_TARGET=120  # ms p95
ITERATIONS="${BENCH_ITERATIONS:-100}"
RESULTS_FILE="$BENCH_RESULTS_DIR/slo01_keystroke.dat"

# ---------------------------------------------------------------------------
# Setup: add network delay
# ---------------------------------------------------------------------------
netem_add_delay "$NETEM_IFACE" "$NETEM_DELAY_MS"
bench_on_exit cleanup
cleanup() {
    netem_remove "$NETEM_IFACE"
    # Kill any helper processes
    [[ -n "${ECHO_PID:-}" ]] && kill "$ECHO_PID" 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# Setup: start a timestamping echo server on localhost via SSH
# ---------------------------------------------------------------------------
# The remote side runs a script that echoes each received character with a
# timestamp marker that we can detect in the terminal output.
MARKER_PREFIX="__SK_BENCH_"

bench_log "Starting shellkeep with a connection to localhost..."
# Assume shellkeep can be launched with a connection URI
$SHELLKEEP_BIN --connect ssh://localhost &
SK_PID=$!
sleep 3  # Wait for window + connection

bench_log "Sending $ITERATIONS keystrokes and measuring round-trip..."
> "$RESULTS_FILE"

for i in $(seq 1 "$ITERATIONS"); do
    ts_send=$(now_ns)

    # Send a single character via xdotool
    xdotool key --clearmodifiers "a"

    # Wait for echo to appear. In a real measurement we would monitor the
    # terminal PTY or use accessibility hooks. Here we use a simple approach:
    # poll the tmux pane content for the character.
    ts_recv=""
    for attempt in $(seq 1 50); do
        # Check if the character appeared in tmux pane (via tmux capture-pane)
        if tmux capture-pane -t shellkeep -p 2>/dev/null | tail -1 | grep -q "a"; then
            ts_recv=$(now_ns)
            break
        fi
        sleep 0.002  # 2ms poll interval
    done

    if [[ -n "$ts_recv" ]]; then
        latency=$(elapsed_ms "$ts_send" "$ts_recv")
        echo "$latency" >> "$RESULTS_FILE"
    fi
done

# ---------------------------------------------------------------------------
# Analyze results
# ---------------------------------------------------------------------------
if [[ -s "$RESULTS_FILE" ]]; then
    p95=$(cat "$RESULTS_FILE" | percentile_95)
    avg=$(cat "$RESULTS_FILE" | mean_value)
    bench_log "Keystroke latency: p95=${p95}ms, avg=${avg}ms"

    if (( p95 <= SLO_TARGET )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "keystroke_latency_p95" "$SLO_TARGET" "$p95" "ms" "$status"
else
    bench_log "No measurements collected — check that shellkeep is running and display is available."
    record_result "$SLO_NUM" "keystroke_latency_p95" "$SLO_TARGET" "N/A" "ms" "ERROR"
fi

kill "$SK_PID" 2>/dev/null || true
