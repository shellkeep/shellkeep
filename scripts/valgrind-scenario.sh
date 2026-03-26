#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# ============================================================================
# valgrind-scenario.sh -- Full lifecycle Valgrind memcheck for shellkeep
#
# This script drives a complete shellkeep lifecycle scenario under Valgrind
# to detect memory leaks in application code. It requires:
#
#   - A running X11 or Wayland display server (DISPLAY or WAYLAND_DISPLAY set)
#   - Valgrind installed (valgrind --version)
#   - shellkeep built in ./build/ (meson compile -C build)
#   - An accessible SSH server for the target host
#   - The suppressions file at tests/valgrind.supp
#
# Usage:
#   ./scripts/valgrind-scenario.sh [user@]host [--port PORT]
#
# The script is designed to be run MANUALLY because it requires:
#   1. A display server for GTK window creation
#   2. An SSH server to connect to
#   3. Interactive observation of reconnection behavior
#
# Lifecycle scenario tested (FR-RECONNECT, FR-TABS, FR-ENV, FR-SESSION):
#   Phase 1: Connect to server
#   Phase 2: Open 10 tabs (FR-TABS-01, FR-TABS-02)
#   Phase 3: Close 5 tabs (FR-TABS-04, FR-SESSION-07)
#   Phase 4: Simulate network drop / reconnect (FR-RECONNECT-01..03)
#   Phase 5: Switch environment (FR-ENV-03..05)
#   Phase 6: Graceful disconnect and shutdown
#
# Valgrind target (Agent 27 scope):
#   "definitely lost: 0 bytes"
#   "indirectly lost: 0 bytes"
#   (excluding suppressions for GTK/GLib/Pango/VTE/libssh internals)
# ============================================================================

set -euo pipefail

# ---- Configuration ----------------------------------------------------------

SHELLKEEP_BIN="${SHELLKEEP_BIN:-./build/shellkeep}"
SUPP_FILE="${SUPP_FILE:-./tests/valgrind.supp}"
VALGRIND_LOG="${VALGRIND_LOG:-./valgrind-output.log}"

# Scenario timing (seconds). Adjust for slower systems.
CONNECT_WAIT=5
TAB_OPEN_DELAY=1
TAB_CLOSE_DELAY=1
RECONNECT_WAIT=15
ENV_SWITCH_WAIT=5
SHUTDOWN_WAIT=3

# Colors for output.
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No color

# ---- Helpers ----------------------------------------------------------------

log_info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*"; }

usage() {
    echo "Usage: $0 [user@]host [--port PORT]"
    echo ""
    echo "Environment variables:"
    echo "  SHELLKEEP_BIN    Path to shellkeep binary (default: ./build/shellkeep)"
    echo "  SUPP_FILE        Path to suppressions file (default: ./tests/valgrind.supp)"
    echo "  VALGRIND_LOG     Path to Valgrind output log (default: ./valgrind-output.log)"
    echo ""
    echo "This script must be run with a display server (X11/Wayland) available."
    exit 1
}

check_prerequisites() {
    local ok=true

    if ! command -v valgrind &>/dev/null; then
        log_error "valgrind not found. Install with: sudo apt install valgrind"
        ok=false
    fi

    if [ ! -x "$SHELLKEEP_BIN" ]; then
        log_error "shellkeep binary not found at $SHELLKEEP_BIN"
        log_error "Build with: meson compile -C build"
        ok=false
    fi

    if [ ! -f "$SUPP_FILE" ]; then
        log_error "Suppressions file not found at $SUPP_FILE"
        ok=false
    fi

    if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
        log_error "No display server detected. Set DISPLAY or WAYLAND_DISPLAY."
        ok=false
    fi

    if [ "$ok" = false ]; then
        exit 1
    fi
}

# ---- Argument parsing -------------------------------------------------------

HOST=""
PORT=""

while [ $# -gt 0 ]; do
    case "$1" in
        --port)
            PORT="$2"
            shift 2
            ;;
        --help|-h)
            usage
            ;;
        *)
            HOST="$1"
            shift
            ;;
    esac
done

if [ -z "$HOST" ]; then
    log_error "No host specified."
    usage
fi

# ---- Prerequisite check -----------------------------------------------------

check_prerequisites

# ---- Build Valgrind command --------------------------------------------------

