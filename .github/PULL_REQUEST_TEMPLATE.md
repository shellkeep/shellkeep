<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

## Summary

<!-- Brief description of what this PR does and why. -->

## Related Issues

<!-- Link related issues: Fixes #123, Closes #456, Related to #789 -->

## Changes

<!-- Bullet list of notable changes. -->

-

## Testing

<!-- How did you verify these changes? -->

- [ ] Unit tests pass (`ctest --test-dir build --label-regex unit --output-on-failure`)
- [ ] Integration tests pass (`ctest --test-dir build --label-regex integration --output-on-failure`)
- [ ] Manual testing performed (describe below)

## Checklist

- [ ] Code follows the project style (`snake_case`, `sk_` prefix, GNOME clang-format)
- [ ] SPDX headers present in all new `.c` and `.h` files
- [ ] Requirement IDs referenced in code comments where applicable
- [ ] User-visible strings use `_()` / `ngettext()` for i18n
- [ ] No passwords, keys, or terminal content in logs
- [ ] No blocking operations on the UI main thread
- [ ] Error handling uses `GError **` or return status codes
- [ ] New files have `0600`/`0700` permissions where required
- [ ] Documentation updated if behavior changed
- [ ] CLA signed (external contributors)

## Screenshots

<!-- If applicable, add screenshots or terminal output. -->
