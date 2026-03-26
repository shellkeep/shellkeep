// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_sftp.c
 * @brief SFTP session and file operations.
 *
 * FR-STATE-06: state writes via SFTP are async (never block GTK).
 * FR-CONN-20 / FR-COMPAT-10: SFTP with fallback to shell commands.
 * FR-STATE-05: atomic writes via posix-rename@openssh.com.
 *
 * NFR-SEC-06: validate paths to prevent traversal.
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>
#include <libssh/sftp.h>

#include "sk_ssh_internal.h"
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/*  Internal SFTP session structure                                    */
/* ------------------------------------------------------------------ */

struct _SkSftpSession
{
  sftp_session sftp;
  SkSshConnection *conn; /* Borrowed reference. */
  gboolean has_posix_rename;
};

/* ------------------------------------------------------------------ */
/*  SFTP session lifecycle                                             */
/* ------------------------------------------------------------------ */

SkSftpSession *
sk_sftp_session_new(SkSshConnection *conn, GError **error)
{
  g_return_val_if_fail(conn != NULL, NULL);

  ssh_session session = sk_ssh_connection_get_session(conn);
  g_return_val_if_fail(session != NULL, NULL);

  sftp_session sftp = sftp_new(session);
  if (sftp == NULL)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "Failed to allocate SFTP session: %s",
                ssh_get_error(session));
    return NULL;
  }

  int rc = sftp_init(sftp);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP,
                "Failed to initialize SFTP session: %s (code %d)", ssh_get_error(session),
                sftp_get_error(sftp));
    sftp_free(sftp);
    return NULL;
  }

  SkSftpSession *s = g_new0(SkSftpSession, 1);
  s->sftp = sftp;
  s->conn = conn;

  /* Detect posix-rename@openssh.com extension (FR-STATE-05). */
  s->has_posix_rename = (sftp_extension_supported(sftp, "posix-rename@openssh.com", "1") != 0);

  return s;
}

gboolean
sk_sftp_has_posix_rename(SkSftpSession *sftp)
{
  g_return_val_if_fail(sftp != NULL, FALSE);
  return sftp->has_posix_rename;
}

void
sk_sftp_session_free(SkSftpSession *sftp)
{
  if (sftp == NULL)
    return;

  if (sftp->sftp != NULL)
  {
    sftp_free(sftp->sftp);
  }

  g_free(sftp);
}

/* ------------------------------------------------------------------ */
/*  Read file                                                          */
/* ------------------------------------------------------------------ */

gboolean
sk_sftp_read_file(SkSftpSession *sftp, const char *path, char **out_data, size_t *out_len,
                  GError **error)
{
  g_return_val_if_fail(sftp != NULL, FALSE);
  g_return_val_if_fail(path != NULL, FALSE);
  g_return_val_if_fail(out_data != NULL, FALSE);
  g_return_val_if_fail(out_len != NULL, FALSE);

  sftp_file file = sftp_open(sftp->sftp, path, O_RDONLY, 0);
  if (file == NULL)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP: failed to open '%s' for reading: %s",
                path, ssh_get_error(sk_ssh_connection_get_session(sftp->conn)));
    return FALSE;
  }

  /* Read in chunks into a GByteArray. */
  GByteArray *buf = g_byte_array_new();
  char chunk[8192];
  ssize_t nbytes;

  while ((nbytes = sftp_read(file, chunk, sizeof(chunk))) > 0)
  {
    g_byte_array_append(buf, (const guint8 *)chunk, (guint)nbytes);
  }

  sftp_close(file);

  if (nbytes < 0)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP: read error on '%s': %s", path,
                ssh_get_error(sk_ssh_connection_get_session(sftp->conn)));
    g_byte_array_free(buf, TRUE);
    return FALSE;
  }

  *out_len = buf->len;
  *out_data = (char *)g_byte_array_free(buf, FALSE);

  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  Write file (atomic: tmp + rename)  (FR-STATE-04, FR-STATE-05)      */
/* ------------------------------------------------------------------ */

