// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_tmux_control.c
 * @brief tmux control mode (tmux -CC) connection and command dispatch.
 *
 * FR-SESSION-02: control mode is used ONLY for orchestration
 * (list, create, destroy sessions, detect death, manage lock env vars).
 * Never for data streaming.
 *
 * Control mode protocol notifications:
 *   %begin <time> <cmd-number> <flags>
 *   %end <time> <cmd-number> <flags>
 *   %error <time> <cmd-number> <flags>
 *   %output <pane-id> <data>
 *   %session-changed <session-id> <session-name>
 *   %exit [reason]
 */

#include "sk_session_internal.h"
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/* Control mode lifecycle                                              */
/* ------------------------------------------------------------------ */

/* FR-SESSION-02 */
bool
sk_tmux_control_connect(SkSessionManager *mgr, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);

  g_mutex_lock(&mgr->lock);
  if (mgr->ctrl_channel != NULL)
  {
    g_mutex_unlock(&mgr->lock);
    return true; /* Already connected. */
  }
  g_mutex_unlock(&mgr->lock);

  /* Open a dedicated SSH channel for control mode. */
  SkSshChannel *ch = sk_ssh_channel_open(mgr->conn, 80, 24, error);
  if (ch == NULL)
  {
    return false;
  }

  /* Start tmux in control mode. -CC avoids terminal output decoration. */
  if (!sk_ssh_channel_exec(ch, "tmux -CC", error))
  {
    sk_ssh_channel_free(ch);
    return false;
  }

  g_mutex_lock(&mgr->lock);
  mgr->ctrl_channel = ch;
  mgr->ctrl_cmd_seq = 0;
  g_mutex_unlock(&mgr->lock);

  return true;
}

void
sk_tmux_control_disconnect(SkSessionManager *mgr)
{
  if (mgr == NULL)
    return;

  g_mutex_lock(&mgr->lock);
  SkSshChannel *ch = mgr->ctrl_channel;
  mgr->ctrl_channel = NULL;
  g_mutex_unlock(&mgr->lock);

  if (ch != NULL)
  {
    sk_ssh_channel_free(ch);
  }
}

/* ------------------------------------------------------------------ */
/* Send command and read response                                      */
/* ------------------------------------------------------------------ */

/**
 * Read a line from the control mode channel (blocking).
 * Returns a newly-allocated string, or NULL on EOF/error.
 */
static char *
ctrl_read_line(SkSshChannel *ch)
{
  GString *line = g_string_new(NULL);
  char c;

  for (;;)
  {
    int n = sk_ssh_channel_read_nonblocking(ch, &c, 1);
    if (n == 1)
    {
      if (c == '\n')
      {
        break;
      }
      g_string_append_c(line, c);
    }
    else if (n == 0)
    {
      if (!sk_ssh_channel_is_open(ch))
      {
        if (line->len == 0)
        {
          g_string_free(line, TRUE);
          return NULL;
        }
        break;
      }
      g_usleep(500); /* 0.5ms yield */
    }
    else
    {
      /* EOF or error. */
      if (line->len == 0)
      {
        g_string_free(line, TRUE);
        return NULL;
      }
      break;
    }
  }

  return g_string_free(line, FALSE);
}

