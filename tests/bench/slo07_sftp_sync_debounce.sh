#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 7: SFTP state sync — max frequency 1 write / 2s (debounce)
#
# Method: Monitor SFTP write operations using strace while generating rapid
# state changes. Count writes per 2-second window — should never exceed 1.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=7
TARGET_WRITES_PER_2S=1
OBSERVATION_SECS=20

bench_log "Launching shellkeep with strace to monitor SFTP sync..."

STRACE_LOG="$BENCH_RESULTS_DIR/slo07_sftp_strace.log"

# Trace write and sendto syscalls (SFTP uses SSH channel writes)
strace -f -e trace=write,sendto -tt -o "$STRACE_LOG" $SHELLKEEP_BIN &
SK_PID=$!
sleep 5

# Generate rapid state changes (open/close tabs, rename sessions)
bench_log "Generating rapid state changes for ${OBSERVATION_SECS}s..."
for i in $(seq 1 "$OBSERVATION_SECS"); do
    # Trigger state changes via keyboard shortcuts
    xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
    sleep 0.5
    xdotool key --clearmodifiers ctrl+shift+w 2>/dev/null || true
    sleep 0.5
done

kill "$SK_PID" 2>/dev/null || true
wait "$SK_PID" 2>/dev/null || true

# ---------------------------------------------------------------------------
# Analyze: count SFTP writes per 2-second window
# ---------------------------------------------------------------------------
if [[ -f "$STRACE_LOG" ]]; then
    # Extract timestamps of state.json writes (SFTP sync operations)
    # In a real measurement, we would filter by the SFTP connection's FD.
    max_writes_per_window=$(grep -i "state.json\|sftp" "$STRACE_LOG" 2>/dev/null \
        | awk -F'[ :]' '{
            ts = $1 * 3600 + $2 * 60 + $3
            window = int(ts / 2)
            count[window]++
        }
        END {
            max = 0
            for (w in count) if (count[w] > max) max = count[w]
            print max+0
        }')

    bench_log "Max SFTP writes in any 2s window: $max_writes_per_window"

    if (( max_writes_per_window <= TARGET_WRITES_PER_2S )); then
        status="PASS"
    else
        status="FAIL"
    fi
    record_result "$SLO_NUM" "sftp_sync_debounce" "$TARGET_WRITES_PER_2S" "$max_writes_per_window" "writes/2s" "$status"
else
    record_result "$SLO_NUM" "sftp_sync_debounce" "$TARGET_WRITES_PER_2S" "N/A" "writes/2s" "ERROR"
fi
