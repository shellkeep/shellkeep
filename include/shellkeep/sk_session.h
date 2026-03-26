// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_session.h
 * @brief Session layer — tmux interaction public API.
 *
 * Manages tmux sessions on the remote server: detect tmux, control mode,
 * create/attach/list/kill sessions, lock mechanism, history via pipe-pane,
 * and reconciliation.
 *
 * This layer sits between SSH (below) and Terminal/UI (above).
 *
 * Requirements: FR-SESSION-*, FR-LOCK-*, FR-HISTORY-01..03, FR-CONN-13..15
 */

#ifndef SK_SESSION_H
#define SK_SESSION_H

#include "shellkeep/sk_types.h"

#include <glib.h>

#include <stdbool.h>
#include <time.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Error domain                                                        */
  /* ------------------------------------------------------------------ */

#define SK_SESSION_ERROR (sk_session_error_quark())
  GQuark sk_session_error_quark(void);

  typedef enum
  {
    SK_SESSION_ERROR_TMUX_NOT_FOUND, /**< tmux not installed on server. */
    SK_SESSION_ERROR_TMUX_VERSION,   /**< tmux version too old. */
    SK_SESSION_ERROR_CONTROL_MODE,   /**< Control mode protocol error. */
    SK_SESSION_ERROR_SESSION,        /**< Session create/attach failure. */
    SK_SESSION_ERROR_LOCK,           /**< Lock acquire/release failure. */
    SK_SESSION_ERROR_LOCK_CONFLICT,  /**< Another client holds the lock. */
    SK_SESSION_ERROR_COMMAND,        /**< tmux command execution failure. */
    SK_SESSION_ERROR_PARSE,          /**< Failed to parse tmux output. */
  } SkSessionErrorCode;

/* ------------------------------------------------------------------ */
/* Constants                                                           */
/* ------------------------------------------------------------------ */

/** Minimum supported tmux version (FR-SESSION-01). */
#define SK_TMUX_MIN_VERSION_MAJOR 3
#define SK_TMUX_MIN_VERSION_MINOR 0

/** Session name delimiter (FR-SESSION-04). */
#define SK_SESSION_NAME_DELIM "--"

/** Lock session prefix (FR-LOCK-02). */
#define SK_LOCK_SESSION_PREFIX "shellkeep-lock-"

/** Default heartbeat interval in seconds (FR-LOCK-09). */
#define SK_LOCK_HEARTBEAT_INTERVAL 30

/** Orphan threshold multiplier (FR-LOCK-07): 2x keepalive timeout. */
#define SK_LOCK_ORPHAN_MULTIPLIER 2

/** Default keepalive timeout for orphan detection (seconds). */
#define SK_LOCK_DEFAULT_KEEPALIVE_TIMEOUT 45

