// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_session_ops.c
 * @brief Session management operations: create, attach, list, kill, rename.
 *
 * Implements FR-SESSION-01..12: session naming, UUID assignment, listing
 * by prefix, existence checks, and rename.
 */

#include "sk_session_internal.h"
#include <stdio.h>
#include <string.h>
#include <time.h>

/* ------------------------------------------------------------------ */
/* Session name utilities (FR-SESSION-04)                              */
/* ------------------------------------------------------------------ */

char *
sk_session_build_name(const char *client_id, const char *environment, const char *session_name)
{
  g_return_val_if_fail(client_id != NULL, NULL);
  g_return_val_if_fail(environment != NULL, NULL);
  g_return_val_if_fail(session_name != NULL, NULL);

  /* FR-SESSION-04: <client-id>--<environment>--<session-name> */
  return g_strdup_printf("%s%s%s%s%s", client_id, SK_SESSION_NAME_DELIM, environment,
                         SK_SESSION_NAME_DELIM, session_name);
}

bool
sk_session_parse_name(const char *full_name, char **out_client_id, char **out_environment,
                      char **out_session_name)
{
  g_return_val_if_fail(full_name != NULL, false);

  /* Find the first "--" delimiter. */
  const char *first_delim = strstr(full_name, SK_SESSION_NAME_DELIM);
  if (first_delim == NULL)
  {
    return false;
  }

  /* Find the second "--" delimiter. */
  const char *after_first = first_delim + strlen(SK_SESSION_NAME_DELIM);
  const char *second_delim = strstr(after_first, SK_SESSION_NAME_DELIM);
  if (second_delim == NULL)
  {
    return false;
  }

  const char *after_second = second_delim + strlen(SK_SESSION_NAME_DELIM);

  if (out_client_id != NULL)
  {
    *out_client_id = g_strndup(full_name, (gsize)(first_delim - full_name));
  }
  if (out_environment != NULL)
  {
    *out_environment = g_strndup(after_first, (gsize)(second_delim - after_first));
  }
  if (out_session_name != NULL)
  {
    *out_session_name = g_strdup(after_second);
  }

  return true;
}

/* FR-SESSION-05 */
char *
sk_session_generate_name(void)
{
  GDateTime *now = g_date_time_new_now_local();
  char *name = g_date_time_format(now, "session-%Y%m%d-%H%M%S");
  g_date_time_unref(now);
  return name;
}

/* ------------------------------------------------------------------ */
/* UUID generation                                                     */
/* ------------------------------------------------------------------ */

static char *
generate_uuid_v4(void)
{
  return g_uuid_string_random();
}

/* ------------------------------------------------------------------ */
/* Session info parsing                                                */
/* ------------------------------------------------------------------ */

/**
 * Parse a single line from `tmux list-sessions -F '...'` into SkSessionInfo.
 * Expected format: <name>|<num_windows>|<attached_flag>
 */
static SkSessionInfo *
parse_session_line(const char *line)
{
  if (line == NULL || line[0] == '\0')
  {
    return NULL;
  }

  gchar **parts = g_strsplit(line, "|", 3);
  int n = 0;
  while (parts[n] != NULL)
    n++;

  if (n < 1)
  {
    g_strfreev(parts);
    return NULL;
  }

  SkSessionInfo *info = g_new0(SkSessionInfo, 1);
  info->name = g_strdup(parts[0]);

  if (n >= 2)
  {
    info->num_windows = atoi(parts[1]);
  }
  if (n >= 3)
  {
    info->attached = (strcmp(parts[2], "1") == 0);
  }

  /* Try to parse the structured name. */
  sk_session_parse_name(info->name, &info->client_id, &info->environment, &info->session_name);

  g_strfreev(parts);
  return info;
}

void
sk_session_info_free(SkSessionInfo *info)
{
  if (info == NULL)
    return;
  g_free(info->name);
  g_free(info->session_uuid);
  g_free(info->client_id);
  g_free(info->environment);
  g_free(info->session_name);
  g_free(info);
}

/* ------------------------------------------------------------------ */
/* Session create (FR-SESSION-01, FR-SESSION-04, FR-SESSION-07)        */
/* ------------------------------------------------------------------ */

