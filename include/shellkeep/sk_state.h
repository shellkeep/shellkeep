// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_state.h
 * @brief State persistence layer for shellkeep.
 *
 * Provides structs and functions for managing application state:
 * - State file (environments, windows, tabs) with JSON serialization
 * - Atomic read/write with tmp+rename (INV-STATE-1)
 * - Schema versioning and migration (FR-STATE-08)
 * - Corruption detection and recovery (FR-STATE-13..17)
 * - Debounced saves (NFR-PERF-07, FR-STATE-03)
 * - Local cache under XDG paths
 * - Recent connections (Appendix A.3)
 * - Session history JSONL/raw (FR-HISTORY-*)
 * - XDG path management (NFR-XDG-*)
 * - File permissions enforcement (INV-SECURITY-3)
 */

#ifndef SK_STATE_H
#define SK_STATE_H

#include <glib.h>

#include <stdbool.h>
#include <stdint.h>
#include <time.h>

G_BEGIN_DECLS

/* --------------------------------------------------------------------------
 * Constants
 * -------------------------------------------------------------------------- */

#define SK_STATE_SCHEMA_VERSION 1
#define SK_RECENT_SCHEMA_VERSION 1
#define SK_RECENT_MAX_ENTRIES 50
#define SK_HISTORY_MAX_FILE_SIZE_MB 50
#define SK_HISTORY_MAX_TOTAL_SIZE_MB 500
#define SK_HISTORY_DEFAULT_MAX_DAYS 90
#define SK_STATE_DEBOUNCE_INTERVAL_MS 2000
#define SK_DIR_PERMISSIONS 0700
#define SK_FILE_PERMISSIONS 0600

/* --------------------------------------------------------------------------
 * XDG Path Functions (NFR-XDG-*)
 * -------------------------------------------------------------------------- */

/**
 * Get the shellkeep config directory ($XDG_CONFIG_HOME/shellkeep/).
 * Creates with 0700 on first use. Caller must g_free().
 * NFR-XDG-01
 */
char *sk_paths_config_dir(void);

/**
 * Get the shellkeep data directory ($XDG_DATA_HOME/shellkeep/).
 * Creates with 0700 on first use. Caller must g_free().
 * NFR-XDG-02
 */
char *sk_paths_data_dir(void);

/**
 * Get the shellkeep state directory ($XDG_STATE_HOME/shellkeep/).
 * Creates with 0700 on first use. Caller must g_free().
 * NFR-XDG-03
 */
char *sk_paths_state_dir(void);

/**
 * Get the shellkeep runtime directory ($XDG_RUNTIME_DIR/shellkeep/).
 * Creates with 0700 on first use. Caller must g_free().
 * NFR-XDG-04
 */
char *sk_paths_runtime_dir(void);

/**
 * Get the shellkeep cache directory ($XDG_CACHE_HOME/shellkeep/).
 * Creates with 0700 on first use. Caller must g_free().
 * NFR-XDG-05
 */
char *sk_paths_cache_dir(void);

/**
 * Get the local cache directory for a specific server.
 * Path: $XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/
 * Creates with 0700 on first use. Caller must g_free().
 * FR-STATE-01
 */
char *sk_paths_server_cache_dir(const char *host_fingerprint);

/**
 * Get the logs directory ($XDG_STATE_HOME/shellkeep/logs/).
 * Creates with 0700 on first use. Caller must g_free().
 */
char *sk_paths_logs_dir(void);

/**
 * Get the crashes directory ($XDG_STATE_HOME/shellkeep/crashes/).
 * Creates with 0700 on first use. Caller must g_free().
 */
char *sk_paths_crashes_dir(void);

/* --------------------------------------------------------------------------
 * Permissions (INV-SECURITY-3, NFR-SEC-01..03)
 * -------------------------------------------------------------------------- */

/**
 * Verify and correct permissions on all shellkeep directories and files.
 * Directories: 0700, files: 0600.
 * Called at startup.
 * Returns TRUE on success, FALSE if any permission could not be set.
 */
bool sk_permissions_verify_and_fix(void);

/**
 * Ensure a single file has 0600 permissions.
 * Returns TRUE on success.
 */
bool sk_permissions_fix_file(const char *path);

