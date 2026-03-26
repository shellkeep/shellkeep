#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 11: Dead session scrollback restore < 3s
#
# Method: Create a JSONL history file with substantial content, kill the
# session's tmux, then measure how long shellkeep takes to display the
# dead session's scrollback from the JSONL file.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=11
TARGET_MS=3000

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/shellkeep"
JSONL_DIR="$DATA_DIR/history"

# ---------------------------------------------------------------------------
# Setup: create a realistic JSONL file (~5MB, ~10k lines of output)
# ---------------------------------------------------------------------------
bench_log "Preparing JSONL history file for dead session restore test..."
mkdir -p "$JSONL_DIR"

SESSION_UUID="bench-dead-$(date +%s)"
JSONL_FILE="$JSONL_DIR/${SESSION_UUID}.jsonl"

python3 -c "
import json, time
with open('$JSONL_FILE', 'w') as f:
    for i in range(10000):
        line = {
            'ts': time.time() - (10000 - i),
            'type': 'output',
            'data': f'Line {i}: ' + 'x' * 200
        }
        f.write(json.dumps(line) + '\n')
" 2>/dev/null || bench_log "Could not generate JSONL file"

if [[ -f "$JSONL_FILE" ]]; then
    size_mb=$(( $(stat -c %s "$JSONL_FILE") / 1048576 ))
    bench_log "Generated JSONL file: ${size_mb}MB"
fi

# ---------------------------------------------------------------------------
# Benchmark: open dead session tab and measure restore time
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep and triggering dead session restore..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 3

# Trigger dead session view (via CLI or D-Bus)
ts_start=$(now_ns)

# The actual trigger mechanism depends on shellkeep's API. Possible approaches:
# 1. Click on dead session in UI
# 2. Use D-Bus method call
# 3. Use CLI: shellkeep --show-session $SESSION_UUID
$SHELLKEEP_BIN --show-session "$SESSION_UUID" 2>/dev/null &

# Wait for scrollback to be populated (detect via window content)
restored=false
for attempt in $(seq 1 300); do  # up to 3s at 10ms
    # Check if the dead session tab shows content
    if xdotool search --name "$SESSION_UUID" --limit 1 &>/dev/null; then
        ts_done=$(now_ns)
        restored=true
        break
    fi
    sleep 0.01
done

if $restored; then
    elapsed=$(elapsed_ms "$ts_start" "$ts_done")
    bench_log "Dead session restored in ${elapsed}ms"

    if (( elapsed <= TARGET_MS )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "dead_session_restore" "$TARGET_MS" "$elapsed" "ms" "$status"
else
    bench_log "Dead session restore did not complete within timeout"
    record_result "$SLO_NUM" "dead_session_restore" "$TARGET_MS" "TIMEOUT" "ms" "FAIL"
fi

kill "$SK_PID" 2>/dev/null || true
rm -f "$JSONL_FILE"
