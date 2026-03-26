#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# SLO 2: CPU overhead parsing tmux -CC < 5% idle, < 15% under load
#
# Method: Launch shellkeep with tmux -CC sessions, measure CPU with top/ps
# in idle state and under synthetic load (rapid output).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/bench_common.sh"

ensure_shellkeep
ensure_display
mkdir_results

SLO_NUM=2
TARGET_IDLE=5     # percent
TARGET_LOAD=15    # percent
SAMPLE_DURATION=10  # seconds

# ---------------------------------------------------------------------------
# Idle measurement
# ---------------------------------------------------------------------------
bench_log "Launching shellkeep for idle CPU measurement..."
$SHELLKEEP_BIN &
SK_PID=$!
sleep 5  # Let it settle

bench_log "Sampling CPU for ${SAMPLE_DURATION}s in idle state..."
cpu_idle=$(sample_cpu_percent "$SK_PID" "$SAMPLE_DURATION")
bench_log "Idle CPU: ${cpu_idle}%"

if awk "BEGIN { exit !($cpu_idle <= $TARGET_IDLE) }"; then
    status_idle="PASS"
else
    status_idle="FAIL"
fi
record_result "$SLO_NUM" "cpu_tmux_cc_idle" "$TARGET_IDLE" "$cpu_idle" "%" "$status_idle"

# ---------------------------------------------------------------------------
# Load measurement: generate rapid output in all sessions
# ---------------------------------------------------------------------------
bench_log "Generating load: rapid output in tmux sessions..."
# Send a command that generates continuous output
for sess in $(tmux list-sessions -F '#{session_name}' 2>/dev/null | grep -i shellkeep); do
    tmux send-keys -t "$sess" "yes | head -10000" Enter &
done
sleep 2  # Let output start flowing

bench_log "Sampling CPU for ${SAMPLE_DURATION}s under load..."
cpu_load=$(sample_cpu_percent "$SK_PID" "$SAMPLE_DURATION")
bench_log "Load CPU: ${cpu_load}%"

if awk "BEGIN { exit !($cpu_load <= $TARGET_LOAD) }"; then
    status_load="PASS"
else
    status_load="FAIL"
fi
record_result "$SLO_NUM" "cpu_tmux_cc_load" "$TARGET_LOAD" "$cpu_load" "%" "$status_load"

kill "$SK_PID" 2>/dev/null || true