/** Remote history base path (FR-HISTORY-01). */
#define SK_REMOTE_HISTORY_DIR "~/.terminal-state/history"

  /* ------------------------------------------------------------------ */
  /* Tmux version info                                                   */
  /* ------------------------------------------------------------------ */

  /** Parsed tmux version. */
  typedef struct
  {
    int major;
    int minor;
    char *version_string; /**< Raw version string (caller frees). */
  } SkTmuxVersion;

  /* ------------------------------------------------------------------ */
  /* Control mode notification types                                     */
  /* ------------------------------------------------------------------ */

  /** Types of control mode notifications (%begin, %end, %error, etc.). */
  typedef enum
  {
    SK_CTRL_NOTIFICATION_BEGIN,
    SK_CTRL_NOTIFICATION_END,
    SK_CTRL_NOTIFICATION_ERROR,
    SK_CTRL_NOTIFICATION_OUTPUT,
    SK_CTRL_NOTIFICATION_SESSION_CHANGED,
    SK_CTRL_NOTIFICATION_EXIT,
    SK_CTRL_NOTIFICATION_UNKNOWN,
  } SkCtrlNotificationType;

  /** A parsed control mode notification. */
  typedef struct
  {
    SkCtrlNotificationType type;
    int64_t cmd_number; /**< Command number (from %begin/%end/%error). */
    char *data;         /**< Notification payload (caller frees). */
  } SkCtrlNotification;

  /* ------------------------------------------------------------------ */
  /* Lock info (FR-LOCK-03)                                              */
  /* ------------------------------------------------------------------ */

  /** Metadata extracted from a lock session's environment variables. */
  typedef struct
  {
    char *client_id;    /**< SHELLKEEP_LOCK_CLIENT_ID */
    char *hostname;     /**< SHELLKEEP_LOCK_HOSTNAME */
    char *connected_at; /**< SHELLKEEP_LOCK_CONNECTED_AT (ISO 8601) */
    char *pid;          /**< SHELLKEEP_LOCK_PID */
    char *version;      /**< SHELLKEEP_LOCK_VERSION */
    bool valid;         /**< TRUE if all required fields present. */
    bool orphaned;      /**< TRUE if heartbeat expired. */
  } SkLockInfo;

  /* ------------------------------------------------------------------ */
  /* Session info (from list-sessions)                                   */
  /* ------------------------------------------------------------------ */

  /** Information about a single tmux session on the server. */
  typedef struct
  {
    char *name;         /**< Full tmux session name. */
    char *session_uuid; /**< SHELLKEEP_SESSION_UUID env var (may be NULL). */
    char *client_id;    /**< Parsed client-id from name (may be NULL). */
    char *environment;  /**< Parsed environment from name (may be NULL). */
    char *session_name; /**< Parsed session-name part (may be NULL). */
    int num_windows;    /**< Number of windows in the session. */
    bool attached;      /**< Whether any client is currently attached. */
  } SkSessionInfo;

  /* ------------------------------------------------------------------ */
  /* Reconciliation result                                               */
  /* ------------------------------------------------------------------ */

  /** Result of reconciling local state with live tmux sessions. */
  typedef struct
  {
    GPtrArray *alive;    /**< SkSessionInfo* — sessions present both in state and live. */
    GPtrArray *dead;     /**< char* (session_uuid) — UUIDs in state but not live. */
    GPtrArray *orphaned; /**< SkSessionInfo* — live sessions not in state. */
    GPtrArray *renamed;  /**< SkSessionInfo* — sessions whose tmux name diverged. */
  } SkReconcileResult;

  /* ------------------------------------------------------------------ */
  /* Session manager lifecycle                                           */
  /* ------------------------------------------------------------------ */

  /**
   * Create a session manager bound to an SSH connection.
   * @param conn  Connected SSH connection (ownership NOT transferred).
   * @return New session manager, or NULL on failure.
   */
  SkSessionManager *sk_session_manager_new(SkSshConnection *conn);

  /**
   * Free the session manager and all owned tmux session handles.
   */
  void sk_session_manager_free(SkSessionManager *mgr);

  /* ------------------------------------------------------------------ */
  /* Tmux detection (FR-CONN-13..15)                                     */
  /* ------------------------------------------------------------------ */

  /**
   * Check tmux availability and version on the remote server.
   * Runs `tmux -V` via SSH, parses the version string.
   *
   * @param mgr      Session manager.
   * @param version  Output: parsed version (caller frees version_string).
   * @param error    Return location for error.
   * @return TRUE if tmux >= 3.0 is available.
   */
  bool sk_tmux_detect(SkSessionManager *mgr, SkTmuxVersion *version, GError **error);

  /**
   * Parse a tmux version string like "tmux 3.3a" into major.minor.
   *
   * @param version_str  Raw version string from `tmux -V`.
   * @param out_major    Output: major version.
   * @param out_minor    Output: minor version.
   * @return TRUE on success.
   */
  bool sk_tmux_parse_version(const char *version_str, int *out_major, int *out_minor);

  /**
   * Compare a parsed version against the minimum requirement.
   *
   * @param major  Detected major version.
   * @param minor  Detected minor version.
   * @return TRUE if version >= SK_TMUX_MIN_VERSION.
   */
  bool sk_tmux_version_ok(int major, int minor);