SkTmuxSession *
sk_session_create(SkSessionManager *mgr, const char *client_id, const char *environment,
                  const char *session_name, int cols, int rows, GError **error)
{
  g_return_val_if_fail(mgr != NULL, NULL);
  g_return_val_if_fail(client_id != NULL, NULL);
  g_return_val_if_fail(environment != NULL, NULL);
  g_return_val_if_fail(cols > 0 && rows > 0, NULL);

  /* FR-SESSION-05: auto-generate name if not provided. */
  g_autofree char *auto_name = NULL;
  if (session_name == NULL || session_name[0] == '\0')
  {
    auto_name = sk_session_generate_name();
    session_name = auto_name;
  }

  /* FR-SESSION-04: build full session name. */
  g_autofree char *full_name = sk_session_build_name(client_id, environment, session_name);

  /* FR-SESSION-07: generate UUID. */
  g_autofree char *uuid = generate_uuid_v4();

  /* NFR-SEC-05: validate session_name and environment before use. */
  if (!sk_validate_user_name(environment))
  {
    g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_SESSION,
                "Invalid environment name: contains prohibited characters");
    return NULL;
  }
  if (!sk_validate_user_name(session_name))
  {
    g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_SESSION,
                "Invalid session name: contains prohibited characters");
    return NULL;
  }

  /* Create the tmux session with a detached shell and size.
   * -d = detach immediately, -x/-y = initial size.
   * Also set SHELLKEEP_SESSION_UUID as an env var inside the session.
   * NFR-SEC-07: shell-safe quoting to prevent injection. */
  g_autofree char *q_name = sk_shell_quote(full_name);
  g_autofree char *q_uuid = sk_shell_quote(uuid);
  g_autofree char *cmd = g_strdup_printf("tmux new-session -d -s %s -x %d -y %d "
                                         "\\; set-environment -t %s SHELLKEEP_SESSION_UUID %s",
                                         q_name, cols, rows, q_name, q_uuid);

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_SESSION,
                  "Failed to create tmux session '%s' (exit %d)", full_name, rc);
    }
    return NULL;
  }

  /* Build the session handle. */
  SkTmuxSession *session = g_new0(SkTmuxSession, 1);
  session->mgr = mgr;
  session->name = g_strdup(full_name);
  session->session_uuid = g_strdup(uuid);

  return session;
}

/* ------------------------------------------------------------------ */
/* Session attach (FR-SESSION-03)                                      */
/* ------------------------------------------------------------------ */

SkTmuxSession *
sk_session_attach(SkSessionManager *mgr, const char *session_name, GError **error)
{
  g_return_val_if_fail(mgr != NULL, NULL);
  g_return_val_if_fail(session_name != NULL, NULL);

  /* FR-SESSION-03: verify session exists before returning handle.
   * Actual PTY attach happens when the UI creates the SSH channel
   * with `tmux attach-session -t <name>`.
   * NFR-SEC-07: shell-safe quoting. */
  g_autofree char *q_name = sk_shell_quote(session_name);
  g_autofree char *cmd = g_strdup_printf("tmux has-session -t %s", q_name);

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_SESSION,
                  "tmux session '%s' does not exist", session_name);
    }
    return NULL;
  }

  /* Retrieve the session UUID from the tmux environment. NFR-SEC-07. */
  g_autofree char *env_cmd = g_strdup_printf(
      "tmux show-environment -t %s SHELLKEEP_SESSION_UUID 2>/dev/null", q_name);

  char *env_output = NULL;
  sk_session_exec_command(mgr->conn, env_cmd, &env_output, NULL);

  char *uuid = NULL;
  if (env_output != NULL)
  {
    /* Format: SHELLKEEP_SESSION_UUID=<value> */
    g_strstrip(env_output);
    const char *eq = strchr(env_output, '=');
    if (eq != NULL)
    {
      uuid = g_strdup(eq + 1);
    }
    g_free(env_output);
  }

  SkTmuxSession *session = g_new0(SkTmuxSession, 1);
  session->mgr = mgr;
  session->name = g_strdup(session_name);
  session->session_uuid = uuid;

  return session;
}

/* ------------------------------------------------------------------ */
/* Session list (FR-SESSION-01)                                        */
/* ------------------------------------------------------------------ */

