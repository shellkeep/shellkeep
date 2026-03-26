#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 9: Concurrent SSH connections during reconnection max 5
#
# Method: Simulate reconnection of many tabs simultaneously. Monitor SSH
# connection attempts using ss/netstat to verify no more than 5 concurrent
# outgoing SSH connections are established at once.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_root
ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=9
TARGET_MAX=5
SSH_HOST="${BENCH_SSH_HOST:-localhost}"
SSH_PORT="${BENCH_SSH_PORT:-22}"

bench_on_exit cleanup
cleanup() {
    iptables -D OUTPUT -p tcp --dport "$SSH_PORT" -j DROP 2>/dev/null || true
    kill "$SK_PID" 2>/dev/null || true
    kill "$MONITOR_PID" 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# Setup: launch with 20 tabs (to force queued reconnection)
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep with many tabs..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 5

for i in $(seq 2 20); do
    xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
    sleep 0.5
done
sleep 5

# ---------------------------------------------------------------------------
# Monitor concurrent SSH connections in background
# ---------------------------------------------------------------------------
MONITOR_LOG="$BENCH_RESULTS_DIR/slo09_concurrent.dat"
> "$MONITOR_LOG"

monitor_connections() {
    while true; do
        count=$(ss -tn state syn-sent state established dst "$SSH_HOST" dport = :"$SSH_PORT" 2>/dev/null \
            | tail -n +2 | wc -l)
        echo "$(now_ms) $count" >> "$MONITOR_LOG"
        sleep 0.1
    done
}
monitor_connections &
MONITOR_PID=$!

# ---------------------------------------------------------------------------
# Trigger reconnection by dropping and restoring network
# ---------------------------------------------------------------------------
bench_log "Dropping network to trigger mass reconnection..."
iptables -A OUTPUT -p tcp --dport "$SSH_PORT" -j DROP
sleep 5

bench_log "Restoring network — monitoring concurrent connections..."
iptables -D OUTPUT -p tcp --dport "$SSH_PORT" -j DROP
sleep 15  # Wait for all reconnections

kill "$MONITOR_PID" 2>/dev/null || true

# ---------------------------------------------------------------------------
# Analyze: find peak concurrent connections
# ---------------------------------------------------------------------------
if [[ -s "$MONITOR_LOG" ]]; then
    max_concurrent=$(awk '{ if ($2+0 > max) max = $2+0 } END { print max }' "$MONITOR_LOG")
    bench_log "Peak concurrent SSH connections: $max_concurrent"

    if (( max_concurrent <= TARGET_MAX )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "concurrent_ssh_reconnect" "$TARGET_MAX" "$max_concurrent" "connections" "$status"
else
    record_result "$SLO_NUM" "concurrent_ssh_reconnect" "$TARGET_MAX" "N/A" "connections" "ERROR"
fi
