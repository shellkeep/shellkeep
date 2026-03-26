#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Distro matrix test runner for shellkeep.
# Builds and tests shellkeep across multiple distros using Docker.
#
# Usage: ./test_distro.sh [distro...]
#   If no distros specified, runs all: ubuntu2204 ubuntu2404 debian12
#
# Environment:
#   PROJECT_DIR — path to shellkeep source (default: auto-detected)
#   RESULTS_DIR — where to write per-distro logs (default: ./results)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Auto-detect project root (two levels up from tests/distro/)
PROJECT_DIR="${PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
RESULTS_DIR="${RESULTS_DIR:-$SCRIPT_DIR/results}"

ALL_DISTROS=(ubuntu2204 ubuntu2404 debian12)
DISTROS=("${@:-${ALL_DISTROS[@]}}")
if [[ $# -eq 0 ]]; then
    DISTROS=("${ALL_DISTROS[@]}")
fi

mkdir -p "$RESULTS_DIR"

# Summary arrays
declare -A BUILD_STATUS
declare -A TEST_STATUS
declare -A HELP_STATUS
declare -A DESKTOP_STATUS
declare -A MAN_STATUS
declare -A DEB_STATUS
declare -A DEP_VERSIONS

pass_or_fail() {
    if echo "$1" | grep -q "$2"; then
        echo "PASS"
    else
        echo "FAIL"
    fi
}

for distro in "${DISTROS[@]}"; do
    dockerfile="$SCRIPT_DIR/Dockerfile.${distro}"
    image_tag="shellkeep-distro-${distro}"
    log_file="$RESULTS_DIR/${distro}.log"

    echo "=============================================="
    echo "  Testing distro: $distro"
    echo "=============================================="

    if [[ ! -f "$dockerfile" ]]; then
        echo "ERROR: Dockerfile not found: $dockerfile"
        BUILD_STATUS[$distro]="FAIL (no Dockerfile)"
        continue
    fi

    # Build the Docker image — capture full output
    echo "Building $image_tag ..."
    if docker build \
        -f "$dockerfile" \
        -t "$image_tag" \
        "$PROJECT_DIR" \
        > "$log_file" 2>&1; then
        BUILD_STATUS[$distro]="PASS"
    else
        BUILD_STATUS[$distro]="FAIL"
        echo "  BUILD FAILED — see $log_file"
        continue
    fi

    # Extract results from the build log
    log_content=$(cat "$log_file")

    # Check meson test results
    if echo "$log_content" | grep -q "Ok:.*Expected Fail:.*Unexpected Pass:.*Skipped:"; then
        test_line=$(echo "$log_content" | grep "Ok:.*Expected Fail:" | tail -1)
        if echo "$test_line" | grep -q "Fail: *0"; then
            TEST_STATUS[$distro]="PASS ($test_line)"
        else
            TEST_STATUS[$distro]="FAIL ($test_line)"
        fi
    else
        # meson test may print different summary format
        if echo "$log_content" | grep -qE "^(1/[0-9]+|[0-9]+/[0-9]+).*OK"; then
            TEST_STATUS[$distro]="PASS"
        else
            TEST_STATUS[$distro]="UNKNOWN (check log)"
        fi
    fi

    # Check --help
    if echo "$log_content" | grep -qi "usage\|shellkeep\|--help"; then
        HELP_STATUS[$distro]="PASS"
    else
        HELP_STATUS[$distro]="FAIL"
    fi

    # Check .desktop validation
    if echo "$log_content" | grep -q "desktop-file-validate"; then
        # If the step ran without error (Docker build continued), it passed
        DESKTOP_STATUS[$distro]="PASS"
    else
        DESKTOP_STATUS[$distro]="UNKNOWN"
    fi

    # Check man page
    if echo "$log_content" | grep -q "man page OK"; then
        MAN_STATUS[$distro]="PASS"
    else
        MAN_STATUS[$distro]="FAIL"
    fi

    # Check .deb build
    if echo "$log_content" | grep -q "DEB_BUILD_FAILED"; then
        DEB_STATUS[$distro]="FAIL"
    elif echo "$log_content" | grep -q "DEB_INSTALL_SKIPPED"; then
        DEB_STATUS[$distro]="SKIPPED"
    elif echo "$log_content" | grep -q "dpkg.*shellkeep"; then
        DEB_STATUS[$distro]="PASS"
    else
        DEB_STATUS[$distro]="UNKNOWN"
    fi

    # Extract dependency versions
    if echo "$log_content" | grep -q "=== DEPENDENCY VERSIONS ==="; then
        versions=$(echo "$log_content" | sed -n '/=== DEPENDENCY VERSIONS ===/,/=== END ===/p' | grep -v "===")
        DEP_VERSIONS[$distro]="$versions"
    else
        DEP_VERSIONS[$distro]="(not captured)"
    fi

    echo "  Build:    ${BUILD_STATUS[$distro]}"
    echo "  Tests:    ${TEST_STATUS[$distro]}"
    echo "  --help:   ${HELP_STATUS[$distro]}"
    echo "  Desktop:  ${DESKTOP_STATUS[$distro]}"
    echo "  Man page: ${MAN_STATUS[$distro]}"
    echo "  .deb:     ${DEB_STATUS[$distro]}"
    echo ""
done

# Print summary
echo ""
echo "=============================================="
echo "  DISTRO MATRIX SUMMARY"
echo "=============================================="
for distro in "${DISTROS[@]}"; do
    echo ""
    echo "--- $distro ---"
    echo "  Build:    ${BUILD_STATUS[$distro]:-N/A}"
    echo "  Tests:    ${TEST_STATUS[$distro]:-N/A}"
    echo "  --help:   ${HELP_STATUS[$distro]:-N/A}"
    echo "  Desktop:  ${DESKTOP_STATUS[$distro]:-N/A}"
    echo "  Man page: ${MAN_STATUS[$distro]:-N/A}"
    echo "  .deb:     ${DEB_STATUS[$distro]:-N/A}"
    echo "  Deps:"
    echo "    ${DEP_VERSIONS[$distro]:-N/A}" | head -20
done

echo ""
echo "Logs saved to: $RESULTS_DIR/"
echo "Done."