/* FR-SESSION-02 */
bool
sk_tmux_control_send(SkSessionManager *mgr, const char *command, char **output, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(command != NULL, false);

  if (output != NULL)
  {
    *output = NULL;
  }

  g_mutex_lock(&mgr->lock);
  SkSshChannel *ch = mgr->ctrl_channel;
  g_mutex_unlock(&mgr->lock);

  if (ch == NULL)
  {
    g_set_error_literal(error, SK_SESSION_ERROR, SK_SESSION_ERROR_CONTROL_MODE,
                        "Control mode not connected");
    return false;
  }

  /* Send command followed by newline. */
  g_autofree char *cmd_line = g_strdup_printf("%s\n", command);
  int written = sk_ssh_channel_write(ch, cmd_line, strlen(cmd_line));
  if (written < 0)
  {
    g_set_error_literal(error, SK_SESSION_ERROR, SK_SESSION_ERROR_CONTROL_MODE,
                        "Failed to write to control mode channel");
    return false;
  }

  /* Read lines until we see %end or %error for our command.
   * Collect output lines between %begin and %end. */
  GString *result = g_string_new(NULL);
  bool in_output = false;
  bool success = false;

  for (;;)
  {
    char *line = ctrl_read_line(ch);
    if (line == NULL)
    {
      g_set_error_literal(error, SK_SESSION_ERROR, SK_SESSION_ERROR_CONTROL_MODE,
                          "Control mode channel closed unexpectedly");
      g_string_free(result, TRUE);
      return false;
    }

    SkCtrlNotification *notif = sk_ctrl_parse_notification(line);

    if (notif != NULL)
    {
      switch (notif->type)
      {
      case SK_CTRL_NOTIFICATION_BEGIN:
        in_output = true;
        break;

      case SK_CTRL_NOTIFICATION_END:
        success = true;
        sk_ctrl_notification_free(notif);
        g_free(line);
        goto done;

      case SK_CTRL_NOTIFICATION_ERROR:
        g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_COMMAND, "tmux error: %s",
                    notif->data ? notif->data : "(unknown)");
        sk_ctrl_notification_free(notif);
        g_free(line);
        g_string_free(result, TRUE);
        return false;

      case SK_CTRL_NOTIFICATION_EXIT:
        g_set_error_literal(error, SK_SESSION_ERROR, SK_SESSION_ERROR_CONTROL_MODE,
                            "tmux control mode exited");
        sk_ctrl_notification_free(notif);
        g_free(line);
        g_string_free(result, TRUE);
        return false;

      default:
        /* Ignore other notifications. */
        break;
      }
      sk_ctrl_notification_free(notif);
    }
    else if (in_output)
    {
      /* Data line between %begin and %end. */
      if (result->len > 0)
      {
        g_string_append_c(result, '\n');
      }
      g_string_append(result, line);
    }

    g_free(line);
  }

done:
  if (output != NULL)
  {
    *output = g_string_free(result, FALSE);
  }
  else
  {
    g_string_free(result, TRUE);
  }

  return success;
}

/* ------------------------------------------------------------------ */
/* Notification parsing                                                */
/* ------------------------------------------------------------------ */

SkCtrlNotification *
sk_ctrl_parse_notification(const char *line)
{
  if (line == NULL || line[0] != '%')
  {
    return NULL;
  }

  SkCtrlNotification *notif = g_new0(SkCtrlNotification, 1);
  notif->cmd_number = -1;

  if (g_str_has_prefix(line, "%begin "))
  {
    notif->type = SK_CTRL_NOTIFICATION_BEGIN;
    /* Format: %begin <time> <cmd-number> <flags> */
    const char *rest = line + 7;
    /* Skip timestamp. */
    while (*rest != '\0' && *rest != ' ')
      rest++;
    if (*rest == ' ')
      rest++;
    /* Parse command number. */
    char *end = NULL;
    notif->cmd_number = strtoll(rest, &end, 10);
    (void)end;
  }
  else if (g_str_has_prefix(line, "%end "))
  {
    notif->type = SK_CTRL_NOTIFICATION_END;
    const char *rest = line + 5;
    while (*rest != '\0' && *rest != ' ')
      rest++;
    if (*rest == ' ')
      rest++;
    char *end = NULL;
    notif->cmd_number = strtoll(rest, &end, 10);
    (void)end;
  }
  else if (g_str_has_prefix(line, "%error "))
  {
    notif->type = SK_CTRL_NOTIFICATION_ERROR;
    const char *rest = line + 7;
    notif->data = g_strdup(rest);
  }
  else if (g_str_has_prefix(line, "%output "))
  {
    notif->type = SK_CTRL_NOTIFICATION_OUTPUT;
    notif->data = g_strdup(line + 8);
  }
  else if (g_str_has_prefix(line, "%session-changed "))
  {
    notif->type = SK_CTRL_NOTIFICATION_SESSION_CHANGED;
    notif->data = g_strdup(line + 17);
  }
  else if (g_str_has_prefix(line, "%exit"))
  {
    notif->type = SK_CTRL_NOTIFICATION_EXIT;
    if (line[5] == ' ')
    {
      notif->data = g_strdup(line + 6);
    }
  }
  else
  {
    notif->type = SK_CTRL_NOTIFICATION_UNKNOWN;
    notif->data = g_strdup(line);
  }

  return notif;
}

void
sk_ctrl_notification_free(SkCtrlNotification *notif)
{
  if (notif == NULL)
    return;
  g_free(notif->data);
  g_free(notif);
}