/**
 * Ensure a single directory has 0700 permissions.
 * Returns TRUE on success.
 */
bool sk_permissions_fix_dir(const char *path);

/* --------------------------------------------------------------------------
 * State Structs (Appendix A.1 — FR-STATE-13..16)
 * -------------------------------------------------------------------------- */

/** Window geometry (optional). */
typedef struct
{
  int x;
  int y;
  int width;
  int height;
  bool is_set; /**< FALSE when geometry was not specified. */
} SkGeometry;

/** A single tab in a window (FR-STATE-15). */
typedef struct _SkTab
{
  char *session_uuid;      /**< UUID v4. Primary stable identifier. */
  char *tmux_session_name; /**< Name of the tmux session on the server. */
  char *title;             /**< Visible tab title. */
  int position;            /**< 0-indexed position in the tab bar. */
} SkTab;

/** A window containing tabs (FR-STATE-14). */
typedef struct _SkWindow
{
  char *id;            /**< UUID v4. Stable window identifier. */
  char *title;         /**< Visible window title. */
  bool visible;        /**< Whether the window is shown or hidden. */
  SkGeometry geometry; /**< Optional saved geometry. */
  SkTab **tabs;        /**< NULL-terminated array of tabs. */
  int n_tabs;          /**< Number of tabs. Must be >= 1. */
  int active_tab;      /**< 0-indexed active tab. Default: 0. */
} SkWindow;

/** An environment — a named group of windows. */
typedef struct
{
  char *name;         /**< Environment name (key in the map). */
  SkWindow **windows; /**< NULL-terminated array of windows. */
  int n_windows;      /**< Number of windows. */
} SkEnvironment;

/** Top-level state file (Appendix A.1 — FR-STATE-13). */
typedef struct _SkStateFile
{
  int schema_version;           /**< Must be positive integer. */
  char *last_modified;          /**< ISO 8601 UTC timestamp. */
  char *client_id;              /**< The client-id owning this file. */
  SkEnvironment **environments; /**< NULL-terminated array. */
  int n_environments;           /**< Number of environments. */
  char *last_environment;       /**< Must reference an existing env. */
} SkStateFile;

/* --------------------------------------------------------------------------
 * State Lifecycle
 * -------------------------------------------------------------------------- */

/**
 * Allocate a new empty SkStateFile with defaults.
 * Caller must free with sk_state_file_free().
 */
SkStateFile *sk_state_file_new(const char *client_id);

/**
 * Free a state file and all its children.
 */
void sk_state_file_free(SkStateFile *state);

/**
 * Allocate a new SkEnvironment.
 */
SkEnvironment *sk_environment_new(const char *name);

/**
 * Free an environment and all its children.
 */
void sk_environment_free(SkEnvironment *env);

/**
 * Allocate a new SkWindow.
 */
SkWindow *sk_window_new(const char *id, const char *title);

/**
 * Free a window and all its children.
 */
void sk_window_free(SkWindow *win);

/**
 * Allocate a new SkTab.
 */
SkTab *sk_tab_new(const char *session_uuid, const char *tmux_session_name, const char *title,
                  int position);

/**
 * Free a tab.
 */
void sk_tab_free(SkTab *tab);

/* --------------------------------------------------------------------------
 * State I/O — Atomic Read/Write (INV-STATE-1, FR-STATE-04..07)
 * -------------------------------------------------------------------------- */

/**
 * Load state from a JSON file. Performs schema validation.
 * On parse failure: renames file to .corrupt.<timestamp>, returns NULL.
 * On version too high: sets error, returns NULL.
 * On version too low: auto-migrates, creates .v<old>.bak backup.
 *
 * @param path      Path to the JSON state file.
 * @param error     GError output (nullable).
 * @return Parsed state, or NULL on error. Caller must free.
 */
SkStateFile *sk_state_load(const char *path, GError **error);

/**
 * Save state to a JSON file atomically (tmp+rename).
 * Updates last_modified to current UTC time.
 * Sets file permissions to 0600. (FR-STATE-04, INV-STATE-1)
 *
 * @param state     The state to save.
 * @param path      Destination path.
 * @param error     GError output (nullable).
 * @return TRUE on success.
 */
bool sk_state_save(SkStateFile *state, const char *path, GError **error);

