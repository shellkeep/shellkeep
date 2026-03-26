// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_session_internal.h
 * @brief Internal declarations shared across src/session/ source files.
 *
 * NOT part of the public API. Never install this header.
 */

#ifndef SK_SESSION_INTERNAL_H
#define SK_SESSION_INTERNAL_H

#include "shellkeep/sk_session.h"
#include "shellkeep/sk_ssh.h"

G_BEGIN_DECLS

/* ------------------------------------------------------------------ */
/* Session manager internal structure                                  */
/* ------------------------------------------------------------------ */

struct _SkSessionManager
{
  SkSshConnection *conn;      /**< Borrowed SSH connection. */
  SkSshChannel *ctrl_channel; /**< Control mode channel (NULL if not open). */
  int64_t ctrl_cmd_seq;       /**< Next control mode command sequence. */
  GMutex lock;                /**< Protects internal state. */
};

/* ------------------------------------------------------------------ */
/* Tmux session handle internal structure                              */
/* ------------------------------------------------------------------ */

struct _SkTmuxSession
{
  SkSessionManager *mgr; /**< Borrowed reference to manager. */
  char *name;            /**< Full tmux session name. */
  char *session_uuid;    /**< UUID v4 (may be NULL). */
};

/* ------------------------------------------------------------------ */
/* Internal helpers                                                    */
/* ------------------------------------------------------------------ */

/**
 * Execute a tmux command via the SSH connection and return stdout.
 * Uses a fresh exec channel (not control mode).
 * Blocking — must be called from a worker thread.
 *
 * @param conn     SSH connection.
 * @param command  Shell command to execute.
 * @param output   Return location for stdout (caller frees). May be NULL.
 * @param error    Return location for error.
 * @return Exit status of the command, or -1 on channel error.
 */
int sk_session_exec_command(SkSshConnection *conn, const char *command, char **output,
                            GError **error);

/**
 * Build the lock session name for a client ID.
 * Returns: "shellkeep-lock-<client_id>" (caller frees).
 */
char *sk_lock_session_name(const char *client_id);

/**
 * Get the current time as ISO 8601 string.
 * Caller must g_free().
 */
char *sk_iso8601_now(void);

/**
 * Parse an ISO 8601 timestamp to time_t.
 * Returns (time_t)-1 on failure.
 */
time_t sk_iso8601_parse(const char *timestamp);

/**
 * Shell-safe quote a string for use in single-quoted shell arguments.
 * Replaces ' with '\'' to prevent shell injection (NFR-SEC-07).
 * Caller must g_free().
 */
char *sk_shell_quote(const char *str);

/**
 * Validate a session or environment name per NFR-SEC-05.
 * Rejects: colon (:), dot (.), forward slash (/), backslash (\),
 * double-dot (..), and null bytes (\0).
 * Accepts Unicode but rejects tmux-incompatible and path-traversal chars.
 * Returns TRUE if the name is safe.
 */
bool sk_validate_user_name(const char *name);

/**
 * Validate a UUID string format: only [a-f0-9-] allowed (NFR-SEC-06).
 * Returns TRUE if valid.
 */
bool sk_validate_uuid_format(const char *uuid);

G_END_DECLS

#endif /* SK_SESSION_INTERNAL_H */
