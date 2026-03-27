<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# shellkeep Architecture

This document describes the layered architecture, data flow, and key design
decisions of shellkeep.

## Layer Diagram

```
┌─────────────────────────────────────────────────────┐
│                    Qt6 UI Layer                       │
│   SkMainWindow, tabs, tray, dialogs, welcome         │
│   Headers: sk_ui_qt.h, sk_terminal_qt.h              │
├──────────────────────────┬──────────────────────────┤
│     Terminal Layer        │       State Layer         │
│   SkTerminalWidget, I/O   │   JSON persistence,       │
│   QSocketNotifier, search │   JSONL history, lock,    │
│   SkTerminalDead          │   SFTP sync               │
│                           │   Header: sk_state.h      │
├──────────────────────────┴──────────────────────────┤
│                 UI Bridge (sk_ui_bridge.h)            │
│   Toolkit-agnostic callback vtable                   │
│   Decouples C backend from Qt6 UI                    │
├─────────────────────────────────────────────────────┤
│                    Session Layer                      │
│   tmux interaction: create, attach, list, destroy     │
│   Control mode orchestration, session naming          │
│   Header: sk_session.h                               │
├─────────────────────────────────────────────────────┤
│                      SSH Layer                        │
│   libssh connections, authentication, channels        │
│   SFTP, keepalive, reconnection, algorithm config     │
│   Header: sk_ssh.h                                   │
└─────────────────────────────────────────────────────┘
```

### Orchestration Modules

```
┌──────────────────────┐  ┌──────────────────────┐
│      Connect          │  │     Reconnect         │
│  End-to-end connect   │  │  Exponential backoff  │
│  flow: host key,      │  │  with jitter, per-    │
│  auth, tmux, lock,    │  │  server connection    │
│  state, restore       │  │  manager              │
│  Header: sk_connect.h │  │  Header: sk_reconnect.h│
└──────────────────────┘  └──────────────────────┘
```

### Supporting Modules

```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│   Config      │  │     Log       │  │    Types      │  │     i18n      │
│  INI parse,   │  │  Async ring   │  │  Shared enum  │  │  gettext      │
│  defaults,    │  │  buffer, file  │  │  and struct   │  │  macros       │
│  validation   │  │  rotation     │  │  definitions  │  │               │
│  sk_config.h  │  │  sk_log.h     │  │  sk_types.h   │  │  sk_i18n.h    │
└──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘
```

## Dependency Rules

These rules are strictly enforced and verified in code review:

| Rule | Description |
|---|---|
| UI does not include SSH | `sk_ui_qt.h` never includes `sk_ssh.h` directly |
| SSH does not call UI | SSH layer has no Qt or GTK dependencies |
| State does not call UI | State layer has no Qt or GTK dependencies |
| Backend uses bridge | Connect layer uses `sk_ui_bridge.h` — no toolkit headers |
| Opaque types | Each layer exposes opaque pointer types (e.g., `SkSshConnection *`) |
| Callback communication | Backend ↔ UI via bridge vtable function pointers |

This separation enables:
- Unit testing per layer in isolation
- Cross-platform UI (bridge implementations for Qt6, future toolkits)
- Daemon mode without UI

## Data Flow

### Connection Establishment

```
User input (CLI or GUI)
    │
    ▼
┌──────────┐   via UI Bridge    ┌─────────────┐
│ Qt6 UI   │ ─────────────────> │  SSH Layer   │
└──────────┘                    └──────┬──────┘
                                       │
                    Authenticated       │
                                       ▼
                                ┌──────────────┐
                                │ Session Layer │
                                └──────┬───────┘
                                       │
          tmux -CC (control mode)      │  tmux -V, lock check
          list sessions                │
                                       ▼
                                ┌──────────────┐
                                │ State Layer   │
                                └──────┬───────┘
                                       │
          Load <client-id>.json        │  via SFTP
          Reconcile with server        │
                                       ▼
                                ┌──────────────┐
                                │ Qt6 UI       │
                                └──────┬───────┘
                                       │
          Create windows/tabs          │  Restore layout
          per state file               │
                                       ▼
                                ┌─────────────────┐
                                │ Terminal Layer   │
                                │ SkTerminalWidget │
                                └─────────────────┘
                                       │
          Each tab: independent SSH    │  tmux attach-session
          connection + QTermWidget     │
```

### Terminal I/O (per tab)

```
Keyboard Input
    │
    ▼
┌─────────────────┐  write   ┌───────────────┐  SSH channel  ┌────────┐
│ SkTerminalWidget│ ───────> │ Terminal Layer │ ────────────> │ Server │
│ (Qt widget)     │          └───────────────┘               │ (tmux  │
│                 │                                           │ session│
│                 │  feed    ┌───────────────┐  SSH channel  │        │
│                 │ <─────── │ Terminal Layer │ <──────────── │        │
└─────────────────┘          └───────────────┘               └────────┘

I/O is non-blocking, integrated via QSocketNotifier on SSH fd.
```

