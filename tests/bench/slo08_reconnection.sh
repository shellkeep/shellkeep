#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 8: Reconnection after short drop (<5s) < 10s for all tabs
#
# Method: Establish connections, simulate network drop via iptables for <5s,
# restore, measure time until all tabs are reconnected.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_root
ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=8
TARGET_MS=10000
DROP_DURATION=3  # seconds
SSH_HOST="${BENCH_SSH_HOST:-localhost}"
SSH_PORT="${BENCH_SSH_PORT:-22}"
NUM_TABS=5

bench_on_exit cleanup
cleanup() {
    # Restore network
    iptables -D OUTPUT -p tcp --dport "$SSH_PORT" -j DROP 2>/dev/null || true
    kill "$SK_PID" 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# Setup: launch shellkeep with multiple tabs
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep with $NUM_TABS tabs..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 5

for i in $(seq 2 "$NUM_TABS"); do
    xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
    sleep 1
done
sleep 3  # Let all connections establish

# ---------------------------------------------------------------------------
# Simulate network drop
# ---------------------------------------------------------------------------
bench_log "Dropping network for ${DROP_DURATION}s..."
iptables -A OUTPUT -p tcp --dport "$SSH_PORT" -j DROP
sleep "$DROP_DURATION"

bench_log "Restoring network..."
ts_restore=$(now_ns)
iptables -D OUTPUT -p tcp --dport "$SSH_PORT" -j DROP

# ---------------------------------------------------------------------------
# Measure reconnection time
# ---------------------------------------------------------------------------
all_reconnected=false
for attempt in $(seq 1 1000); do  # up to 10s at 10ms intervals
    # Check if all tmux sessions are alive (shellkeep reconnects them)
    active=$(tmux list-sessions -F '#{session_name}' 2>/dev/null \
        | grep -c "shellkeep" || echo 0)
    if (( active >= NUM_TABS )); then
        ts_done=$(now_ns)
        all_reconnected=true
        break
    fi
    sleep 0.01
done

if $all_reconnected; then
    elapsed=$(elapsed_ms "$ts_restore" "$ts_done")
    bench_log "All tabs reconnected in ${elapsed}ms"

    if (( elapsed <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "reconnection_short_drop" "$TARGET_MS" "$elapsed" "ms" "$status"
else
    bench_log "Not all tabs reconnected within timeout"
    record_result "$SLO_NUM" "reconnection_short_drop" "$TARGET_MS" "TIMEOUT" "ms" "FAIL"
fi
