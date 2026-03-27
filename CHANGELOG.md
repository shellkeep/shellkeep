<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-03-26

### Changed

- **UI framework migrated from GTK3+VTE to Qt6** for cross-platform support
- Build system migrated from Meson to CMake
- System tray uses QSystemTrayIcon (replaces libayatana-appindicator3)
- Terminal rendering via QTermWidget (replaces VTE)
- Modern dark theme (Catppuccin Mocha inspired) replaces GTK system theme
- CI now tests on Linux, macOS, and Windows

### Added

- macOS support (.app bundle, .dmg distribution)
- Windows support (installer, portable zip)
- Cross-platform single-instance via QLocalServer
- Platform-agnostic UI bridge (sk_ui_bridge.h) decouples backend from toolkit
- Thread-safe dialog dispatch via Qt signals/slots
- Connection feedback overlay with phase progress
- Toast notification system with fade animations
- Welcome screen with recent connections

### Removed

- GTK3 dependency
- VTE dependency
- libayatana-appindicator3 dependency
- Meson build files (CMake replaces them)

### Fixed

- Platform-specific code guarded with ifdefs (prctl, execinfo, Unix signals)

## [0.1.0] - 2026-03-15

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

[0.2.0]: https://github.com/shellkeep/shellkeep/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/shellkeep/shellkeep/releases/tag/v0.1.0
