# shellkeep — Refactoring TODO

Work through each item in order. Check off when done and committed.

## Quick wins (< 15 min each)

- [x] P-011: Add Message::Noop instead of repurposing unrelated messages
- [x] P-019: Fix color_pallete typo -> color_palette
- [x] P-017: Replace chrono_now() with chrono::Utc::now().to_rfc3339()
- [x] P-015: Remove Tray::_active field from stub
- [x] P-016: ProxyError — use thiserror instead of manual Display impl

## Medium tasks (15-60 min each)

- [x] P-008: Move read_default_gateway() from main.rs to library crate
- [x] P-012: Consolidate whoami::username() calls
- [x] P-010: Add #[must_use] to key return types
- [x] P-009: Replace blocking_lock() with async lock in update.rs
- [x] P-007: Replace stringly-typed config values with enums
- [x] P-018: i18n — eliminate hardcoded locale checks in format_relative_time()

## Larger tasks (1+ hour each)

- [x] P-006: Add structured error context to SshError::Connect (with #[source])
- [x] P-003: Deduplicate CLI argument parsing (3 copies -> 1)
- [x] P-013: Add module-level documentation to update.rs and view/mod.rs
- [x] P-020: Dependency audit (regex caching with LazyLock)

## Major refactors

- [x] P-001a: Extract WelcomeState sub-struct and use it
- [x] P-001b: Extract SearchState sub-struct and use it
- [x] P-001c: Extract DialogState sub-struct and use it
- [x] P-002: Complete ConnectionState/TabBackend migration (remove boolean duplication)
- [x] P-005: Extract connection establishment logic (deduplicate 4 copies)
- [x] P-004: Eliminate #[allow(dead_code)] suppressions (message.rs, theme.rs; tab.rs deferred to P-002)
- [x] P-014: Add unit tests for app/update.rs (25 tests: backoff, tab index, paste, client-id, regex)

## Packaging

- [ ] NFR-DIST-01/02: Validate AppImage + .deb build scripts work with Rust binary