GPtrArray *
sk_session_list(SkSessionManager *mgr, const char *client_id, const char *environment,
                GError **error)
{
  g_return_val_if_fail(mgr != NULL, NULL);

  /* List all sessions with a parseable format. */
  const char *cmd =
      "tmux list-sessions -F '#{session_name}|#{session_windows}|#{session_attached}' 2>/dev/null";

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);

  GPtrArray *sessions = g_ptr_array_new_with_free_func((GDestroyNotify)sk_session_info_free);

  if (rc != 0 || output == NULL)
  {
    /* No sessions or tmux not running — return empty list. */
    g_free(output);
    return sessions;
  }

  /* Build prefix filter if client_id/environment given. */
  g_autofree char *prefix = NULL;
  if (client_id != NULL && environment != NULL)
  {
    prefix = g_strdup_printf("%s%s%s%s", client_id, SK_SESSION_NAME_DELIM, environment,
                             SK_SESSION_NAME_DELIM);
  }
  else if (client_id != NULL)
  {
    prefix = g_strdup_printf("%s%s", client_id, SK_SESSION_NAME_DELIM);
  }

  gchar **lines = g_strsplit(output, "\n", -1);
  g_free(output);

  for (int i = 0; lines[i] != NULL; i++)
  {
    g_strstrip(lines[i]);
    if (lines[i][0] == '\0')
      continue;

    /* Apply prefix filter. */
    if (prefix != NULL && !g_str_has_prefix(lines[i], prefix))
    {
      continue;
    }

    SkSessionInfo *info = parse_session_line(lines[i]);
    if (info != NULL)
    {
      /* Retrieve UUID for each matching session. NFR-SEC-07. */
      g_autofree char *q_sname = sk_shell_quote(info->name);
      g_autofree char *env_cmd = g_strdup_printf(
          "tmux show-environment -t %s SHELLKEEP_SESSION_UUID 2>/dev/null", q_sname);
      char *env_out = NULL;
      sk_session_exec_command(mgr->conn, env_cmd, &env_out, NULL);
      if (env_out != NULL)
      {
        g_strstrip(env_out);
        const char *eq = strchr(env_out, '=');
        if (eq != NULL)
        {
          info->session_uuid = g_strdup(eq + 1);
        }
        g_free(env_out);
      }

      g_ptr_array_add(sessions, info);
    }
  }

  g_strfreev(lines);
  return sessions;
}

/* ------------------------------------------------------------------ */
/* Session exists                                                      */
/* ------------------------------------------------------------------ */

bool
sk_session_exists(SkSessionManager *mgr, const char *name)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(name != NULL, false);

  g_autofree char *q_name = sk_shell_quote(name);
  g_autofree char *cmd = g_strdup_printf("tmux has-session -t %s 2>/dev/null", q_name);
  int rc = sk_session_exec_command(mgr->conn, cmd, NULL, NULL);
  return (rc == 0);
}

/* ------------------------------------------------------------------ */
/* Session kill                                                        */
/* ------------------------------------------------------------------ */

bool
sk_session_kill_by_name(SkSessionManager *mgr, const char *name, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(name != NULL, false);

  g_autofree char *q_name = sk_shell_quote(name);
  g_autofree char *cmd = g_strdup_printf("tmux kill-session -t %s", q_name);
  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_SESSION,
                  "Failed to kill tmux session '%s'", name);
    }
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Session rename (FR-SESSION-06)                                      */
/* ------------------------------------------------------------------ */

bool
sk_session_rename(SkSessionManager *mgr, const char *old_name, const char *new_name, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(old_name != NULL, false);
  g_return_val_if_fail(new_name != NULL, false);

  g_autofree char *q_old = sk_shell_quote(old_name);
  g_autofree char *q_new = sk_shell_quote(new_name);
  g_autofree char *cmd = g_strdup_printf("tmux rename-session -t %s %s", q_old, q_new);
  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_SESSION,
                  "Failed to rename tmux session '%s' to '%s'", old_name, new_name);
    }
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* SkTmuxSession handle operations                                     */
/* ------------------------------------------------------------------ */

bool
sk_session_detach(SkTmuxSession *session, GError **error)
{
  g_return_val_if_fail(session != NULL, false);
  /* FR-SESSION-10: detach never kills the session. */
  (void)error;
  /* Detach is a client-side operation — just release the handle.
   * The actual SSH channel is managed by the terminal layer. */
  return true;
}

bool
sk_session_kill(SkTmuxSession *session, GError **error)
{
  g_return_val_if_fail(session != NULL, false);
  g_return_val_if_fail(session->mgr != NULL, false);

  return sk_session_kill_by_name(session->mgr, session->name, error);
}

const char *
sk_tmux_session_get_name(const SkTmuxSession *session)
{
  g_return_val_if_fail(session != NULL, NULL);
  return session->name;
}

const char *
sk_tmux_session_get_uuid(const SkTmuxSession *session)
{
  g_return_val_if_fail(session != NULL, NULL);
  return session->session_uuid;
}

void
sk_tmux_session_free(SkTmuxSession *session)
{
  if (session == NULL)
    return;
  g_free(session->name);
  g_free(session->session_uuid);
  g_free(session);
}