/**
 * Serialize state to a JSON string.
 * Caller must g_free() the result.
 */
char *sk_state_to_json(const SkStateFile *state);

/**
 * Parse state from a JSON string.
 * Returns NULL on error, setting error.
 */
SkStateFile *sk_state_from_json(const char *json, GError **error);

/**
 * Clean up orphaned .tmp files in the given directory.
 * Called at startup. (FR-STATE-07)
 */
void sk_state_cleanup_tmp_files(const char *dir_path);

/**
 * Validate state file integrity invariants (FR-STATE-16).
 * Returns TRUE if valid, FALSE with error details otherwise.
 */
bool sk_state_validate(const SkStateFile *state, GError **error);

/* --------------------------------------------------------------------------
 * Local Cache (FR-STATE-01..02)
 * -------------------------------------------------------------------------- */

/**
 * Save a local cache copy of the state file.
 * Path: $XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/<client_id>.json
 */
bool sk_state_save_local_cache(const SkStateFile *state, const char *host_fingerprint,
                               GError **error);

/**
 * Load state from local cache. Returns NULL if not found.
 */
SkStateFile *sk_state_load_local_cache(const char *host_fingerprint, const char *client_id,
                                       GError **error);

/* --------------------------------------------------------------------------
 * Debounce (FR-STATE-03, NFR-PERF-07)
 * -------------------------------------------------------------------------- */

/** Opaque debounce context. */
typedef struct _SkStateDebounce SkStateDebounce;

/**
 * Create a debounce context.
 * @param save_path        The file path for saves.
 * @param host_fingerprint For local cache saves (nullable to skip cache).
 */
SkStateDebounce *sk_state_debounce_new(const char *save_path, const char *host_fingerprint);

/**
 * Schedule a save. At most one write every SK_STATE_DEBOUNCE_INTERVAL_MS.
 * Takes ownership of the state reference (will free old pending state).
 * The state is copied internally.
 */
void sk_state_schedule_save(SkStateDebounce *debounce, const SkStateFile *state);

/**
 * Flush any pending save immediately (e.g., on shutdown).
 */
void sk_state_debounce_flush(SkStateDebounce *debounce);

/**
 * Free the debounce context and flush pending saves.
 */
void sk_state_debounce_free(SkStateDebounce *debounce);

/* --------------------------------------------------------------------------
 * Recent Connections (Appendix A.3 — NFR-SEC-08)
 * -------------------------------------------------------------------------- */

/** A single recent connection entry. */
typedef struct
{
  char *host;                 /**< Hostname or IP. */
  char *user;                 /**< Username. */
  int port;                   /**< Port number (default: 22). */
  char *alias;                /**< Optional display name. */
  char *last_connected;       /**< ISO 8601 UTC timestamp. */
  char *host_key_fingerprint; /**< Optional SHA256 fingerprint. */
} SkRecentConnection;

/** Recent connections file. */
typedef struct
{
  int schema_version;
  SkRecentConnection **connections; /**< NULL-terminated array. */
  int n_connections;
} SkRecentConnections;

/**
 * Load recent connections from default path.
 * Returns empty list if file doesn't exist.
 */
SkRecentConnections *sk_recent_load(GError **error);

/**
 * Save recent connections to default path atomically.
 * Enforces max 50 entries and 0600 permissions. (NFR-SEC-11)
 */
bool sk_recent_save(const SkRecentConnections *recent, GError **error);

/**
 * Add or merge a connection. Duplicates (same host+user+port) are merged
 * by updating timestamp. NEVER saves passwords. (INV-SECURITY-2)
 */
void sk_recent_add(SkRecentConnections *recent, const char *host, const char *user, int port,
                   const char *alias, const char *host_key_fingerprint);

/**
 * Free a recent connections list.
 */
void sk_recent_free(SkRecentConnections *recent);

/**
 * Allocate an empty recent connections list.
 */
SkRecentConnections *sk_recent_new(void);

/**
 * Free a single recent connection entry.
 */
void sk_recent_connection_free(SkRecentConnection *conn);

/* --------------------------------------------------------------------------
 * Session History — JSONL/raw (FR-HISTORY-*)
 * -------------------------------------------------------------------------- */

