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
│                       UI Layer                       │
│   GTK windows, tabs, tray icon, dialogs, menus       │
│   Header: sk_ui.h                                    │
├──────────────────────────┬──────────────────────────┤
│     Terminal Layer        │       State Layer         │
│   VTE widgets, I/O        │   JSON persistence,       │
│   routing, scrollback     │   JSONL history, lock,    │
│   Header: sk_terminal.h  │   SFTP sync               │
│                           │   Header: sk_state.h      │
├──────────────────────────┴──────────────────────────┤
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
│  state, restore       │  │  manager, NM D-Bus    │
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
| UI does not include SSH | `sk_ui.h` never includes `sk_ssh.h` directly |
| SSH does not call GTK | SSH layer has no GTK dependencies |
| State does not call GTK | State layer has no GTK dependencies |
| Opaque types | Each layer exposes opaque pointer types (e.g., `SkSshConnection *`) |
| Callback communication | Layers communicate via function pointers and header-defined interfaces |

This separation enables unit testing per layer, isolated contributions,
and future extensibility (plugins, alternative frontends, daemon mode).

## Data Flow

### Connection Establishment

```
User input (CLI or GUI)
    │
    ▼
┌─────────┐   SSH handshake    ┌─────────────┐
│ UI Layer │ ─────────────────> │  SSH Layer   │
└─────────┘                     └──────┬──────┘
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
                                │ UI Layer      │
                                └──────┬───────┘
                                       │
          Create windows/tabs          │  Restore layout
          per state file               │
                                       ▼
                                ┌────────────────┐
                                │ Terminal Layer  │
                                └────────────────┘
                                       │
          Each tab: independent SSH    │  tmux attach-session
          connection + VTE widget      │
```

### Terminal I/O (per tab)

```
Keyboard Input
    │
    ▼
┌────────────────┐   write    ┌───────────────┐   SSH channel   ┌────────┐
│ VTE Terminal   │ ─────────> │ Terminal Layer │ ──────────────> │ Server │
│ (GTK widget)   │            └───────────────┘                 │ (tmux  │
│                │                                               │ session│
│                │   feed     ┌───────────────┐   SSH channel   │        │
│                │ <───────── │ Terminal Layer │ <────────────── │        │
└────────────────┘            └───────────────┘                 └────────┘

I/O is non-blocking, integrated with GLib main loop via g_io_add_watch().
```

### State Persistence

```
Layout change (tab move, window resize, etc.)
    │
    ▼
┌──────────────┐  debounce (2s)   ┌──────────────┐
│ UI Layer     │ ───────────────> │ State Layer   │
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

### Reconnection

```
Keepalive timeout detected
    │
    ▼
┌────────────────────┐
│ Connection Manager │   Per-server, centralized
└────────┬───────────┘
         │
         │  1. Try master connection first
         │  2. On success, reconnect tabs in batches of 5
         │  3. Exponential backoff: 2s, 4s, 8s, 16s, 32s, 60s...
         │  4. Jitter: +/- 25%
         │
         ▼
┌────────────────────┐
│ Per-tab spinner    │   "Reconnecting... attempt 2/10, next in 4s"
│ overlay            │
└────────────────────┘
```

## Threading Model

shellkeep uses a hybrid threading model:

| Operation | Thread | Mechanism |
|---|---|---|
| GTK rendering, user input | Main thread | GMainLoop |
| SSH data channel I/O | Main thread | `g_io_add_watch()` on SSH fd |
| SSH handshake, auth | Worker thread | `GTask` / `g_task_run_in_thread()` |
| SFTP file operations | Worker thread | `GTask` |
| tmux commands | Worker thread | `GTask` |
| State file writes | Worker thread | `GTask` |
| Log writes | Dedicated thread | Lock-free ring buffer |
| JSONL history writes | Worker thread | `GTask` |

**Invariant:** No blocking I/O ever executes on the GTK main thread.

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
  shellkeep.sock                IPC socket
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

### Why GTK 3 instead of GTK 4?

VTE (the terminal widget) had more mature GTK 3 support at the time of
initial development. Migration to GTK 4 is planned for a future version.

## Related Documents

- [STATE-FORMAT.md](STATE-FORMAT.md) -- JSON schema for state files
- [REQUIREMENTS.md](../REQUIREMENTS.md) -- Full requirements registry
- [CONTRIBUTING.md](../CONTRIBUTING.md) -- Development setup and guidelines