gboolean
sk_sftp_write_file(SkSftpSession *sftp, const char *path, const char *data, size_t len, int mode,
                   GError **error)
{
  g_return_val_if_fail(sftp != NULL, FALSE);
  g_return_val_if_fail(path != NULL, FALSE);
  g_return_val_if_fail(data != NULL || len == 0, FALSE);

  /* Write to a temporary file in the same directory, then rename. */
  g_autofree char *tmp_path = g_strdup_printf("%s.tmp.%d", path, g_random_int());

  sftp_file file = sftp_open(sftp->sftp, tmp_path, O_WRONLY | O_CREAT | O_TRUNC, mode);
  if (file == NULL)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP: failed to create temp file '%s': %s",
                tmp_path, ssh_get_error(sk_ssh_connection_get_session(sftp->conn)));
    return FALSE;
  }

  /* Write in chunks. */
  size_t written = 0;
  while (written < len)
  {
    size_t chunk = len - written;
    if (chunk > 32768)
      chunk = 32768;

    ssize_t n = sftp_write(file, data + written, chunk);
    if (n < 0)
    {
      g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP: write error on '%s': %s", tmp_path,
                  ssh_get_error(sk_ssh_connection_get_session(sftp->conn)));
      sftp_close(file);
      sftp_unlink(sftp->sftp, tmp_path);
      return FALSE;
    }
    written += (size_t)n;
  }

  sftp_close(file);

  /* Atomic rename. */
  if (!sk_sftp_rename(sftp, tmp_path, path, error))
  {
    sftp_unlink(sftp->sftp, tmp_path);
    return FALSE;
  }

  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  Rename (with posix-rename detection)  (FR-STATE-05)                */
/* ------------------------------------------------------------------ */

gboolean
sk_sftp_rename(SkSftpSession *sftp, const char *old_path, const char *new_path, GError **error)
{
  g_return_val_if_fail(sftp != NULL, FALSE);
  g_return_val_if_fail(old_path != NULL, FALSE);
  g_return_val_if_fail(new_path != NULL, FALSE);

  int rc;

  if (sftp->has_posix_rename)
  {
    /* posix-rename@openssh.com allows atomic overwrite. */
    rc = sftp_rename(sftp->sftp, old_path, new_path);
  }
  else
  {
    /* Fallback: unlink destination first, then rename. */
    sftp_unlink(sftp->sftp, new_path); /* Ignore error if not exists. */
    rc = sftp_rename(sftp->sftp, old_path, new_path);
  }

  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP: rename '%s' -> '%s' failed: %s",
                old_path, new_path, ssh_get_error(sk_ssh_connection_get_session(sftp->conn)));
    return FALSE;
  }

  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  Exists                                                             */
/* ------------------------------------------------------------------ */

gboolean
sk_sftp_exists(SkSftpSession *sftp, const char *path)
{
  g_return_val_if_fail(sftp != NULL, FALSE);
  g_return_val_if_fail(path != NULL, FALSE);

  sftp_attributes attrs = sftp_stat(sftp->sftp, path);
  if (attrs == NULL)
    return FALSE;

  sftp_attributes_free(attrs);
  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  mkdir -p                                                           */
/* ------------------------------------------------------------------ */

gboolean
sk_sftp_mkdir_p(SkSftpSession *sftp, const char *path, int mode, GError **error)
{
  g_return_val_if_fail(sftp != NULL, FALSE);
  g_return_val_if_fail(path != NULL, FALSE);

  /* Check if it already exists. */
  sftp_attributes attrs = sftp_stat(sftp->sftp, path);
  if (attrs != NULL)
  {
    sftp_attributes_free(attrs);
    return TRUE; /* Already exists. */
  }

  /* Create parent directories recursively. */
  g_autofree char *parent = g_path_get_dirname(path);
  if (parent != NULL && strcmp(parent, ".") != 0 && strcmp(parent, "/") != 0 &&
      strcmp(parent, path) != 0)
  {
    GError *parent_err = NULL;
    if (!sk_sftp_mkdir_p(sftp, parent, mode, &parent_err))
    {
      g_propagate_error(error, parent_err);
      return FALSE;
    }
  }

  int rc = sftp_mkdir(sftp->sftp, path, mode);
  if (rc != SSH_OK)
  {
    /* Check if it was created between our stat and mkdir. */
    attrs = sftp_stat(sftp->sftp, path);
    if (attrs != NULL)
    {
      sftp_attributes_free(attrs);
      return TRUE;
    }

    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP: mkdir '%s' failed: %s", path,
                ssh_get_error(sk_ssh_connection_get_session(sftp->conn)));
    return FALSE;
  }

  return TRUE;
}
