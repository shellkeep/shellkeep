// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_shell_fallback.c
 * @brief Shell-based file operations when SFTP is unavailable.
 *
 * FR-CONN-20: fallback to shell commands (cat, mktemp+mv).
 * FR-COMPAT-10: warn once when using fallback.
 *
 * NFR-SEC-07: never interpolate untrusted strings into shell commands.
 * File paths are passed via base64 encoding to avoid injection.
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>

#include "sk_ssh_internal.h"
#include <string.h>

/* ------------------------------------------------------------------ */
/*  Helpers                                                            */
/* ------------------------------------------------------------------ */

/**
 * Execute a command on a new channel and read all output.
 * Returns the output in out_data/out_len, or sets error.
 */
static gboolean
exec_and_read(SkSshConnection *conn, const char *command, char **out_data, size_t *out_len,
              GError **error)
{
  SkSshChannel *ch = sk_ssh_channel_open(conn, 80, 24, error);
  if (ch == NULL)
    return FALSE;

  if (!sk_ssh_channel_exec(ch, command, error))
  {
    sk_ssh_channel_free(ch);
    return FALSE;
  }

  /* Read all stdout. */
  GByteArray *buf = g_byte_array_new();
  /* We need the raw channel for blocking read; get it from our struct.
   * For the shell fallback we do a blocking read since this is already
   * in a worker thread (INV-IO-1). */
  char chunk[8192];
  int nbytes;

  /* Use the non-blocking read in a poll loop since we have the channel. */
  while (sk_ssh_channel_is_open(ch))
  {
    nbytes = sk_ssh_channel_read_nonblocking(ch, chunk, sizeof(chunk));
    if (nbytes > 0)
    {
      g_byte_array_append(buf, (const guint8 *)chunk, (guint)nbytes);
    }
    else if (nbytes == 0)
    {
      /* No data yet; small sleep to avoid busy-wait.
       * This is acceptable since we're in a worker thread. */
      g_usleep(1000);
    }
    else
    {
      /* EOF or error. */
      break;
    }
  }

  int exit_status = sk_ssh_channel_get_exit_status(ch);
  sk_ssh_channel_free(ch);

  if (exit_status != 0)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Shell command failed (exit %d): %s",
                exit_status, command);
    g_byte_array_free(buf, TRUE);
    return FALSE;
  }

  if (out_data != NULL && out_len != NULL)
  {
    *out_len = buf->len;
    *out_data = (char *)g_byte_array_free(buf, FALSE);
  }
  else
  {
    g_byte_array_free(buf, TRUE);
  }

  return TRUE;
}

/**
 * Execute a command that produces no meaningful stdout.
 */
static gboolean
exec_command(SkSshConnection *conn, const char *command, GError **error)
{
  return exec_and_read(conn, command, NULL, NULL, error);
}

/* ------------------------------------------------------------------ */
/*  Shell read file  (FR-CONN-20)                                      */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_shell_read_file(SkSshConnection *conn, const char *path, char **out_data, size_t *out_len,
                       GError **error)
{
  g_return_val_if_fail(conn != NULL, FALSE);
  g_return_val_if_fail(path != NULL, FALSE);
  g_return_val_if_fail(out_data != NULL, FALSE);
  g_return_val_if_fail(out_len != NULL, FALSE);

  /* NFR-SEC-07: use base64-encoded path to avoid shell injection.
   * We encode the path and decode it in the shell command. */
  g_autofree char *b64_path = g_base64_encode((const guchar *)path, strlen(path));

  /* Command decodes the path and cats the file.
   * We use printf to avoid issues with echo and escape sequences. */
  g_autofree char *cmd = g_strdup_printf("cat \"$(echo '%s' | base64 -d)\"", b64_path);

  return exec_and_read(conn, cmd, out_data, out_len, error);
}

/* ------------------------------------------------------------------ */
/*  Shell write file  (FR-CONN-20)                                     */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_shell_write_file(SkSshConnection *conn, const char *path, const char *data, size_t len,
                        int mode, GError **error)
{
  g_return_val_if_fail(conn != NULL, FALSE);
  g_return_val_if_fail(path != NULL, FALSE);
  g_return_val_if_fail(data != NULL || len == 0, FALSE);

  /* Encode both path and data as base64 to avoid any injection. */
  g_autofree char *b64_path = g_base64_encode((const guchar *)path, strlen(path));
  g_autofree char *b64_data = g_base64_encode((const guchar *)data, len);

  /* Atomic write via mktemp + base64 decode + mv.
   * FR-STATE-04: write to tmp then rename. */
  g_autofree char *cmd = g_strdup_printf("set -e; "
                                         "_p=\"$(echo '%s' | base64 -d)\"; "
                                         "_d=\"$(dirname \"$_p\")\"; "
                                         "_t=\"$(mktemp \"${_d}/.shellkeep.XXXXXX\")\"; "
                                         "echo '%s' | base64 -d > \"$_t\"; "
                                         "chmod %04o \"$_t\"; "
                                         "mv -f \"$_t\" \"$_p\"",
                                         b64_path, b64_data, mode);

  return exec_command(conn, cmd, error);
}
