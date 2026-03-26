<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Good First Issue: Improve an Error Message

**Difficulty:** Easy
**Skills:** C, empathy for users
**Files:** `src/` (various)

## Description

Good error messages tell the user what happened, why it happened, and what
they can do about it. Some shellkeep error messages may be too technical or
too vague. Improving them is a valuable contribution.

## Steps

1. Run shellkeep in various failure scenarios (wrong host, refused connection,
   missing key file, invalid config). Note any error messages that are
   confusing.
2. Find the message in the source code (search for the string in `src/`).
3. Rewrite it following these guidelines:
   - State what went wrong in plain language.
   - If possible, suggest a fix ("Check that the host is reachable" instead
     of "Connection failed").
   - Keep it under two sentences.
   - Use gettext `_()` for all user-visible strings.
4. Submit a PR titled "error: improve message for <scenario>".

## Acceptance Criteria

- The new message is clearer than the old one.
- It is wrapped in `_()` for translation.
- No sensitive information (keys, passwords) is included in the message.
- The error condition is unchanged (same code path, same severity).
