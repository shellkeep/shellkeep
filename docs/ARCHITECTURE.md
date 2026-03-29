<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# shellkeep Architecture

This document describes the architecture, data flow, and key design decisions
of shellkeep v0.3.0 (Rust rewrite).

## Overview

shellkeep is a cross-platform SSH terminal manager built in Rust. It uses:

- **iced** — GPU-accelerated UI framework (wgpu backend)
- **alacritty_terminal** — VT100/xterm terminal emulation (same as Zed editor)
- **russh** — Pure Rust SSH client for control operations
- **System ssh** — Terminal PTY I/O (via iced_term fork)

```
┌─────────────────────────────────────────────────┐
│                   shellkeep                      │
│                                                  │
│  ┌──────────┐  ┌───────────┐  ┌──────────────┐ │
│  │   iced   │  │ iced_term │  │    russh     │ │
│  │  (UI)    │  │ (terminal)│  │  (control)   │ │
│  └────┬─────┘  └─────┬─────┘  └──────┬───────┘ │
│       │              │               │          │
│       │         ┌────┴────┐    ┌─────┴──────┐  │
│       │         │alacritty│    │  exec/SFTP  │  │
│       │         │terminal │    │  channels   │  │
│       │         └────┬────┘    └─────┬──────┘  │
│       │              │               │          │
│       └──────────────┴───────────────┘          │
│                      │                           │
│              ┌───────┴──────┐                   │
│              │  SSH / tmux  │                   │
│              │   (remote)   │                   │
│              └──────────────┘                   │
└─────────────────────────────────────────────────┘
```

## Module Structure

```
src/
  main.rs           — iced Application: UI, tabs, messages, views
  lib.rs            — library exports for testing
  config.rs         — TOML configuration
  crash.rs          — crash handler, core dump prevention
  theme.rs          — Catppuccin Mocha color palette

  ssh/
    connection.rs   — russh: connect, authenticate, open channels
    tmux.rs         — tmux session detection, creation (system ssh + russh)

  state/
    recent.rs       — recent connections (JSON, max 20)
    state_file.rs   — tab layout persistence (JSON, atomic writes)
    permissions.rs  — file/dir permission enforcement (0600/0700)

crates/
  iced_term/        — forked terminal widget
    backend.rs      — PTY or SSH channel backend
    view.rs         — iced Widget rendering + input handling
    terminal.rs     — Terminal lifecycle + subscriptions
    theme.rs        — color palette
    bindings.rs     — keyboard/mouse bindings
    font.rs         — font metrics
```

## Data Flow

### Terminal I/O (interactive session)

```
User types key
  → iced keyboard event
  → iced_term view.rs captures it
  → iced_term bindings.rs resolves to action
  → backend.rs writes to PTY (system ssh process)
  → ssh sends to remote server
  → remote outputs response
  → ssh PTY receives data
  → alacritty_terminal EventLoop reads from PTY
  → alacritty_terminal Term processes escape sequences
  → iced_term renders grid via iced Canvas
  → wgpu renders to screen
```

### Control operations (russh)

```
App needs to list/create tmux sessions
  → ssh::connection::connect() via russh
  → ssh::connection::exec_command() opens channel
  → runs "tmux list-sessions" or "tmux new-session"
  → parses output
  → returns to app for tab management
```

### State persistence

```
Tab opened/closed/renamed
  → ShellKeep::save_state()
  → StateFile serialized to JSON
  → atomic write: tmp file → rename
  → ~/.local/share/shellkeep/state/<client-id>.json

On reconnect:
  → StateFile::load_local()
  → match saved tabs to live tmux sessions by name
  → restore tab labels from saved state
```

## Key Design Decisions

### Hybrid SSH approach

Terminal I/O uses the system `ssh` binary via a PTY (iced_term spawns it).
This gives us:
- Full SSH config support (~/.ssh/config, ProxyJump, etc.)
- Agent forwarding, FIDO keys, all auth methods
- Proven stability and compatibility

Control operations use russh (pure Rust SSH) for:
- Programmatic command execution
- Future: SFTP file operations
- Future: host key verification UI
- Non-blocking async operations

### Forked iced_term

We forked iced_term (originally by Harzu) to add:
- **SSH backend**: `Backend::new_ssh()` for SSH channel-backed terminals
- **Keyboard pass-through**: Ctrl+Shift shortcuts reach the app
- **Shift+PageUp/Down**: scrollback navigation
- **Right-click events**: context menu support
- **Reasonable default size**: 100x30 instead of 80x50 with 1px cells

### tmux integration

Each tab runs inside a tmux session (`shellkeep-0`, `shellkeep-1`, etc.):
- Sessions survive SSH disconnects
- `tmux new-session -A -s <name>` creates or reattaches
- Status bar hidden (`tmux set status off`)
- TERM=xterm-256color for proper rendering

### Auto-reconnection

When SSH drops (iced_term `Shutdown` event):
1. Tab terminal set to None, auto_reconnect flag set
2. 3-second timer triggers reconnection attempt
3. New ssh+tmux process spawned, reattaches to same session
4. After max attempts, tab shows dead state with manual Reconnect button

## Security

- Core dumps disabled via `prctl(PR_SET_DUMPABLE, 0)` on Linux
- File permissions: directories 0700, files 0600 (verified on startup)
- Passwords never stored (recent connections have no password field)
- Crash dumps never contain terminal content or credentials
- Logs never contain sensitive data

## Testing

- **Unit tests**: config parsing, host input parsing, recent connections, state serialization
- **E2E tmux tests**: SSH connectivity, session create/persist/reattach/list
- **E2E russh tests**: russh connect, exec, shell with PTY
- All tests run in CI on Linux, macOS, Windows
