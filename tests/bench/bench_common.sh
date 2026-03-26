#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# bench_common.sh — Shared benchmark framework for shellkeep SLO measurements.
# Sources: INITIAL-PLAN-V2.md, SLOs de Performance table.
#
# Usage: source this file from individual benchmark scripts.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

SHELLKEEP_BIN="${SHELLKEEP_BIN:-shellkeep}"
BENCH_RESULTS_DIR="${BENCH_RESULTS_DIR:-/tmp/shellkeep-bench-results}"
BENCH_ITERATIONS="${BENCH_ITERATIONS:-10}"
BENCH_WARMUP="${BENCH_WARMUP:-2}"
NETEM_IFACE="${NETEM_IFACE:-lo}"
NETEM_DELAY_MS="${NETEM_DELAY_MS:-25}"  # one-way; RTT = 2x = 50ms

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

bench_log() {
    echo "[bench] $(date +%Y-%m-%dT%H:%M:%S%z) $*"
}

bench_fail() {
    echo "[bench] FATAL: $*" >&2
    exit 1
}

ensure_root() {
    if [[ $EUID -ne 0 ]]; then
        bench_fail "This benchmark requires root (for tc netem). Re-run with sudo."
    fi
}

ensure_shellkeep() {
    if ! command -v "$SHELLKEEP_BIN" &>/dev/null; then
        bench_fail "$SHELLKEEP_BIN not found in PATH. Build first or set SHELLKEEP_BIN."
    fi
}

ensure_display() {
    if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
        bench_fail "No display server detected. These benchmarks require a running X11/Wayland session."
    fi
}

mkdir_results() {
    mkdir -p "$BENCH_RESULTS_DIR"
}

# ---------------------------------------------------------------------------
# Timing helpers
# ---------------------------------------------------------------------------

# Returns current time in nanoseconds (Linux).
now_ns() {
    date +%s%N
}

# Returns current time in milliseconds.
now_ms() {
    local ns
    ns=$(now_ns)
    echo $(( ns / 1000000 ))
}

# Compute elapsed milliseconds between two nanosecond timestamps.
elapsed_ms() {
    local start_ns="$1" end_ns="$2"
    echo $(( (end_ns - start_ns) / 1000000 ))
}

# ---------------------------------------------------------------------------
# Statistics helpers
# ---------------------------------------------------------------------------

# Read numbers from stdin (one per line), print p95 value.
percentile_95() {
    sort -n | awk '
    {
        vals[NR] = $1
        n = NR
    }
    END {
        idx = int(n * 0.95 + 0.5)
        if (idx < 1) idx = 1
        if (idx > n) idx = n
        print vals[idx]
    }'
}

# Read numbers from stdin, print mean.
mean_value() {
    awk '{ sum += $1; n++ } END { if (n > 0) printf "%.2f\n", sum / n; else print 0 }'
}

# Read numbers from stdin, print max.
max_value() {
    sort -n | tail -1
}

# ---------------------------------------------------------------------------
# Network emulation (tc netem)
# ---------------------------------------------------------------------------

netem_add_delay() {
    local iface="$1" delay_ms="$2"
    bench_log "Adding ${delay_ms}ms delay on $iface (RTT = $(( delay_ms * 2 ))ms)"
    tc qdisc add dev "$iface" root netem delay "${delay_ms}ms" 2>/dev/null \
        || tc qdisc change dev "$iface" root netem delay "${delay_ms}ms"
}

netem_remove() {
    local iface="$1"
    bench_log "Removing netem rules on $iface"
    tc qdisc del dev "$iface" root 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# Process measurement
# ---------------------------------------------------------------------------

# Get RSS in KB for a PID.
get_rss_kb() {
    local pid="$1"
    ps -o rss= -p "$pid" 2>/dev/null | tr -d ' '
}

# Get RSS in MB for a PID.
get_rss_mb() {
    local pid="$1"
    local rss_kb
    rss_kb=$(get_rss_kb "$pid")
    if [[ -n "$rss_kb" ]]; then
        echo $(( rss_kb / 1024 ))
    else
        echo 0
    fi
}

# Sample CPU usage of a process over a duration (seconds). Returns percentage.
sample_cpu_percent() {
    local pid="$1" duration="${2:-5}"
    top -b -d "$duration" -n 2 -p "$pid" 2>/dev/null \
        | awk -v pid="$pid" '$1 == pid { cpu = $9 } END { print cpu+0 }'
}

# ---------------------------------------------------------------------------
# Result recording
# ---------------------------------------------------------------------------

# Write a result line to the results CSV.
# Usage: record_result <slo_number> <metric_name> <target> <measured> <unit> <pass_fail>
record_result() {
    local slo="$1" metric="$2" target="$3" measured="$4" unit="$5" status="$6"
    local csv="$BENCH_RESULTS_DIR/results.csv"
    if [[ ! -f "$csv" ]]; then
        echo "slo,metric,target,measured,unit,status" > "$csv"
    fi
    echo "$slo,$metric,$target,$measured,$unit,$status" >> "$csv"
    bench_log "SLO $slo: $metric = $measured $unit (target: $target $unit) => $status"
}

# ---------------------------------------------------------------------------
# Cleanup trap
# ---------------------------------------------------------------------------

_bench_cleanup_fns=()

bench_on_exit() {
    _bench_cleanup_fns+=("$1")
}

_bench_run_cleanup() {
    for fn in "${_bench_cleanup_fns[@]:-}"; do
        [[ -n "$fn" ]] && "$fn" || true
    done
}

trap _bench_run_cleanup EXIT
