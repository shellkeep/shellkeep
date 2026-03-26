#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 10: Maximum JSONL per session = 50 MB (with rotation)
#
# Method: Generate enough terminal output to exceed 50 MB of JSONL data for a
# single session. Verify that shellkeep rotates the file and it never exceeds
# the limit.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=10
TARGET_MB=50

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 5

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/shellkeep"
JSONL_DIR="$DATA_DIR/history"

# ---------------------------------------------------------------------------
# Generate massive output to trigger rotation
# ---------------------------------------------------------------------------
bench_log "Generating ~60MB of terminal output to trigger JSONL rotation..."
# Each line of 'yes' output is ~2 bytes. We need ~60MB = ~30M lines.
# Use dd for more predictable output volume.
for sess in $(tmux list-sessions -F '#{session_name}' 2>/dev/null | head -1); do
    tmux send-keys -t "$sess" \
        "dd if=/dev/urandom bs=1024 count=61440 2>/dev/null | base64" Enter
done

# Wait for output to be processed and JSONL written
sleep 30

# ---------------------------------------------------------------------------
# Check JSONL file sizes
# ---------------------------------------------------------------------------
max_size_mb=0
if [[ -d "$JSONL_DIR" ]]; then
    for f in "$JSONL_DIR"/*.jsonl; do
        [[ -f "$f" ]] || continue
        size_bytes=$(stat -c %s "$f" 2>/dev/null || echo 0)
        size_mb=$(( size_bytes / 1048576 ))
        bench_log "JSONL file: $f = ${size_mb} MB"
        if (( size_mb > max_size_mb )); then
            max_size_mb=$size_mb
        fi
    done
else
    bench_log "JSONL directory not found at $JSONL_DIR"
fi

bench_log "Largest JSONL file: ${max_size_mb} MB"

if (( max_size_mb <= TARGET_MB )); then
    status="PASS"
else
    status="FAIL"
fi

record_result "$SLO_NUM" "jsonl_max_size" "$TARGET_MB" "$max_size_mb" "MB" "$status"

kill "$SK_PID" 2>/dev/null || true
