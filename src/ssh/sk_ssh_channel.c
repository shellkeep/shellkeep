// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_channel.c
 * @brief SSH channel management: PTY allocation, resize, read/write.
 *
 * Each tab gets its own SSH channel with a PTY (FR-CONN-22, INV-CONN-1).
 * PTY resize dispatches SIGWINCH to the remote (FR-TERMINAL-16).
 * Reads are non-blocking for use with g_io_add_watch() (INV-IO-1).
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>

#include "sk_ssh_internal.h"
#include <string.h>

/* ------------------------------------------------------------------ */
/*  Internal channel structure                                         */
/* ------------------------------------------------------------------ */

struct _SkSshChannel
{
  ssh_channel channel;
  SkSshConnection *conn; /* Borrowed reference (not owned). */
};

/* ------------------------------------------------------------------ */
/*  Open channel with PTY                                              */
/* ------------------------------------------------------------------ */

SkSshChannel *
sk_ssh_channel_open(SkSshConnection *conn, int cols, int rows, GError **error)
{
  g_return_val_if_fail(conn != NULL, NULL);
  g_return_val_if_fail(cols > 0 && rows > 0, NULL);

  ssh_session session = sk_ssh_connection_get_session(conn);
  g_return_val_if_fail(session != NULL, NULL);

  ssh_channel ch = ssh_channel_new(session);
  if (ch == NULL)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Failed to allocate SSH channel: %s",
                ssh_get_error(session));
    return NULL;
  }

  int rc = ssh_channel_open_session(ch);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Failed to open session channel: %s",
                ssh_get_error(session));
    ssh_channel_free(ch);
    return NULL;
  }

  /* Request PTY with xterm-256color (standard for modern terminals). */
  rc = ssh_channel_request_pty_size(ch, "xterm-256color", cols, rows);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Failed to allocate PTY: %s",
                ssh_get_error(session));
    ssh_channel_close(ch);
    ssh_channel_free(ch);
    return NULL;
  }

  SkSshChannel *channel = g_new0(SkSshChannel, 1);
  channel->channel = ch;
  channel->conn = conn;

  return channel;
}

/* ------------------------------------------------------------------ */
/*  Shell / exec                                                       */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_channel_request_shell(SkSshChannel *channel, GError **error)
{
  g_return_val_if_fail(channel != NULL, FALSE);

  ssh_session session = sk_ssh_connection_get_session(channel->conn);

  int rc = ssh_channel_request_shell(channel->channel);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Failed to request shell: %s",
                ssh_get_error(session));
    return FALSE;
  }

  return TRUE;
}

gboolean
sk_ssh_channel_exec(SkSshChannel *channel, const char *command, GError **error)
{
  g_return_val_if_fail(channel != NULL, FALSE);
  g_return_val_if_fail(command != NULL, FALSE);

  ssh_session session = sk_ssh_connection_get_session(channel->conn);

  int rc = ssh_channel_request_exec(channel->channel, command);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Failed to execute command: %s",
                ssh_get_error(session));
    return FALSE;
  }

  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  PTY resize  (FR-TERMINAL-16)                                       */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_channel_resize_pty(SkSshChannel *channel, int cols, int rows, GError **error)
{
  g_return_val_if_fail(channel != NULL, FALSE);
  g_return_val_if_fail(cols > 0 && rows > 0, FALSE);

  /* FR-TERMINAL-16: explicit and mandatory PTY size change. */
  int rc = ssh_channel_change_pty_size(channel->channel, cols, rows);
  if (rc != SSH_OK)
  {
    ssh_session session = sk_ssh_connection_get_session(channel->conn);
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Failed to resize PTY: %s",
                ssh_get_error(session));
    return FALSE;
  }

  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  Non-blocking read / write                                          */
/* ------------------------------------------------------------------ */

int
sk_ssh_channel_read_nonblocking(SkSshChannel *channel, void *buf, size_t bufsize)
{
  g_return_val_if_fail(channel != NULL, -1);
  g_return_val_if_fail(buf != NULL, -1);

  /* ssh_channel_read_nonblocking returns:
   *   >0  = bytes read
   *    0  = no data available
   *   <0  = error or EOF */
  return ssh_channel_read_nonblocking(channel->channel, buf, (uint32_t)bufsize, 0);
}

int
sk_ssh_channel_write(SkSshChannel *channel, const void *data, size_t len)
{
  g_return_val_if_fail(channel != NULL, -1);
  g_return_val_if_fail(data != NULL, -1);

  return ssh_channel_write(channel->channel, data, (uint32_t)len);
}

/* ------------------------------------------------------------------ */
/*  Status queries                                                     */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_channel_is_open(SkSshChannel *channel)
{
  g_return_val_if_fail(channel != NULL, FALSE);

  return ssh_channel_is_open(channel->channel) && !ssh_channel_is_eof(channel->channel);
}

int
sk_ssh_channel_get_exit_status(SkSshChannel *channel)
{
  g_return_val_if_fail(channel != NULL, -1);
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"
  return ssh_channel_get_exit_status(channel->channel);
#pragma GCC diagnostic pop
}

/* ------------------------------------------------------------------ */
/*  Cleanup                                                            */
/* ------------------------------------------------------------------ */

void
sk_ssh_channel_free(SkSshChannel *channel)
{
  if (channel == NULL)
    return;

  if (channel->channel != NULL)
  {
    if (ssh_channel_is_open(channel->channel))
    {
      ssh_channel_send_eof(channel->channel);
      ssh_channel_close(channel->channel);
    }
    ssh_channel_free(channel->channel);
  }

  g_free(channel);
}
