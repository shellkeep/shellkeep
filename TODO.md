# shellkeep v1 — Completion TODO

## SSH
- [x] FR-STATE-05: SFTP posix-rename extension check + unlink+rename fallback

## Terminal / UI
- [x] FR-UI-04/05: Latency measurement via keepalive RTT + tab indicator
- [x] FR-UI-03: First-use client-id naming input field
- [x] FR-TABS-03: Tab reordering via context menu (Move Left/Right)
- [x] FR-TABS-11: Context menu copy/paste (clipboard integration)
- [x] FR-TRAY-04: Tray icon appearance change when windows hidden

## i18n
- [x] NFR-I18N-02: Positional placeholders via tf() function
- [x] NFR-I18N-03: Plural support via tn() function (ngettext equivalent)

## Observability
- [x] NFR-OBS-05: Async logging — tracing-subscriber uses dedicated writer thread

## Legacy Code
- [x] Remove all C/Qt6 source files (35k lines)
- [x] Remove legacy CI workflows (ci.yml, lint.yml, package.yml, release.yml, codeql.yml)
- [x] Update setup-dev.sh and build-appimage.sh for Rust

## Tests
- [x] 97 unit tests passing
- [x] 19 E2E tests passing on real droplet (3 russh + 7 tmux + 9 features)
- [x] CI green on Linux/macOS/Windows

## Packaging
- [ ] NFR-DIST-01/02: Validate AppImage + .deb build scripts work with Rust binary
