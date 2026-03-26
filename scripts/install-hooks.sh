#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# install-hooks.sh — Point git to the project's .githooks/ directory.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }

cd "${REPO_ROOT}"

# Use core.hooksPath so we don't need symlinks.
git config core.hooksPath .githooks
chmod +x .githooks/*

ok "Git hooks installed (core.hooksPath = .githooks)."
info "Pre-commit hook will check clang-format and SPDX headers on staged .c/.h files."