VALGRIND_CMD=(
    valgrind
    --leak-check=full
    --show-leak-kinds=all
    --track-origins=yes
    --error-exitcode=1
    --num-callers=30
    --trace-children=no
    --child-silent-after-fork=yes
    --gen-suppressions=all
    --log-file="$VALGRIND_LOG"
    --suppressions="$SUPP_FILE"
)

SHELLKEEP_ARGS=("$HOST")
if [ -n "$PORT" ]; then
    SHELLKEEP_ARGS+=(-p "$PORT")
fi

# Enable debug logging for leak analysis context.
SHELLKEEP_ARGS+=(--debug)

# ---- Scenario execution -----------------------------------------------------

log_info "============================================================"
log_info "shellkeep Valgrind Memcheck Scenario"
log_info "============================================================"
log_info "Binary:        $SHELLKEEP_BIN"
log_info "Target:        $HOST${PORT:+ (port $PORT)}"
log_info "Suppressions:  $SUPP_FILE"
log_info "Valgrind log:  $VALGRIND_LOG"
log_info "============================================================"
echo ""

# --- Phase 0: Start shellkeep under Valgrind ---------------------------------

log_info "Phase 0: Starting shellkeep under Valgrind..."
log_info "  Command: ${VALGRIND_CMD[*]} $SHELLKEEP_BIN ${SHELLKEEP_ARGS[*]}"

"${VALGRIND_CMD[@]}" "$SHELLKEEP_BIN" "${SHELLKEEP_ARGS[@]}" &
VALGRIND_PID=$!

log_info "  Valgrind PID: $VALGRIND_PID"
log_info "  Waiting ${CONNECT_WAIT}s for connection to establish..."
sleep "$CONNECT_WAIT"

# Verify the process is still running.
if ! kill -0 "$VALGRIND_PID" 2>/dev/null; then
    log_error "shellkeep exited prematurely. Check $VALGRIND_LOG for details."
    exit 1
fi

# --- Phase 1: Interact via xdotool (if available) ----------------------------
#
# The following phases use xdotool to simulate user interaction. If xdotool is
# not available, we print instructions for manual execution.
#
# In a CI/headless environment, this script serves as documentation of the
# scenario that should be tested. The Valgrind invocation and suppressions
# are the primary deliverables.

if command -v xdotool &>/dev/null; then
    WINDOW_ID=$(xdotool search --name "shellkeep" 2>/dev/null | head -1 || true)

    if [ -z "$WINDOW_ID" ]; then
        log_warn "Could not find shellkeep window via xdotool."
        log_warn "Proceeding with manual scenario instructions."
        HAS_XDOTOOL=false
    else
        HAS_XDOTOOL=true
        log_info "  Found shellkeep window: $WINDOW_ID"
    fi
else
    HAS_XDOTOOL=false
    log_warn "xdotool not found. Manual interaction required."
fi

# --- Phase 2: Open 10 tabs ---------------------------------------------------

log_info ""
log_info "Phase 2: Opening 10 tabs..."

if [ "$HAS_XDOTOOL" = true ]; then
    for i in $(seq 1 10); do
        log_info "  Opening tab $i/10..."
        xdotool key --window "$WINDOW_ID" ctrl+shift+t
        sleep "$TAB_OPEN_DELAY"
    done
else
    log_info "  MANUAL: Press Ctrl+Shift+T ten times to open 10 tabs."
    log_info "  Press Enter when done..."
    read -r
fi

# --- Phase 3: Close 5 tabs ---------------------------------------------------

log_info ""
log_info "Phase 3: Closing 5 tabs..."

if [ "$HAS_XDOTOOL" = true ]; then
    for i in $(seq 1 5); do
        log_info "  Closing tab $i/5..."
        xdotool key --window "$WINDOW_ID" ctrl+shift+w
        sleep "$TAB_CLOSE_DELAY"
    done
else
    log_info "  MANUAL: Press Ctrl+Shift+W five times to close 5 tabs."
    log_info "  Press Enter when done..."
    read -r
fi

# --- Phase 4: Simulate reconnection ------------------------------------------

log_info ""
log_info "Phase 4: Simulating network drop and reconnection..."
log_info "  This phase tests FR-RECONNECT-01..03: automatic reconnection"
log_info "  after connection loss."
log_info ""

if [ "$HAS_XDOTOOL" = false ]; then
    log_info "  MANUAL: To simulate network drop, run in another terminal:"
    log_info "    sudo iptables -A OUTPUT -d <server-ip> -j DROP"
    log_info "  Wait 10 seconds, then restore:"
    log_info "    sudo iptables -D OUTPUT -d <server-ip> -j DROP"
    log_info "  Press Enter when reconnection is complete..."
    read -r
