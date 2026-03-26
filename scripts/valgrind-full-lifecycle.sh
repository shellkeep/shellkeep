#!/bin/bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Full Valgrind lifecycle test script for shellkeep.
#
# This script runs on the build server (droplet) and performs:
# 1. Build without sanitizers (Valgrind conflicts with ASan/UBSan)
# 2. Start Xvfb + D-Bus for GUI testing
# 3. Full GUI lifecycle under Valgrind (startup, window, shutdown)
# 4. Integration tests under Valgrind (real SSH connections)
# 5. Unit + edge + upgrade tests under Valgrind (for completeness)
#
# Prerequisites:
#   - valgrind, xvfb, xdotool, dbus-x11, at-spi2-core, sshpass installed
#   - Docker SSH container running on port 2223
#   - Project source at /opt/shellkeep-valgrind-full/
#
# Usage:
#   cd /opt/shellkeep-valgrind-full
#   bash scripts/valgrind-full-lifecycle.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$PROJECT_DIR/build-vg"
LOG_DIR="/tmp/valgrind-full-logs"
SUPP_FILE="$PROJECT_DIR/tests/valgrind.supp"

# SSH test server settings
export SK_TEST_SSH_HOST="${SK_TEST_SSH_HOST:-127.0.0.1}"
export SK_TEST_SSH_PORT="${SK_TEST_SSH_PORT:-2223}"
export SK_TEST_SSH_USER="${SK_TEST_SSH_USER:-testuser}"
export SK_TEST_SSH_PASSWORD="${SK_TEST_SSH_PASSWORD:-testpass}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

passed=0
failed=0
skipped=0

mkdir -p "$LOG_DIR"

log_pass() { echo -e "${GREEN}PASS${NC}: $1"; ((passed++)); }
log_fail() { echo -e "${RED}FAIL${NC}: $1"; ((failed++)); }
log_skip() { echo -e "${YELLOW}SKIP${NC}: $1"; ((skipped++)); }
log_info() { echo -e "INFO: $1"; }

# ---- Step 1: Build ----
log_info "Building shellkeep without sanitizers..."
cd "$PROJECT_DIR"

if [ -d "$BUILD_DIR" ]; then
    rm -rf "$BUILD_DIR"
fi

meson setup "$BUILD_DIR" -Dtests=true -Db_sanitize=none -Dbuildtype=release 2>&1
meson compile -C "$BUILD_DIR" 2>&1

log_info "Build complete."

# ---- Step 2: Start Xvfb + D-Bus ----
log_info "Starting Xvfb and D-Bus..."

# Kill any existing Xvfb on :98
pkill -f "Xvfb :98" 2>/dev/null || true
sleep 1

Xvfb :98 -screen 0 1280x800x24 &
XVFB_PID=$!
export DISPLAY=:98

eval $(dbus-launch --sh-syntax)
sleep 2

log_info "Xvfb PID=$XVFB_PID, DISPLAY=$DISPLAY"

# ---- Step 3: Verify SSH test server ----
log_info "Checking SSH test server on $SK_TEST_SSH_HOST:$SK_TEST_SSH_PORT..."
if sshpass -p "$SK_TEST_SSH_PASSWORD" ssh -o StrictHostKeyChecking=no -p "$SK_TEST_SSH_PORT" \
    "$SK_TEST_SSH_USER@$SK_TEST_SSH_HOST" "echo SSH_OK" 2>/dev/null; then
    log_info "SSH test server is reachable."
    SSH_AVAILABLE=true
else
    log_info "SSH test server NOT reachable. Integration tests will be skipped."
    SSH_AVAILABLE=false
fi

# ---- Step 4: Full GUI lifecycle under Valgrind ----
log_info "=== Full GUI Lifecycle Test ==="

LIFECYCLE_LOG="$LOG_DIR/valgrind-lifecycle.log"

# Run shellkeep under Valgrind with a timeout
valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes \
    --error-exitcode=1 --suppressions="$SUPP_FILE" \
    --log-file="$LIFECYCLE_LOG" \
    timeout 30 "$BUILD_DIR/shellkeep" testuser@localhost -p "$SK_TEST_SSH_PORT" &
SHELLKEEP_PID=$!

# Wait for window to appear
sleep 8

# Try to interact with the window
WINDOW_ID=$(xdotool search --name shellkeep 2>/dev/null | head -1) || true
if [ -n "$WINDOW_ID" ]; then
    log_info "Found shellkeep window: $WINDOW_ID"

    # Type password if auth dialog appears
    xdotool key --delay 100 t e s t p a s s Return 2>/dev/null || true
    sleep 3

    # Try to gracefully close
    xdotool key --delay 100 alt+F4 2>/dev/null || true
    sleep 2
else
    log_info "No shellkeep window found (may have exited or failed to create)"
fi

# Wait for process or kill
kill "$SHELLKEEP_PID" 2>/dev/null || true
wait "$SHELLKEEP_PID" 2>/dev/null || true