### State Persistence

```
Layout change (tab move, window resize, etc.)
    │
    ▼
┌──────────────┐  debounce (2s)   ┌──────────────┐
│ Qt6 UI       │ ───────────────> │ State Layer   │
└──────────────┘                  └──────┬───────┘
                                         │
                   1. Write to local     │
                      cache (sync)       │
                                         │
                   2. Write to server    │  GTask (worker thread)
                      via SFTP           │
                      (tmp + rename)     │
                                         ▼
                                  ┌──────────────┐
                                  │    Server     │
                                  │ ~/.terminal-  │
                                  │ state/<cid>.  │
                                  │ json          │
                                  └──────────────┘
```

## Threading Model

shellkeep uses a hybrid threading model:

| Operation | Thread | Mechanism |
|---|---|---|
| Qt rendering, user input | Main thread | Qt event loop |
| SSH data channel I/O | Main thread | `QSocketNotifier` on SSH fd |
| SSH handshake, auth | Worker thread | `GTask` / `g_task_run_in_thread()` |
| SFTP file operations | Worker thread | `GTask` |
| tmux commands | Worker thread | `GTask` |
| State file writes | Worker thread | `GTask` |
| Log writes | Dedicated thread | Lock-free ring buffer |
| JSONL history writes | Worker thread | `GTask` |
| Dialog dispatch | UI thread | `QMetaObject::invokeMethod(Qt::BlockingQueuedConnection)` |

**Invariant:** No blocking I/O ever executes on the Qt main thread.

### GLib + Qt Event Loop Integration

- **Linux:** Qt's `QEventDispatcherGlib` handles GLib event loop natively
- **macOS/Windows:** GLib main context runs on a background `QThread`;
  cross-thread dispatch via `QMetaObject::invokeMethod`

## File System Layout

### Client

```
~/.config/shellkeep/          (XDG_CONFIG_HOME)
  config.ini                    Optional configuration overrides
  client-id                     Auto-generated UUID or user-defined name
  themes/                       Custom terminal color themes (JSON)

~/.local/share/shellkeep/     (XDG_DATA_HOME)
  recent_connections.json       Last 50 connections (host, user, port)
  cache/servers/
    <host-fingerprint>/
      <client-id>.json          Local cache of server state

~/.local/state/shellkeep/     (XDG_STATE_HOME)
  logs/
    shellkeep.log               Current log file
    shellkeep.log.1 ... .5      Rotated logs (10 MB each, max 5)
  crashes/
    crash-YYYYMMDD-HHMMSS-PID.txt

/run/user/$UID/shellkeep/     (XDG_RUNTIME_DIR)
  shellkeep.sock                IPC socket (single-instance via QLocalServer)
  shellkeep.pid                 PID file
```

### Server

```
~/.terminal-state/            Permission: 0700
  <client-id>.json              State file per client (0600)
  history/
    <session-uuid>.jsonl        Structured history (0600)
    <session-uuid>.raw          Raw tmux pipe-pane output (0600)
```

## Key Design Decisions

### Why libssh instead of the ssh binary?

Using libssh within the process ensures that `killall ssh` does not affect
shellkeep sessions. It also provides programmatic control over connections,
channels, and authentication without parsing command output.

### Why tmux and not screen or zellij?

tmux provides control mode (`tmux -CC`) for programmatic interaction,
a well-defined command API, and is widely deployed. Screen has limited
automation capabilities, and zellij's built-in UI conflicts with
shellkeep's architecture where the client owns all rendering.

### Why one SSH connection per tab?

Isolation: if one tab's connection has issues, other tabs are unaffected.
This also simplifies the threading model since each connection has its own
file descriptor in the event loop.

### Why Qt6 instead of GTK?

v0.1 used GTK3+VTE which only works on Linux. Qt6 provides:
- Cross-platform support (Linux, macOS, Windows) from a single codebase
- Modern widget toolkit with built-in system tray, dark mode, HiDPI
- QTermWidget for terminal emulation across all platforms
- Better C++ integration for the UI layer while backend stays C

### Why the UI Bridge pattern?

The `sk_ui_bridge.h` vtable decouples the C backend from any specific
toolkit. This means:
- The connect layer (`sk_connect.c`) never includes Qt headers
- Future toolkit migrations require only a new bridge implementation
- The backend can run headlessly (daemon mode) with a stub bridge
- Testing the backend doesn't require a display server

## Related Documents

- [STATE-FORMAT.md](STATE-FORMAT.md) -- JSON schema for state files
- [REQUIREMENTS.md](../REQUIREMENTS.md) -- Full requirements registry
- [CONTRIBUTING.md](../CONTRIBUTING.md) -- Development setup and guidelines