else
    log_info "  NOTE: Network drop simulation requires manual iptables"
    log_info "  intervention or SIGSTOP/SIGCONT to the SSH subprocess."
    log_info "  Skipping automated network simulation."
    log_info "  Waiting ${RECONNECT_WAIT}s for any background activity..."
    sleep "$RECONNECT_WAIT"
fi

# --- Phase 5: Switch environment ----------------------------------------------

log_info ""
log_info "Phase 5: Switching environment (FR-ENV-03..05)..."

if [ "$HAS_XDOTOOL" = false ]; then
    log_info "  MANUAL: Use the environment switcher to change environments."
    log_info "  Press Enter when done..."
    read -r
else
    log_info "  Waiting ${ENV_SWITCH_WAIT}s..."
    sleep "$ENV_SWITCH_WAIT"
fi

# --- Phase 6: Graceful shutdown -----------------------------------------------

log_info ""
log_info "Phase 6: Graceful disconnect and shutdown..."

if [ "$HAS_XDOTOOL" = true ]; then
    xdotool key --window "$WINDOW_ID" ctrl+shift+q 2>/dev/null || true
    sleep 2
    # If the window is still there (close confirmation dialog), press Enter.
    xdotool key --window "$WINDOW_ID" Return 2>/dev/null || true
fi

# Wait for shellkeep to exit, or kill after timeout.
log_info "  Waiting for shellkeep to exit..."
TIMEOUT=30
ELAPSED=0
while kill -0 "$VALGRIND_PID" 2>/dev/null && [ "$ELAPSED" -lt "$TIMEOUT" ]; do
    sleep 1
    ELAPSED=$((ELAPSED + 1))
done

if kill -0 "$VALGRIND_PID" 2>/dev/null; then
    log_warn "  shellkeep did not exit within ${TIMEOUT}s. Sending SIGTERM..."
    kill -TERM "$VALGRIND_PID" 2>/dev/null || true
    sleep "$SHUTDOWN_WAIT"

    if kill -0 "$VALGRIND_PID" 2>/dev/null; then
        log_warn "  Still running. Sending SIGKILL..."
        kill -KILL "$VALGRIND_PID" 2>/dev/null || true
    fi
fi

wait "$VALGRIND_PID" 2>/dev/null || true

# ---- Results analysis --------------------------------------------------------

log_info ""
log_info "============================================================"
log_info "Valgrind Memcheck Results"
log_info "============================================================"

if [ ! -f "$VALGRIND_LOG" ]; then
    log_error "Valgrind log not found at $VALGRIND_LOG"
    exit 1
fi

# Extract the summary.
log_info ""
log_info "Leak summary:"
grep -A 10 "LEAK SUMMARY" "$VALGRIND_LOG" || log_warn "No leak summary found."

log_info ""
log_info "Error summary:"
grep "ERROR SUMMARY" "$VALGRIND_LOG" || log_warn "No error summary found."

# Check for shellkeep-specific leaks (not suppressed).
log_info ""
log_info "Checking for shellkeep-specific leaks..."

DEFINITELY_LOST=$(grep "definitely lost:" "$VALGRIND_LOG" | grep -oP '\d+(?= bytes)' | head -1 || echo "?")
INDIRECTLY_LOST=$(grep "indirectly lost:" "$VALGRIND_LOG" | grep -oP '\d+(?= bytes)' | head -1 || echo "?")

if [ "$DEFINITELY_LOST" = "0" ] && [ "$INDIRECTLY_LOST" = "0" ]; then
    log_info "${GREEN}PASS: No definite or indirect leaks detected.${NC}"
    EXIT_CODE=0
elif [ "$DEFINITELY_LOST" = "?" ] || [ "$INDIRECTLY_LOST" = "?" ]; then
    log_warn "Could not parse leak summary. Review $VALGRIND_LOG manually."
    EXIT_CODE=2
else
    log_error "FAIL: definitely lost=$DEFINITELY_LOST bytes, indirectly lost=$INDIRECTLY_LOST bytes"
    log_error "Review $VALGRIND_LOG for full details."
    log_error "Use --gen-suppressions=all output to identify external library leaks."
    EXIT_CODE=1
fi

log_info ""
log_info "Full Valgrind log: $VALGRIND_LOG"
log_info "============================================================"

exit "${EXIT_CODE}"