# Analyze lifecycle log
if [ -f "$LIFECYCLE_LOG" ]; then
    DEFINITELY_LOST=$(grep -oP 'definitely lost: \K[\d,]+' "$LIFECYCLE_LOG" | tr -d ',' | tail -1)
    INDIRECTLY_LOST=$(grep -oP 'indirectly lost: \K[\d,]+' "$LIFECYCLE_LOG" | tr -d ',' | tail -1)
    ERROR_SUMMARY=$(grep -oP 'ERROR SUMMARY: \K[\d,]+' "$LIFECYCLE_LOG" | tr -d ',' | tail -1)

    log_info "Lifecycle: definitely lost=$DEFINITELY_LOST, indirectly lost=$INDIRECTLY_LOST, errors=$ERROR_SUMMARY"

    if [ "${DEFINITELY_LOST:-0}" = "0" ] && [ "${INDIRECTLY_LOST:-0}" = "0" ]; then
        log_pass "Full lifecycle: 0 definite/indirect leaks"
    else
        log_fail "Full lifecycle: definitely=$DEFINITELY_LOST, indirectly=$INDIRECTLY_LOST"
    fi
else
    log_skip "Full lifecycle: no Valgrind log produced"
fi

# ---- Step 5: Integration tests under Valgrind ----
log_info "=== Integration Tests Under Valgrind ==="

if [ "$SSH_AVAILABLE" = true ]; then
    for test in "$BUILD_DIR"/test_integration_*; do
        if [ -x "$test" ]; then
            testname=$(basename "$test")
            test_log="$LOG_DIR/valgrind-${testname}.log"

            log_info "Running $testname under Valgrind..."
            valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes \
                --error-exitcode=1 --suppressions="$SUPP_FILE" \
                --log-file="$test_log" \
                "$test" 2>&1 || true

            if [ -f "$test_log" ]; then
                DL=$(grep -oP 'definitely lost: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)
                IL=$(grep -oP 'indirectly lost: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)
                ES=$(grep -oP 'ERROR SUMMARY: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)

                if [ "${DL:-0}" = "0" ] && [ "${IL:-0}" = "0" ]; then
                    log_pass "$testname: 0 definite/indirect leaks (errors=$ES)"
                else
                    log_fail "$testname: definitely=$DL, indirectly=$IL, errors=$ES"
                fi
            else
                log_skip "$testname: no Valgrind log"
            fi
        fi
    done
else
    log_skip "Integration tests: SSH server not available"
fi

# ---- Step 6: Unit tests under Valgrind ----
log_info "=== Unit Tests Under Valgrind ==="

for test in test_state test_config test_ssh test_session test_log test_history test_reconnect; do
    test_path="$BUILD_DIR/$test"
    if [ -x "$test_path" ]; then
        test_log="$LOG_DIR/valgrind-${test}.log"

        log_info "Running $test under Valgrind..."
        valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes \
            --error-exitcode=1 --suppressions="$SUPP_FILE" \
            --log-file="$test_log" \
            "$test_path" 2>&1 || true

        if [ -f "$test_log" ]; then
            DL=$(grep -oP 'definitely lost: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)
            IL=$(grep -oP 'indirectly lost: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)
            ES=$(grep -oP 'ERROR SUMMARY: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)

            if [ "${DL:-0}" = "0" ] && [ "${IL:-0}" = "0" ]; then
                log_pass "$test: 0 definite/indirect leaks (errors=$ES)"
            else
                log_fail "$test: definitely=$DL, indirectly=$IL, errors=$ES"
            fi
        fi
    fi
done

# ---- Step 7: Edge tests under Valgrind ----
log_info "=== Edge Tests Under Valgrind ==="

for test in test_edge_state test_edge_limits test_edge_tmux test_edge_ssh test_edge_signals; do
    test_path="$BUILD_DIR/$test"
    if [ -x "$test_path" ]; then
        test_log="$LOG_DIR/valgrind-${test}.log"

        log_info "Running $test under Valgrind..."
        valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes \
            --error-exitcode=1 --suppressions="$SUPP_FILE" \
            --log-file="$test_log" \
            "$test_path" 2>&1 || true

        if [ -f "$test_log" ]; then
            DL=$(grep -oP 'definitely lost: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)
            IL=$(grep -oP 'indirectly lost: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)
            ES=$(grep -oP 'ERROR SUMMARY: \K[\d,]+' "$test_log" | tr -d ',' | tail -1)

            if [ "${DL:-0}" = "0" ] && [ "${IL:-0}" = "0" ]; then
                log_pass "$test: 0 definite/indirect leaks (errors=$ES)"
            else
                log_fail "$test: definitely=$DL, indirectly=$IL, errors=$ES"
            fi
        fi
    fi
done

# ---- Cleanup ----
kill "$XVFB_PID" 2>/dev/null || true
kill "$DBUS_SESSION_BUS_PID" 2>/dev/null || true

# ---- Summary ----
echo ""
echo "==================================================================="
echo "  Valgrind Full Lifecycle Test Summary"
echo "==================================================================="
echo -e "  ${GREEN}PASSED${NC}: $passed"
echo -e "  ${RED}FAILED${NC}: $failed"
echo -e "  ${YELLOW}SKIPPED${NC}: $skipped"
echo "  Logs: $LOG_DIR/"
echo "==================================================================="

if [ "$failed" -gt 0 ]; then
    echo ""
    echo "FAILED test logs:"
    for log in "$LOG_DIR"/*.log; do
        DL=$(grep -oP 'definitely lost: \K[\d,]+' "$log" | tr -d ',' | tail -1)
        IL=$(grep -oP 'indirectly lost: \K[\d,]+' "$log" | tr -d ',' | tail -1)
        if [ "${DL:-0}" != "0" ] || [ "${IL:-0}" != "0" ]; then
            echo "  $(basename "$log"): definitely=$DL, indirectly=$IL"
            echo "  Stack traces:"
            grep -A 20 "definitely lost" "$log" | head -40
            echo ""
        fi
    done
    exit 1
fi

exit 0
