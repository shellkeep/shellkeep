#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 3: RAM with 20 tabs (scrollback 10k lines) < 300 MB RSS
#
# Method: Launch shellkeep, open 20 tabs, fill each with 10k lines of output,
# then measure RSS via ps.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=3
TARGET_MB=300
NUM_TABS=20
SCROLLBACK_LINES=10000

bench_log "Launching shellkeep..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 3

# ---------------------------------------------------------------------------
# Open 20 tabs and fill scrollback
# ---------------------------------------------------------------------------
bench_log "Opening $NUM_TABS tabs and filling each with $SCROLLBACK_LINES lines..."

for i in $(seq 1 "$NUM_TABS"); do
    # Use shellkeep's D-Bus or CLI interface to open a new tab
    # Fallback: use keyboard shortcut via xdotool
    xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
    sleep 0.5
done

# Fill each tab's scrollback with output
for sess in $(tmux list-sessions -F '#{session_name}' 2>/dev/null | head -"$NUM_TABS"); do
    tmux send-keys -t "$sess" "seq 1 $SCROLLBACK_LINES" Enter &
done

# Wait for output to be processed
sleep 10

# ---------------------------------------------------------------------------
# Measure RSS
# ---------------------------------------------------------------------------
rss_mb=$(get_rss_mb "$SK_PID")
bench_log "RSS with $NUM_TABS tabs: ${rss_mb} MB"

if (( rss_mb <= TARGET_MB )); then
    status="PASS"
else
    status="FAIL"
fi

record_result "$SLO_NUM" "ram_20_tabs_10k_scrollback" "$TARGET_MB" "$rss_mb" "MB" "$status"

kill "$SK_PID" 2>/dev/null || true
