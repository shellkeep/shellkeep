<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial project structure with layered architecture (SSH, Session, Terminal, State, UI)
- SSH connection management with libssh >= 0.10.0
- tmux session backend with control mode orchestration
- Per-device layout persistence via client-id
- Environment-based session grouping
- VTE terminal rendering with True Color, Unicode, and mouse support
- Automatic reconnection with exponential backoff and jitter
- Dead session recovery with scrollback history
- System tray integration via libayatana-appindicator3
- INI configuration with hot reload
- Structured logging with rotation and async writes
- Crash handling with backtrace capture
- JSONL session history recording
- Host key verification (TOFU model)
- SSH config (~/.ssh/config) compatibility
- Keyboard shortcut system with Ctrl+Shift prefix
- Gettext i18n infrastructure (English + pt_BR)
- AppImage and .deb packaging
- CI/CD with GitHub Actions
- CodeQL security analysis
- Man page (shellkeep.1)

[Unreleased]: https://github.com/shellkeep/shellkeep/commits/main
