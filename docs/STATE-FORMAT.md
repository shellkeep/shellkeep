<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# State File Format

This document describes the JSON formats used by shellkeep for persistence.

## State File

**Path**: `~/.local/share/shellkeep/state/<client-id>.json`

Stores the window/tab layout. Updated atomically (write to temp, rename).

```json
{
  "schema_version": 3,
  "last_modified": "1711667200Z",
  "client_id": "user-hostname",
  "workspaces": {
    "work": {
      "name": "work",
      "uuid": "a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d",
      "tabs": [
        {
          "session_uuid": "tab-0",
          "tmux_session_name": "shellkeep-0",
          "title": "user@server.com",
          "position": 0
        }
      ]
    }
  },
  "last_workspace": "work",
  "hidden_windows": [],
  "window_geometry": {}
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | u32 | Current version is 3. Used for migrations. |
| `last_modified` | string | Unix timestamp in seconds + "Z" suffix. |
| `client_id` | string | Unique client identifier (from config or auto-generated). |
| `workspaces` | object | Map of workspace name → workspace object. |
| `workspaces.*.name` | string | Workspace display name. |
| `workspaces.*.uuid` | string | Unique workspace identifier. |
| `workspaces.*.tabs` | array | Ordered list of tab states in this workspace. |
| `workspaces.*.tabs[].session_uuid` | string | Unique tab identifier. |
| `workspaces.*.tabs[].tmux_session_name` | string | Remote tmux session name. |
| `workspaces.*.tabs[].title` | string | Display title of the tab. |
| `workspaces.*.tabs[].position` | usize | Position in the tab bar (0-indexed). |
| `last_workspace` | string | Name of the last active workspace. |
| `hidden_windows` | array | List of hidden window IDs. |
| `window_geometry` | object | Per-device window positions and sizes. |

## Recent Connections

**Path**: `~/.local/share/shellkeep/recent.json`

Stores the last 50 SSH connections.

```json
{
  "connections": [
    {
      "label": "user@server.com",
      "ssh_args": ["user@server.com", "-p", "2222"],
      "host": "server.com",
      "user": "user",
      "port": "2222",
      "alias": null,
      "last_connected": 1711667200,
      "host_key_fingerprint": null
    }
  ]
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `label` | string | Display label (usually "user@host"). |
| `ssh_args` | string[] | CLI arguments passed to ssh. |
| `host` | string | Hostname or IP. |
| `user` | string | SSH username. |
| `port` | string | SSH port. |
| `alias` | string? | Optional friendly name. |
| `last_connected` | u64? | Unix timestamp of last connection. |
| `host_key_fingerprint` | string? | SHA256 host key fingerprint. |

## Configuration

**Path**: `~/.config/shellkeep/config.toml`

See [config.rs](../src/config.rs) for all sections and defaults.

## Crash Dumps

**Path**: `~/.local/state/shellkeep/crashes/crash-<timestamp>-<pid>.txt`

Plain text file with backtrace. Never contains terminal content or credentials.

## Log File

**Path**: `~/.local/state/shellkeep/logs/shellkeep.log`

Append-only log with tracing output. No ANSI colors.
