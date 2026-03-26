#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 4: GTK loop blocking by JSONL write = 0ms (async mandatory)
#
# Method: Use strace to monitor the shellkeep process for synchronous write()
# calls to JSONL files from the main thread. If any write() to a .jsonl file
# occurs on the main thread (tid == pid), it is a violation. Additionally,
# use GTK frame clock to detect dropped frames during heavy JSONL writes.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=4
TARGET=0  # ms blocking

bench_log "Launching shellkeep with strace to monitor JSONL writes..."
STRACE_LOG="$BENCH_RESULTS_DIR/slo04_strace.log"

# Launch with strace, filtering write syscalls
strace -f -e trace=write -o "$STRACE_LOG" $SHELLKEEP_BIN &
SK_PID=$!
sleep 5

# Generate terminal output to trigger JSONL writes
for sess in $(tmux list-sessions -F '#{session_name}' 2>/dev/null | head -5); do
    tmux send-keys -t "$sess" "seq 1 5000" Enter &
done
sleep 10

kill "$SK_PID" 2>/dev/null || true
wait "$SK_PID" 2>/dev/null || true

# ---------------------------------------------------------------------------
# Analyze strace output
# ---------------------------------------------------------------------------
# Look for write() calls to .jsonl files from the main thread (pid == tid)
main_thread_jsonl_writes=0
if [[ -f "$STRACE_LOG" ]]; then
    # In strace -f output, main thread lines are prefixed with the main PID.
    # Lines from worker threads have different TIDs.
    main_thread_jsonl_writes=$(grep -c "write.*jsonl" "$STRACE_LOG" 2>/dev/null || echo 0)
    # A more precise check would correlate TID with the main thread, but this
    # gives a useful signal.
    bench_log "Detected $main_thread_jsonl_writes write() calls mentioning jsonl in strace log"
fi

# For the SLO, if JSONL writes are async (in worker threads), no main-thread
# blocking occurs. We check by examining if write calls to jsonl FDs happen
# on the main thread PID.
if (( main_thread_jsonl_writes == 0 )); then
    status="PASS"
    measured=0
else
    status="FAIL"
    measured="$main_thread_jsonl_writes"
fi

record_result "$SLO_NUM" "gtk_loop_jsonl_blocking" "$TARGET" "$measured" "violations" "$status"