/** History event type. */
typedef enum
{
  SK_HISTORY_OUTPUT = 0,     /**< Terminal output. */
  SK_HISTORY_INPUT_ECHO = 1, /**< Echoed input. */
  SK_HISTORY_RESIZE = 2,     /**< Terminal resize event. */
  SK_HISTORY_META = 3,       /**< Metadata event. */
} SkHistoryEventType;

/** Resize dimensions (for SK_HISTORY_RESIZE). */
typedef struct
{
  int cols;
  int rows;
} SkHistorySize;

/** A single history event (one JSONL line). */
typedef struct
{
  char *ts;                /**< ISO 8601 timestamp with timezone. */
  char *text;              /**< Text content (for output/input_echo). */
  SkHistoryEventType type; /**< Event type. */
  SkHistorySize size;      /**< Valid only when type == RESIZE. */
  char *raw_hex;           /**< Optional hex for non-UTF-8 bytes. */
} SkHistoryEvent;

/**
 * Append a history event to a JSONL file.
 * Creates the file if it doesn't exist (with 0600 perms).
 * Append-only (INV-JSONL-1). Temporal order preserved (INV-JSONL-2).
 *
 * @param session_uuid  Session UUID for filename.
 * @param event         The event to append.
 * @param base_dir      Base directory (e.g., ~/.terminal-state/history/).
 * @param error         GError output (nullable).
 * @return TRUE on success.
 */
bool sk_history_append(const char *session_uuid, const SkHistoryEvent *event, const char *base_dir,
                       GError **error);

/**
 * Read all valid events from a JSONL file.
 * Discards the last line if truncated/invalid (FR-HISTORY-09).
 *
 * @param session_uuid  Session UUID for filename.
 * @param base_dir      Base directory.
 * @param n_events      Output: number of events read.
 * @param error         GError output (nullable).
 * @return Array of events (caller must free each + the array), or NULL.
 */
SkHistoryEvent **sk_history_read(const char *session_uuid, const char *base_dir, int *n_events,
                                 GError **error);

/**
 * Rotate a JSONL file if it exceeds max_size_mb.
 * Truncates oldest 25% via tmp+rename. (FR-HISTORY-05)
 *
 * @param session_uuid  Session UUID for filename.
 * @param base_dir      Base directory.
 * @param max_size_mb   Maximum file size in MB.
 * @param error         GError output (nullable).
 * @return TRUE on success or if no rotation needed.
 */
bool sk_history_rotate(const char *session_uuid, const char *base_dir, int max_size_mb,
                       GError **error);

/**
 * Clean up history files by age and total size.
 * Removes files older than max_days; if total > max_total_mb,
 * removes oldest files until under limit. (FR-HISTORY-06, FR-HISTORY-07)
 *
 * @param base_dir       Base directory.
 * @param max_days       Maximum age in days.
 * @param max_total_mb   Maximum total size in MB.
 * @param error          GError output (nullable).
 * @return TRUE on success.
 */
bool sk_history_cleanup(const char *base_dir, int max_days, int max_total_mb, GError **error);

/**
 * Free a single history event.
 */
void sk_history_event_free(SkHistoryEvent *event);

/**
 * Serialize a history event to a single JSON line (no trailing newline).
 * Caller must g_free().
 */
char *sk_history_event_to_json(const SkHistoryEvent *event);

/**
 * Parse a single JSON line into a history event.
 * Returns NULL on parse failure.
 */
SkHistoryEvent *sk_history_event_from_json(const char *json_line);

/* --------------------------------------------------------------------------
 * Error Domain
 * -------------------------------------------------------------------------- */

#define SK_STATE_ERROR (sk_state_error_quark())
GQuark sk_state_error_quark(void);

typedef enum
{
  SK_STATE_ERROR_PARSE,          /**< JSON parse failure. */
  SK_STATE_ERROR_SCHEMA,         /**< Schema validation failure. */
  SK_STATE_ERROR_VERSION_FUTURE, /**< Version higher than supported. */
  SK_STATE_ERROR_IO,             /**< File I/O error. */
  SK_STATE_ERROR_CORRUPT,        /**< Corruption detected. */
  SK_STATE_ERROR_PERMISSION,     /**< Permission error. */
} SkStateError;

G_END_DECLS

#endif /* SK_STATE_H */
