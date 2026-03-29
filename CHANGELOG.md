<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-03-29

### Changed

- **Complete rewrite in Rust** with iced + alacritty_terminal
- Terminal emulation: alacritty_terminal (same engine as Zed editor)
- GPU-accelerated rendering via wgpu (Vulkan/Metal/DX12)
- SSH: system ssh binary for terminal + russh for control operations
- Config format: TOML (was INI)
- Binary size: ~19MB (was ~50MB with Qt6)

### Added

- Welcome screen with host/user/port form and recent connections
- Multi-tab with tab bar, close buttons, status indicators (green/yellow/red)
- Tab rename via F2 or right-click context menu
- Tab reorder via right-click Move left/right
- Auto-reconnection on SSH disconnect with retry counter
- tmux session persistence (sessions survive disconnects)
- Auto-detect and restore existing tmux sessions on connect
- State persistence: tab layout saved to JSON, restored on reconnect
- Right-click context menu in terminal (Copy/Paste)
- Right-click context menu on tabs (Move/Rename/Close)
- Zoom controls: Ctrl+=/- and Ctrl+0 reset
- Shift+PageUp/PageDown for scrollback navigation
- Status bar with tab count and zoom indicator
- Keyboard shortcuts hint in welcome screen
- Recent connections persisted to JSON (last 20)
- TOML config file (~/.config/shellkeep/config.toml)
- Crash handler with backtrace dump
- Core dump prevention (prctl PR_SET_DUMPABLE)
- File permission enforcement (0700 dirs, 0600 files)
- russh integration for programmatic SSH (exec, SFTP ready)
- Release workflow (GitHub Actions → Linux/macOS/Windows binaries)
- 23 tests (13 unit + 7 tmux E2E + 3 russh E2E)
- CLI: --version, --help, --debug, -p, -i

### Removed

- Qt6, CMake, C/C++ codebase
- QTermWidget dependency
- GLib, json-glib, libssh dependencies

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

[0.3.0]: https://github.com/shellkeep/shellkeep/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/shellkeep/shellkeep/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/shellkeep/shellkeep/releases/tag/v0.1.0
