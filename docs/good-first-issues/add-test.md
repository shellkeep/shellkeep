<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Good First Issue: Add a Unit Test

**Difficulty:** Easy-Medium
**Skills:** C, cmocka
**Files:** `tests/unit/`

## Description

Increasing test coverage helps catch regressions early. Many utility
functions and state-management routines could use additional test cases.

## Steps

1. Run coverage to find under-tested files:
   ```bash
   meson setup build-cov --buildtype=debug -Db_coverage=true -Dtests=true
   meson test -C build-cov
   ninja -C build-cov coverage-html
   ```
   Open `build-cov/meson-logs/coveragereport/index.html` and look for files
   with low line coverage.
2. Pick a function that is not well covered and write test cases using the
   cmocka framework. See existing tests in `tests/unit/` for examples.
3. Add your test file to `tests/unit/meson.build`.
4. Run the full test suite to make sure nothing breaks:
   ```bash
   meson test -C build --print-errorlogs
   ```
5. Submit a PR titled "tests: add unit tests for <function/module>".

## Acceptance Criteria

- At least 3 test cases per function (normal input, edge case, error case).
- Tests pass reliably (no flaky behavior).
- Tests are registered in `meson.build` so CI picks them up.
