// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_session_history.c
 * @brief History capture via tmux pipe-pane.
 *
 * Implements FR-HISTORY-01..03:
 * - Enable: `tmux pipe-pane -t <session> "cat >> ~/.terminal-state/history/<uuid>.raw"`
 * - Disable: `tmux pipe-pane -t <session>` (no argument = stop piping)
 *
 * The raw file accumulates on the server even while the client is disconnected.
 * Conversion to structured JSONL is handled by the state layer on reconnect.
 */

#include "sk_session_internal.h"
#include <string.h>

/* ------------------------------------------------------------------ */
/* Enable history (FR-HISTORY-01)                                      */
/* ------------------------------------------------------------------ */

bool
sk_session_enable_history(SkSessionManager *mgr, const char *session_name, const char *session_uuid,
                          GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(session_name != NULL, false);
  g_return_val_if_fail(session_uuid != NULL, false);

  /* NFR-SEC-06: validate UUID format before interpolating into file path. */
  if (!sk_validate_uuid_format(session_uuid))
  {
    g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_COMMAND,
                "Invalid session UUID format: '%s'", session_uuid);
    return false;
  }

  /* FR-HISTORY-01: ensure the history directory exists on the server. */
  g_autofree char *mkdir_cmd =
      g_strdup_printf("mkdir -p %s && chmod 700 %s", SK_REMOTE_HISTORY_DIR, SK_REMOTE_HISTORY_DIR);

  int rc = sk_session_exec_command(mgr->conn, mkdir_cmd, NULL, NULL);
  if (rc != 0)
  {
    g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_COMMAND,
                "Failed to create history directory %s on server", SK_REMOTE_HISTORY_DIR);
    return false;
  }

  /* FR-HISTORY-01: pipe session output to raw file.
   * Using 'cat >>' for append-only writes.
   * NFR-SEC-07: shell-safe quoting for session_name. UUID is validated above. */
  g_autofree char *q_session = sk_shell_quote(session_name);
  g_autofree char *cmd = g_strdup_printf("tmux pipe-pane -t %s 'cat >> %s/%s.raw'", q_session,
                                         SK_REMOTE_HISTORY_DIR, session_uuid);

  char *output = NULL;
  rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_COMMAND,
                  "Failed to enable history pipe-pane for session '%s'", session_name);
    }
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Disable history                                                     */
/* ------------------------------------------------------------------ */

bool
sk_session_disable_history(SkSessionManager *mgr, const char *session_name, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(session_name != NULL, false);

  /* Calling pipe-pane without a command argument stops the pipe.
   * NFR-SEC-07: shell-safe quoting. */
  g_autofree char *q_session = sk_shell_quote(session_name);
  g_autofree char *cmd = g_strdup_printf("tmux pipe-pane -t %s", q_session);

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_COMMAND,
                  "Failed to disable history pipe-pane for session '%s'", session_name);
    }
    return false;
  }

  return true;
}