/* Legacy compat name for sk_tmux_detect */
#define sk_session_check_tmux(mgr, version_out, error)                                             \
  sk_session_check_tmux_compat((mgr), (version_out), (error))
  bool sk_session_check_tmux_compat(SkSessionManager *mgr, char **version, GError **error);

  /* ------------------------------------------------------------------ */
  /* Control mode (FR-SESSION-02)                                        */
  /* ------------------------------------------------------------------ */

  /**
   * Open a tmux control mode connection (`tmux -CC`).
   * Uses a dedicated SSH channel for orchestration commands.
   *
   * @param mgr    Session manager.
   * @param error  Return location for error.
   * @return TRUE on success.
   */
  bool sk_tmux_control_connect(SkSessionManager *mgr, GError **error);

  /**
   * Close the control mode connection.
   *
   * @param mgr  Session manager.
   */
  void sk_tmux_control_disconnect(SkSessionManager *mgr);

  /**
   * Send a command through the control mode channel and read the response.
   * Blocking — must be called from a worker thread.
   *
   * @param mgr      Session manager.
   * @param command  tmux command string.
   * @param output   Return location for command output (caller frees).
   * @param error    Return location for error.
   * @return TRUE on success.
   */
  bool sk_tmux_control_send(SkSessionManager *mgr, const char *command, char **output,
                            GError **error);

  /**
   * Parse a single line of control mode output into a notification.
   *
   * @param line  A line from the control mode stream.
   * @return Parsed notification (caller frees via sk_ctrl_notification_free).
   */
  SkCtrlNotification *sk_ctrl_parse_notification(const char *line);

  /**
   * Free a control mode notification.
   */
  void sk_ctrl_notification_free(SkCtrlNotification *notif);

  /* ------------------------------------------------------------------ */
  /* Session management (FR-SESSION-01..12)                              */
  /* ------------------------------------------------------------------ */

  /**
   * Build a full tmux session name from components.
   * Format: <client_id>--<environment>--<session_name> (FR-SESSION-04).
   * Caller must g_free().
   */
  char *sk_session_build_name(const char *client_id, const char *environment,
                              const char *session_name);

  /**
   * Parse a full tmux session name into its components.
   * Returns FALSE if the name does not match the expected format.
   */
  bool sk_session_parse_name(const char *full_name, char **out_client_id, char **out_environment,
                             char **out_session_name);

  /**
   * Generate a default session name: session-YYYYMMDD-HHMMSS (FR-SESSION-05).
   * Caller must g_free().
   */
  char *sk_session_generate_name(void);

  /**
   * Create a new tmux session on the server.
   * Sets SHELLKEEP_SESSION_UUID env var inside the session (FR-SESSION-07).
   *
   * @param mgr           Session manager.
   * @param client_id     Client identifier.
   * @param environment   Environment name.
   * @param session_name  Session name (or NULL for auto-generated).
   * @param cols          Initial terminal width.
   * @param rows          Initial terminal height.
   * @param error         Return location for error.
   * @return New tmux session handle, or NULL on failure.
   */
  SkTmuxSession *sk_session_create(SkSessionManager *mgr, const char *client_id,
                                   const char *environment, const char *session_name, int cols,
                                   int rows, GError **error);

  /**
   * Attach to an existing tmux session.
   *
   * @param mgr           Session manager.
   * @param session_name  Full tmux session name to attach to.
   * @param error         Return location for error.
   * @return Tmux session handle, or NULL on failure.
   */
  SkTmuxSession *sk_session_attach(SkSessionManager *mgr, const char *session_name, GError **error);

  /**
   * List tmux sessions matching a prefix (client_id + environment).
   * If client_id is NULL, lists all sessions.
   *
   * @param mgr          Session manager.
   * @param client_id    Filter by client ID (NULL = all).
   * @param environment  Filter by environment (NULL = all).
   * @param error        Return location for error.
   * @return GPtrArray of SkSessionInfo* (caller owns array and elements).
   */
  GPtrArray *sk_session_list(SkSessionManager *mgr, const char *client_id, const char *environment,
                             GError **error);

  /**
   * Check whether a tmux session exists on the server.
   *
   * @param mgr   Session manager.
   * @param name  Full tmux session name.
   * @return TRUE if the session exists.
   */
  bool sk_session_exists(SkSessionManager *mgr, const char *name);

  /**
   * Kill a tmux session on the server.
   *
   * @param mgr    Session manager.
   * @param name   Full tmux session name.
   * @param error  Return location for error.
   * @return TRUE on success.
   */
  bool sk_session_kill_by_name(SkSessionManager *mgr, const char *name, GError **error);

  /**
   * Rename a tmux session on the server (FR-SESSION-06).
   *
   * @param mgr       Session manager.
   * @param old_name  Current full tmux session name.
   * @param new_name  New full tmux session name.
   * @param error     Return location for error.
   * @return TRUE on success.
   */
  bool sk_session_rename(SkSessionManager *mgr, const char *old_name, const char *new_name,
                         GError **error);

  /* ------------------------------------------------------------------ */
  /* SkTmuxSession handle                                                */
  /* ------------------------------------------------------------------ */

  /**
   * Detach from a tmux session (session continues running on server).
   * FR-SESSION-10: Never kills the session.
   */
  bool sk_session_detach(SkTmuxSession *session, GError **error);

  /**
   * Kill a tmux session on the server (via handle).
   */
  bool sk_session_kill(SkTmuxSession *session, GError **error);

  /**
   * Get the full tmux session name.
   * @return Internal string — do NOT free.
   */
  const char *sk_tmux_session_get_name(const SkTmuxSession *session);

  /**
   * Get the session UUID.
   * @return Internal string — do NOT free. May be NULL.
   */
  const char *sk_tmux_session_get_uuid(const SkTmuxSession *session);

  /**
   * Free a tmux session handle (does NOT kill the remote session).
   */
  void sk_tmux_session_free(SkTmuxSession *session);

  /**
   * Free a session info struct.
   */
  void sk_session_info_free(SkSessionInfo *info);

  /* ------------------------------------------------------------------ */
  /* Lock mechanism (FR-LOCK-*)                                          */
  /* ------------------------------------------------------------------ */

  /**
   * Acquire the client-id lock on the server.
   * Creates tmux session `shellkeep-lock-<client_id>` with env vars.
   * Atomic: tmux new-session fails if the session already exists.
   *
   * @param mgr          Session manager.
   * @param client_id    Client identifier.
   * @param hostname     Local hostname.
   * @param error        Return location for error.
   * @return TRUE on success, FALSE if lock exists (SK_SESSION_ERROR_LOCK_CONFLICT).
   */
  bool sk_lock_acquire(SkSessionManager *mgr, const char *client_id, const char *hostname,
                       GError **error);

  /**
   * Release the client-id lock on the server.
   * Kills the lock session. Should be the last operation before disconnect (FR-LOCK-10).
   *
   * @param mgr        Session manager.
   * @param client_id  Client identifier.
   * @param error      Return location for error.
   * @return TRUE on success.
   */
  bool sk_lock_release(SkSessionManager *mgr, const char *client_id, GError **error);

  /**
   * Check the lock status and retrieve metadata (FR-LOCK-03..04).
   *
   * @param mgr        Session manager.
   * @param client_id  Client identifier.
   * @param error      Return location for error.
   * @return Lock info (caller frees via sk_lock_info_free), or NULL if no lock.
   */
  SkLockInfo *sk_lock_check(SkSessionManager *mgr, const char *client_id, GError **error);

  /**
   * Update the heartbeat timestamp in the lock session (FR-LOCK-09).
   *
   * @param mgr        Session manager.
   * @param client_id  Client identifier.
   * @param error      Return location for error.
   * @return TRUE on success.
   */
  bool sk_lock_update_heartbeat(SkSessionManager *mgr, const char *client_id, GError **error);

  /**
   * Check if a lock is orphaned (FR-LOCK-07).
   * Compares SHELLKEEP_LOCK_CONNECTED_AT with current time.
   *
   * @param info             Lock info to check.
   * @param keepalive_timeout  Keepalive timeout in seconds.
   * @return TRUE if the lock is orphaned.
   */
  bool sk_lock_is_orphaned(const SkLockInfo *info, int keepalive_timeout);

  /**
   * Check if the lock belongs to this process (FR-LOCK-06).
   *
   * @param info      Lock info.
   * @param hostname  Current hostname.
   * @param pid       Current process PID as string.
   * @return TRUE if the lock is ours (same hostname + PID).
   */
  bool sk_lock_is_own(const SkLockInfo *info, const char *hostname, const char *pid);

  /**
   * Free a lock info struct.
   */
  void sk_lock_info_free(SkLockInfo *info);

  /* ------------------------------------------------------------------ */
  /* History via pipe-pane (FR-HISTORY-01..03)                           */
  /* ------------------------------------------------------------------ */

  /**
   * Enable history capture for a session via `tmux pipe-pane`.
   * Pipes output to ~/.terminal-state/history/<uuid>.raw on the server.
   *
   * @param mgr           Session manager.
   * @param session_name  Full tmux session name.
   * @param session_uuid  Session UUID for the history filename.
   * @param error         Return location for error.
   * @return TRUE on success.
   */
  bool sk_session_enable_history(SkSessionManager *mgr, const char *session_name,
                                 const char *session_uuid, GError **error);

  /**
   * Disable history capture for a session.
   *
   * @param mgr           Session manager.
   * @param session_name  Full tmux session name.
   * @param error         Return location for error.
   * @return TRUE on success.
   */
  bool sk_session_disable_history(SkSessionManager *mgr, const char *session_name, GError **error);

  /* ------------------------------------------------------------------ */
  /* Reconciliation (FR-SESSION-07..08)                                  */
  /* ------------------------------------------------------------------ */

  /**
   * Reconcile local state with live tmux sessions.
   * Uses session UUIDs as the primary identifier.
   * Detects dead sessions, renamed sessions, and orphans.
   *
   * @param mgr            Session manager.
   * @param state_sessions GPtrArray of SkSessionInfo* from local state.
   * @param client_id      Client identifier for filtering.
   * @param environment    Environment name for filtering.
   * @param error          Return location for error.
   * @return Reconciliation result (caller frees via sk_reconcile_result_free).
   */
  SkReconcileResult *sk_session_reconcile(SkSessionManager *mgr, GPtrArray *state_sessions,
                                          const char *client_id, const char *environment,
                                          GError **error);

  /**
   * Free a reconciliation result.
   */
  void sk_reconcile_result_free(SkReconcileResult *result);

#ifdef __cplusplus
}
#endif

#endif /* SK_SESSION_H */
