<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Contributing to shellkeep

Thank you for your interest in contributing!

## Development Setup

### Prerequisites

- Rust (stable, >= 1.88)
- Linux: `libxkbcommon-dev libwayland-dev libvulkan-dev libfontconfig1-dev`
- macOS / Windows: no extra deps

### Build

```bash
git clone https://github.com/shellkeep/shellkeep.git
cd shellkeep
cargo build
```

### Run

```bash
cargo run -- user@host             # connect to server
cargo run                          # welcome screen
```

### Test

```bash
cargo test                                    # unit tests (13)
cargo test --test e2e_tmux -- --ignored       # tmux E2E (7, needs SSH server)
cargo test --test e2e_russh -- --ignored      # russh E2E (3, needs SSH server)
```

### Lint

```bash
cargo clippy -- -D warnings
cargo fmt -- --check
```

## Code Style

- Rust 2024 edition, `snake_case`
- `cargo fmt` for formatting
- `cargo clippy` must pass with no warnings
- SPDX headers on all `.rs` files:
  ```rust
  // SPDX-FileCopyrightText: 2026 shellkeep contributors
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```

## Project Structure

```
src/
  main.rs           — iced application, UI, tab management
  lib.rs            — library exports
  config.rs         — TOML config
  crash.rs          — crash handler, core dump prevention
  theme.rs          — Catppuccin color palette
  ssh/
    connection.rs   — russh SSH connect, auth, channels
    tmux.rs         — tmux session detection and management
  state/
    recent.rs       — recent connections persistence
    state_file.rs   — tab layout persistence (JSON)
    permissions.rs  — file permission enforcement
crates/
  iced_term/        — forked terminal widget (alacritty_terminal + iced)
tests/
  e2e_tmux.rs       — E2E tests against live SSH server
  e2e_russh.rs      — russh integration tests
```

## Pull Requests

1. Fork and create a feature branch
2. Write tests for your changes
3. `cargo test && cargo clippy -- -D warnings && cargo fmt -- --check`
4. Submit PR — CI runs on Linux, macOS, Windows

## Security

- Never log passwords, keys, terminal content, or environment variables
- Files: 0600, directories: 0700
- See [SECURITY.md](SECURITY.md) for vulnerability disclosure

## License

Contributions licensed under GPL-3.0-or-later.
