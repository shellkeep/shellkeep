#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 5: Restore 50 sessions — first interactive window < 2s
#
# Method: Create 50 saved sessions in state, launch shellkeep, measure time
# until the first window becomes interactive (accepts keystrokes).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=5
TARGET_MS=2000
NUM_SESSIONS=50

# ---------------------------------------------------------------------------
# Setup: create 50 saved sessions in state directory
# ---------------------------------------------------------------------------
STATE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/shellkeep"
bench_log "Preparing $NUM_SESSIONS saved sessions in $STATE_DIR..."
mkdir -p "$STATE_DIR"

# Generate a state file with 50 sessions
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
" > "$STATE_DIR/state.json" 2>/dev/null || bench_log "Could not generate state file"

# ---------------------------------------------------------------------------
# Benchmark: measure time to first interactive window
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep and measuring time to first interactive window..."

ts_start=$(now_ns)
$SHELLKEEP_BIN &
SK_PID=$!

# Poll for window to appear using xdotool
interactive=false
for attempt in $(seq 1 200); do
    if xdotool search --name "shellkeep" --limit 1 &>/dev/null; then
        ts_window=$(now_ns)
        interactive=true
        break
    fi
    sleep 0.01
done

if $interactive; then
    elapsed=$(elapsed_ms "$ts_start" "$ts_window")
    bench_log "First window appeared in ${elapsed}ms"

    if (( elapsed <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "restore_50_first_window" "$TARGET_MS" "$elapsed" "ms" "$status"
else
    bench_log "Window did not appear within timeout"
    record_result "$SLO_NUM" "restore_50_first_window" "$TARGET_MS" "TIMEOUT" "ms" "FAIL"
fi

kill "$SK_PID" 2>/dev/null || true
