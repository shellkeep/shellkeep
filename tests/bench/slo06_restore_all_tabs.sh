#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 6: Restore 50 sessions — all tabs interactive < 15s
#
# Method: Same setup as SLO 5, but measure until all 50 tabs are connected
# and interactive (checking via tmux session count or tab status).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=6
TARGET_MS=15000
NUM_SESSIONS=50

# ---------------------------------------------------------------------------
# Setup: reuse state from SLO 5 or create fresh
# ---------------------------------------------------------------------------
STATE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/shellkeep"
if [[ ! -f "$STATE_DIR/state.json" ]]; then
    bench_log "No state file found. Run slo05 first or generating fresh state..."
    mkdir -p "$STATE_DIR"
    python3 -c "
import json, uuid
sessions = []
for i in range($NUM_SESSIONS):
    sessions.append({
        'session_uuid': str(uuid.uuid4()),
        'name': f'bench-session-{i}',
        'host': 'localhost',
        'port': 22,
        'status': 'disconnected'
    })
state = {'schema_version': 1, 'sessions': sessions}
print(json.dumps(state, indent=2))
" > "$STATE_DIR/state.json"
fi

# ---------------------------------------------------------------------------
# Benchmark
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep and waiting for all $NUM_SESSIONS tabs..."

ts_start=$(now_ns)
$SHELLKEEP_BIN &
SK_PID=$!

all_ready=false
for attempt in $(seq 1 1500); do  # up to 15s at 10ms intervals
    # Count active tmux sessions belonging to shellkeep
    active=$(tmux list-sessions -F '#{session_name}' 2>/dev/null \
        | grep -c "bench-session" || echo 0)
    if (( active >= NUM_SESSIONS )); then
        ts_done=$(now_ns)
        all_ready=true
        break
    fi
    sleep 0.01
done

if $all_ready; then
    elapsed=$(elapsed_ms "$ts_start" "$ts_done")
    bench_log "All $NUM_SESSIONS tabs ready in ${elapsed}ms"

    if (( elapsed <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "restore_50_all_tabs" "$TARGET_MS" "$elapsed" "ms" "$status"
else
    bench_log "Not all tabs became ready within timeout"
    record_result "$SLO_NUM" "restore_50_all_tabs" "$TARGET_MS" "TIMEOUT" "ms" "FAIL"
fi

kill "$SK_PID" 2>/dev/null || true
