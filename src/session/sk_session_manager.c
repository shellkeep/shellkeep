// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_session_manager.c
 * @brief Session manager lifecycle and command execution helpers.
 */

#include "sk_session_internal.h"
#include <string.h>
#include <time.h>

/* ------------------------------------------------------------------ */
/* Error quark                                                         */
/* ------------------------------------------------------------------ */

G_DEFINE_QUARK(sk - session - error - quark, sk_session_error)

/* ------------------------------------------------------------------ */
/* Session manager lifecycle                                           */
/* ------------------------------------------------------------------ */

SkSessionManager *
sk_session_manager_new(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, NULL);

  SkSessionManager *mgr = g_new0(SkSessionManager, 1);
  mgr->conn = conn;
  mgr->ctrl_channel = NULL;
  mgr->ctrl_cmd_seq = 0;
  g_mutex_init(&mgr->lock);

  return mgr;
}

void
sk_session_manager_free(SkSessionManager *mgr)
{
  if (mgr == NULL)
    return;

  sk_tmux_control_disconnect(mgr);

  g_mutex_clear(&mgr->lock);
  g_free(mgr);
}

/* ------------------------------------------------------------------ */
/* Command execution via SSH exec channel                              */
/* ------------------------------------------------------------------ */

/**
 * Execute a command on the remote server via a new SSH exec channel.
 * Reads all stdout output into a single string.
 *
 * @return Exit status (0 = success), or -1 on channel error.
 */
int
sk_session_exec_command(SkSshConnection *conn, const char *command, char **output, GError **error)
{
  g_return_val_if_fail(conn != NULL, -1);
  g_return_val_if_fail(command != NULL, -1);

  if (output != NULL)
  {
    *output = NULL;
  }

  /* Open a fresh exec channel. Use 80x24 as default size — irrelevant
   * for command execution but required by sk_ssh_channel_open. */
  SkSshChannel *channel = sk_ssh_channel_open(conn, 80, 24, error);
  if (channel == NULL)
  {
    return -1;
  }

  if (!sk_ssh_channel_exec(channel, command, error))
  {
    sk_ssh_channel_free(channel);
    return -1;
  }

  /* Read all output. */
  GString *buf = g_string_new(NULL);
  char read_buf[4096];

  for (;;)
  {
    int n = sk_ssh_channel_read_nonblocking(channel, read_buf, sizeof(read_buf));
    if (n > 0)
    {
      g_string_append_len(buf, read_buf, n);
    }
    else if (n == 0)
    {
      /* No data available — check if channel is still open. */
      if (!sk_ssh_channel_is_open(channel))
      {
        break;
      }
      /* Brief yield to avoid busy-looping. In a real async
       * implementation this would use g_io_add_watch; for the
       * blocking helper we do a small sleep. */
      g_usleep(1000); /* 1ms */
    }
    else
    {
      /* EOF or error. */
      break;
    }
  }

  int exit_status = sk_ssh_channel_get_exit_status(channel);
  sk_ssh_channel_free(channel);

  if (output != NULL)
  {
    *output = g_string_free(buf, FALSE);
  }
  else
  {
    g_string_free(buf, TRUE);
  }

  return exit_status;
}

/* ------------------------------------------------------------------ */
/* Utility: ISO 8601 timestamp                                         */
/* ------------------------------------------------------------------ */

char *
sk_iso8601_now(void)
{
  GDateTime *dt = g_date_time_new_now_utc();
  char *str = g_date_time_format_iso8601(dt);
  g_date_time_unref(dt);
  return str;
}

time_t
sk_iso8601_parse(const char *timestamp)
{
  if (timestamp == NULL || timestamp[0] == '\0')
  {
    return (time_t)-1;
  }

  GDateTime *dt = g_date_time_new_from_iso8601(timestamp, NULL);
  if (dt == NULL)
  {
    return (time_t)-1;
  }

  int64_t unix_time = g_date_time_to_unix(dt);
  g_date_time_unref(dt);

  return (time_t)unix_time;
}

/* ------------------------------------------------------------------ */
/* Utility: lock session name                                          */
/* ------------------------------------------------------------------ */

char *
sk_lock_session_name(const char *client_id)
{
  g_return_val_if_fail(client_id != NULL, NULL);
  return g_strdup_printf("%s%s", SK_LOCK_SESSION_PREFIX, client_id);
}

/* ------------------------------------------------------------------ */
/* Shell-safe quoting — NFR-SEC-07                                     */
/* ------------------------------------------------------------------ */

char *
sk_shell_quote(const char *str)
{
  if (str == NULL)
    return g_strdup("''");

  /* Replace each single quote with '\'' (end quote, escaped quote, start quote).
   * This is the standard POSIX way to safely embed arbitrary strings
   * inside single-quoted shell arguments. */
  GString *result = g_string_new("'");
  for (const char *p = str; *p != '\0'; p++)
  {
    if (*p == '\'')
    {
      g_string_append(result, "'\\''");
    }
    else
    {
      g_string_append_c(result, *p);
    }
  }
  g_string_append_c(result, '\'');
  return g_string_free(result, FALSE);
}

/* ------------------------------------------------------------------ */
/* Name validation — NFR-SEC-05                                        */
/* ------------------------------------------------------------------ */

bool
sk_validate_user_name(const char *name)
{
  if (name == NULL || name[0] == '\0')
    return false;

  /* NFR-SEC-05: reject path traversal and tmux-incompatible chars. */
  for (const char *p = name; *p != '\0'; p++)
  {
    unsigned char c = (unsigned char)*p;

    /* Reject null bytes (should not occur but be defensive). */
    if (c == '\0')
      return false;

    /* Reject colon and dot (tmux-incompatible per NFR-SEC-05). */
    if (c == ':' || c == '.')
      return false;

    /* Reject path separators (path traversal). */
    if (c == '/' || c == '\\')
      return false;

    /* Reject control characters. */
    if (c < 0x20 || c == 0x7F)
      return false;
  }

  /* Reject ".." as a name component (double-dot path traversal). */
  if (strstr(name, "..") != NULL)
    return false;

  return true;
}

/* ------------------------------------------------------------------ */
/* UUID format validation — NFR-SEC-06                                 */
/* ------------------------------------------------------------------ */

bool
sk_validate_uuid_format(const char *uuid)
{
  if (uuid == NULL || uuid[0] == '\0')
    return false;

  for (const char *p = uuid; *p != '\0'; p++)
  {
    char c = *p;
    if (!((c >= 'a' && c <= 'f') || (c >= '0' && c <= '9') || c == '-'))
      return false;
  }
  return true;
}
