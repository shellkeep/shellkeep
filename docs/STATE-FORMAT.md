<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# shellkeep State Format

This document describes the public schema for all persistent state files
used by shellkeep. These formats are considered stable within a major
version and are versioned via `schema_version`.

## 1. Main State File

### Location

- **Server (primary):** `~/.terminal-state/<client-id>.json`
- **Client (cache):** `$XDG_DATA_HOME/shellkeep/cache/servers/<host-fingerprint>/<client-id>.json`

The server file is the source of truth. The client cache enables fast
UI rendering before the SSH connection completes and serves as a fallback.

### Schema (v1)

```json
{
  "schema_version": 1,
  "last_modified": "2026-03-25T14:30:00Z",
  "client_id": "desktop-main",
  "environments": {
    "Project A": {
      "windows": [
        {
          "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
          "title": "Backend",
          "visible": true,
          "geometry": {
            "x": 100,
            "y": 200,
            "width": 1200,
            "height": 800
          },
          "tabs": [
            {
              "session_uuid": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
              "tmux_session_name": "desktop-main--Project-A--session-20260325-143000",
              "title": "session-20260325-143000",
              "position": 0
            }
          ],
          "active_tab": 0
        }
      ]
    }
  },
  "last_environment": "Project A"
}
```

### Field Reference

#### Root Object

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | integer | Yes | Schema version. Current: `1`. Monotonically increasing. |
| `last_modified` | string | Yes | ISO 8601 UTC timestamp of the last write. |
| `client_id` | string | Yes | The client-id that owns this file. |
| `environments` | object | Yes | Map of environment name to EnvironmentSchema. |
| `last_environment` | string | Yes | Name of the last active environment. Must be a key in `environments`. |

#### EnvironmentSchema

| Field | Type | Required | Description |
|---|---|---|---|
| `windows` | array | Yes | Array of WindowSchema objects. May be empty. |

#### WindowSchema

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | Yes | UUID v4. Stable identifier for the window. |
| `title` | string | Yes | Visible window title. |
| `visible` | boolean | Yes | Whether the window is currently shown or hidden. |
| `geometry` | object | No | Window position and size. |
| `geometry.x` | integer | -- | X position in pixels. |
| `geometry.y` | integer | -- | Y position in pixels. |
| `geometry.width` | integer | -- | Width in pixels. |
| `geometry.height` | integer | -- | Height in pixels. |
| `tabs` | array | Yes | Array of TabSchema. Must contain at least one element. |
| `active_tab` | integer | No | 0-indexed position of the active tab. Default: `0`. |

#### TabSchema

| Field | Type | Required | Description |
|---|---|---|---|
| `session_uuid` | string | Yes | UUID v4. Primary identifier for the session. Corresponds to the history file name. |
| `tmux_session_name` | string | Yes | Full tmux session name on the server (`<client-id>--<env>--<name>`). |
| `title` | string | Yes | Visible tab title. |
| `position` | integer | Yes | 0-indexed position in the tab bar. |

### Integrity Invariants

1. `schema_version` must be a positive integer.
2. `last_environment` must reference an existing key in `environments`.
3. Every `session_uuid` must be unique across the entire file.
4. `active_tab` must be a valid index within the `tabs` array.
5. `tmux_session_name` must match: `^[a-zA-Z0-9_][a-zA-Z0-9_.:-]*$`

### Version Migration

When loading a state file:

| Condition | Behavior |
|---|---|
| `schema_version` > supported | Refuse to open. Display upgrade message. |
| `schema_version` == supported | Open normally. |
| `schema_version` < supported | Auto-migrate sequentially (v1 -> v2 -> v3...). Create backup as `<file>.v<old>.bak` before migration. |

### Write Protocol

State writes always use atomic rename:

1. Write complete JSON to a temporary file in the same directory (e.g., `<client-id>.json.tmp`)
2. Call `rename(2)` to atomically replace the target file
3. On SFTP, use `posix-rename@openssh.com` extension; fallback to `unlink` + `rename`

Writes are debounced: at most one write every 2 seconds.

### Corruption Recovery

| Scenario | Consequence | Recovery |
|---|---|---|
| Process dies during `.tmp` write | Orphan `.tmp` file; original intact | Delete `*.tmp` at startup |
| Process dies during `rename` | Impossible (atomic on Linux) | N/A |
| JSON parse failure | Corrupted state | Rename to `.corrupt.<timestamp>`, create fresh state |

## 2. Session History (JSONL)

### Location

`~/.terminal-state/history/<session-uuid>.jsonl`

### Format

Each line is a self-contained JSON object terminated by `\n`:

```json
{"ts":"2026-03-25T14:30:01.123Z","type":"output","text":"Build complete.\n"}
{"ts":"2026-03-25T14:30:02.456Z","type":"resize","size":{"cols":120,"rows":40}}
{"ts":"2026-03-25T14:30:03.789Z","type":"meta","text":"Session attached by desktop-main"}
```

### Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `ts` | string | Yes | ISO 8601 timestamp with timezone (UTC preferred). |
| `type` | string | No | Event type. Default: `"output"`. Values: `"output"`, `"input_echo"`, `"resize"`, `"meta"`. |
| `text` | string | Conditional | Terminal text. Required for `"output"` and `"input_echo"` types. |
| `size` | object | Conditional | Terminal dimensions. Required for `"resize"` type. |
| `size.cols` | integer | -- | Number of columns. |
| `size.rows` | integer | -- | Number of rows. |
| `raw_hex` | string | No | Hex-encoded original bytes when UTF-8 conversion lost data (bytes replaced with U+FFFD). |

### Encoding

All `text` values must be valid UTF-8. Invalid bytes from the terminal
stream are replaced with U+FFFD (REPLACEMENT CHARACTER), and the original
bytes are preserved in `raw_hex` for lossless reconstruction.

### Rotation

- Maximum file size: 50 MB per session
- On exceeding limit: truncate the oldest 25% via temp file + atomic rename
- Maximum age: 90 days (configurable)
- Maximum total size of `~/.terminal-state/history/`: 500 MB (configurable)

### Raw History

`~/.terminal-state/history/<session-uuid>.raw`

Raw terminal output captured via `tmux pipe-pane`. This file accumulates
even when the client is disconnected. On reconnection, the client reads
the `.raw` file and converts it to structured JSONL format.

## 3. Recent Connections

### Location

`$XDG_DATA_HOME/shellkeep/recent_connections.json`

### Schema (v1)

```json
{
  "schema_version": 1,
  "connections": [
    {
      "host": "server.com",
      "user": "deploy",
      "port": 22,
      "alias": "Production Server",
      "last_connected": "2026-03-25T14:30:00Z",
      "host_key_fingerprint": "SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8"
    }
  ]
}
```

### Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | integer | Yes | Schema version. Current: `1`. |
| `connections` | array | Yes | Array of connection entries. |
| `connections[].host` | string | Yes | Hostname or IP address. |
| `connections[].user` | string | Yes | SSH username. |
| `connections[].port` | integer | Yes | SSH port number. |
| `connections[].alias` | string | No | User-defined friendly name. |
| `connections[].last_connected` | string | Yes | ISO 8601 UTC timestamp. |
| `connections[].host_key_fingerprint` | string | No | SHA256 fingerprint of the server host key. |

Maximum 50 entries. Duplicates (same host + user + port) are merged,
keeping the most recent `last_connected` timestamp.

**Passwords are never stored in this file or any other file.**

## 4. tmux Lock Session

### Session Name

`shellkeep-lock-<client-id>`

### Environment Variables

| Variable | Type | Description |
|---|---|---|
| `SHELLKEEP_LOCK_CLIENT_ID` | string | The client-id (must match session name suffix). |
| `SHELLKEEP_LOCK_HOSTNAME` | string | Hostname of the client machine. |
| `SHELLKEEP_LOCK_CONNECTED_AT` | string | ISO 8601 timestamp of lock acquisition. |
| `SHELLKEEP_LOCK_PID` | string | PID of the shellkeep process (as string). |
| `SHELLKEEP_LOCK_VERSION` | string | shellkeep version (semver). |

### Lock Lifecycle

1. **Created** after SSH authentication, before reading state or reconnecting sessions.
2. **Heartbeat** updated every `keepalive_interval * 2` (default 30s) via `tmux set-environment`.
3. **Destroyed** as the last operation before closing the SSH connection.
4. **Orphan detection:** if `SHELLKEEP_LOCK_CONNECTED_AT` + (2 * keepalive timeout) has expired, the lock is considered orphaned and can be taken without user confirmation.

## 5. File Permissions

| Resource | Permission | Location |
|---|---|---|
| `~/.terminal-state/` | `0700` | Server |
| All files in `~/.terminal-state/` | `0600` | Server |
| `$XDG_CONFIG_HOME/shellkeep/` | `0700` | Client |
| `$XDG_DATA_HOME/shellkeep/` | `0700` | Client |
| `$XDG_STATE_HOME/shellkeep/` | `0700` | Client |
| All state/config/log files | `0600` | Client |

Permissions are verified at startup and corrected automatically if they
are more permissive than required.

## Related Documents

- [ARCHITECTURE.md](ARCHITECTURE.md) -- System architecture
- [REQUIREMENTS.md](../REQUIREMENTS.md) -- Full requirements registry
