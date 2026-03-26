#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Update all shellkeep artifacts: screenshots, docs, reports.
# Wrapper around capture-screenshots.sh.
#
# Usage: ./scripts/update-artifacts.sh [SSH_HOST] [SSH_KEY]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SSH_HOST="${1:-209.38.150.61}"
SSH_KEY="${2:-/home/node/.ssh/id_shellkeep}"

echo "╔══════════════════════════════════════╗"
echo "║   shellkeep artifact update          ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Step 1: Capture screenshots
echo "── Step 1: Capture screenshots ──"
bash "$SCRIPT_DIR/capture-screenshots.sh" "$SSH_HOST" "$SSH_KEY"

# Step 2: Verify all screenshots are valid PNGs
echo ""
echo "── Step 2: Verify screenshots ──"
VALID=0
INVALID=0
for f in "$PROJECT_ROOT/docs/screenshots"/*.png; do
  if [ -f "$f" ] && file "$f" | grep -q "PNG"; then
    VALID=$((VALID + 1))
  else
    INVALID=$((INVALID + 1))
    echo "  ✗ $(basename "$f") is not a valid PNG"
  fi
done
echo "  $VALID valid PNGs, $INVALID invalid"

# Step 3: Summary
echo ""
echo "── Summary ──"
echo "  Screenshots: $VALID/8 captured"
echo "  Location: docs/screenshots/"
echo ""
echo "Done. Review screenshots and commit if satisfied."
