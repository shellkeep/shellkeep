<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Good First Issue: Add a Translation

**Difficulty:** Easy
**Skills:** Gettext, any non-English language
**Files:** `po/`

## Description

shellkeep uses GNU gettext for internationalization. Adding or improving a
translation helps make the application accessible to more users.

## Steps

1. Check the `po/` directory for existing `.po` files. If your language is
   not yet present, create a new one:
   ```bash
   cd po/
   msginit -l <lang_code> -o <lang_code>.po -i shellkeep.pot
   ```
2. Open the `.po` file in a text editor or a PO editor (e.g., Poedit,
   Lokalize, or the GNOME Translation Editor).
3. Translate the `msgstr` entries. Focus on the most user-visible strings
   first (menus, dialogs, error messages).
4. Test your translation:
   ```bash
   cmake --build build
   LANGUAGE=<lang_code> ./build/shellkeep user@host
   ```
5. Submit a PR titled "i18n: add <Language> translation" or "i18n: update
   <Language> translation".

## Acceptance Criteria

- The `.po` file compiles without errors (`msgfmt --check`).
- At least the main UI strings (menu items, dialog titles, status bar) are
  translated.
- No English-only formatting assumptions (e.g., date order) are introduced.
