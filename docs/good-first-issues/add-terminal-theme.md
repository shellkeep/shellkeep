<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Good First Issue: Add a Terminal Theme

**Difficulty:** Easy
**Skills:** C, VTE, color values
**Files:** `src/terminal/sk_terminal_themes.c`, `data/themes/`

## Description

shellkeep ships with a small set of built-in terminal color themes. Adding a
new theme is a great way to get familiar with the codebase without touching
any complex logic.

## Steps

1. Pick a popular terminal color scheme that is not yet included (e.g.,
   Nord, Gruvbox, Catppuccin variant, Tokyo Night).
2. Open `src/terminal/sk_terminal_themes.c` and look at how existing themes
   are defined. Each theme is a `SkTerminalTheme` struct with 16 ANSI colors
   plus foreground, background, and cursor colors.
3. Add a new entry to the `sk_builtin_themes` array.
4. Test it by running shellkeep and selecting your theme in the preferences
   dialog.
5. Submit a PR with the theme name in the title (e.g., "terminal: add Nord
   theme").

## Acceptance Criteria

- The theme renders correctly in VTE (all 16 colors visible).
- No existing themes are modified.
- The theme name and attribution are documented in the struct comment.
