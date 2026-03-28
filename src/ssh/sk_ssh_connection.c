// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_connection.c
 * @brief SSH connection lifecycle: new, connect, disconnect, free.
 *
 * Wraps libssh ssh_session with host key verification, authentication,
 * crypto configuration, and keepalive.  All blocking operations are
 * designed to run in GTask worker threads (INV-IO-1).
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/callbacks.h>
#include <libssh/libssh.h>

#include "sk_ssh_internal.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#else
#include <netinet/tcp.h>
#include <sys/socket.h>
#endif

/* FR-CONN-21: never invoke ssh binary; everything via libssh. */
/* INV-CONN-3 */

G_DEFINE_QUARK(sk - ssh - error - quark, sk_ssh_error)

/* ------------------------------------------------------------------ */
/*  Internal connection structure                                      */
/* ------------------------------------------------------------------ */

struct _SkSshConnection
{
  ssh_session session;
  SkSshOptions opts; /* Deep copy of caller options. */
  gboolean connected;
  gboolean authenticated;
  GMutex lock; /* Protects connected/authenticated flags. */

  /* Deep-copied strings from opts so the caller can free theirs. */
  char *hostname;
  char *username;
  char *identity_file;
};

/* ------------------------------------------------------------------ */
/*  Helpers                                                            */
/* ------------------------------------------------------------------ */

static char *
safe_strdup(const char *s)
{
  return s ? g_strdup(s) : NULL;
}

/* ------------------------------------------------------------------ */
/*  Constructor / destructor                                           */
/* ------------------------------------------------------------------ */

SkSshConnection *
sk_ssh_connection_new(const SkSshOptions *opts, GError **error)
{
  g_return_val_if_fail(opts != NULL, NULL);
  g_return_val_if_fail(opts->hostname != NULL, NULL);

  ssh_session session = ssh_new();
  if (session == NULL)
  {
    g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_CONNECT,
                        "Failed to allocate libssh session");
    return NULL;
  }

  SkSshConnection *conn = g_new0(SkSshConnection, 1);
  conn->session = session;
  g_mutex_init(&conn->lock);

  /* Deep-copy strings. */
  conn->hostname = g_strdup(opts->hostname);
  conn->username = safe_strdup(opts->username);
  conn->identity_file = safe_strdup(opts->identity_file);

  /* Copy the full options struct, but point strings at our copies. */
  conn->opts = *opts;
  conn->opts.hostname = conn->hostname;
  conn->opts.username = conn->username;
  conn->opts.identity_file = conn->identity_file;

  /* --- Apply libssh session options --- */

  ssh_options_set(session, SSH_OPTIONS_HOST, conn->hostname);

  int port = opts->port > 0 ? opts->port : 22;
  ssh_options_set(session, SSH_OPTIONS_PORT, &port);

  if (conn->username != NULL)
  {
    ssh_options_set(session, SSH_OPTIONS_USER, conn->username);
  }

  if (conn->identity_file != NULL)
  {
    ssh_options_set(session, SSH_OPTIONS_IDENTITY, conn->identity_file);
  }

  int timeout = opts->connect_timeout > 0 ? opts->connect_timeout : 30;
  ssh_options_set(session, SSH_OPTIONS_TIMEOUT, &timeout);

  /* FR-COMPAT-01..05: parse ~/.ssh/config for this host. */
  ssh_options_parse_config(session, NULL);

  /* Apply crypto configuration before connecting. */
  GError *crypto_err = NULL;
  if (!sk_ssh_configure_crypto(conn, &crypto_err))
  {
    /* Non-fatal: log warning but continue with libssh defaults. */
    /* TODO: integrate with sk_log once logging layer is ready */
    g_warning("sk_ssh: crypto config: %s", crypto_err->message);
    g_clear_error(&crypto_err);
  }

  return conn;
}

/* ------------------------------------------------------------------ */
/*  TCP socket tuning                                                  */
/* ------------------------------------------------------------------ */

static void
configure_tcp_socket(int fd)
{
  if (fd < 0)
    return;

  /* TCP_NODELAY — disable Nagle for low-latency interactive I/O. */
  int flag = 1;
  setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, (const char *)&flag, sizeof(flag));

  /* TCP keepalive as secondary defense layer. */
  setsockopt(fd, SOL_SOCKET, SO_KEEPALIVE, (const char *)&flag, sizeof(flag));

#ifdef TCP_KEEPIDLE
  int idle = 15;
  setsockopt(fd, IPPROTO_TCP, TCP_KEEPIDLE, (const char *)&idle, sizeof(idle));
#endif
#ifdef TCP_KEEPINTVL
  int intvl = 5;
  setsockopt(fd, IPPROTO_TCP, TCP_KEEPINTVL, (const char *)&intvl, sizeof(intvl));
#endif
#ifdef TCP_KEEPCNT
  int cnt = 3;
  setsockopt(fd, IPPROTO_TCP, TCP_KEEPCNT, (const char *)&cnt, sizeof(cnt));
#endif
}

/* ------------------------------------------------------------------ */
/*  Connect (blocking — run in GTask)                                  */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_connection_connect(SkSshConnection *conn, GError **error)
{
  g_return_val_if_fail(conn != NULL, FALSE);
  g_return_val_if_fail(conn->session != NULL, FALSE);

  /* --- TCP connect + SSH handshake --- */
  int rc = ssh_connect(conn->session);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CONNECT, "SSH connection failed: %s",
                ssh_get_error(conn->session));
    return FALSE;
  }

  /* TCP socket tuning (TCP_NODELAY, keepalive). */
  int fd = ssh_get_fd(conn->session);
  configure_tcp_socket(fd);

  /* --- Host key verification (FR-CONN-01..05) --- */
  SkHostKeyStatus hk_status = sk_ssh_verify_host_key(conn);

  switch (hk_status)
  {
  case SK_HOST_KEY_OK:
    /* Key matches known_hosts — proceed. */
    break;

  case SK_HOST_KEY_CHANGED:
    /* FR-CONN-02: Block connection.  No override. */
    g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY,
                        "Host key has CHANGED since last connection. "
                        "This may indicate a man-in-the-middle attack. "
                        "Update ~/.ssh/known_hosts manually.");
    ssh_disconnect(conn->session);
    return FALSE;

  case SK_HOST_KEY_UNKNOWN:
  {
    /* FR-CONN-03: TOFU dialog via callback. */
    if (conn->opts.host_key_unknown_cb == NULL)
    {
      g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY,
                          "Unknown host key and no callback registered");
      ssh_disconnect(conn->session);
      return FALSE;
    }
    g_autofree char *fp = sk_ssh_get_host_fingerprint(conn);
    g_autofree char *kt = sk_ssh_get_host_key_type(conn);

    gboolean accepted = conn->opts.host_key_unknown_cb(conn, fp, kt, conn->opts.cb_user_data);

    if (!accepted)
    {
      g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY, "Host key rejected by user");
      ssh_disconnect(conn->session);
      return FALSE;
    }
    /* Accept and save the key. */
    GError *save_err = NULL;
    if (!sk_ssh_accept_host_key(conn, &save_err))
    {
      g_warning("sk_ssh: failed to save host key: %s", save_err->message);
      g_clear_error(&save_err);
      /* Non-fatal: connection can proceed. */
    }
    break;
  }

  case SK_HOST_KEY_OTHER:
  {
    /* FR-CONN-04: Different key type dialog. */
    if (conn->opts.host_key_other_cb == NULL)
    {
      g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY,
                          "Host key type changed and no callback");
      ssh_disconnect(conn->session);
      return FALSE;
    }
    g_autofree char *fp = sk_ssh_get_host_fingerprint(conn);
    /* Note: libssh doesn't easily expose the old key type;
     * pass "unknown" as old_key_type. */
    g_autofree char *kt = sk_ssh_get_host_key_type(conn);

    gboolean accepted =
        conn->opts.host_key_other_cb(conn, fp, "unknown", kt, conn->opts.cb_user_data);

    if (!accepted)
    {
      g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY,
                          "New host key type rejected by user");
      ssh_disconnect(conn->session);
      return FALSE;
    }
    GError *save_err = NULL;
    sk_ssh_accept_host_key(conn, &save_err);
    g_clear_error(&save_err);
    break;
  }

  case SK_HOST_KEY_ERROR:
  default:
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY, "Host key verification error: %s",
                ssh_get_error(conn->session));
    ssh_disconnect(conn->session);
    return FALSE;
  }

  /* --- Authentication (FR-CONN-06..12) --- */
  SkAuthResult auth = sk_ssh_authenticate(conn, error);
  if (auth != SK_AUTH_SUCCESS)
  {
    /* error is already set by sk_ssh_authenticate. */
    ssh_disconnect(conn->session);
    return FALSE;
  }

  /* --- SSH keepalive configuration --- */
  GError *ka_err = NULL;
  if (!sk_ssh_configure_keepalive(conn, &ka_err))
  {
    g_warning("sk_ssh: keepalive config: %s", ka_err->message);
    g_clear_error(&ka_err);
  }

  g_mutex_lock(&conn->lock);
  conn->connected = TRUE;
  conn->authenticated = TRUE;
  g_mutex_unlock(&conn->lock);

  return TRUE;
}

/* ------------------------------------------------------------------ */
/*  Disconnect / free                                                  */
/* ------------------------------------------------------------------ */

void
sk_ssh_connection_disconnect(SkSshConnection *conn)
{
  if (conn == NULL)
    return;

  g_mutex_lock(&conn->lock);
  gboolean was_connected = conn->connected;
  conn->connected = FALSE;
  conn->authenticated = FALSE;
  g_mutex_unlock(&conn->lock);

  if (was_connected && conn->session != NULL)
  {
    ssh_disconnect(conn->session);
  }
}

void
sk_ssh_connection_free(SkSshConnection *conn)
{
  if (conn == NULL)
    return;

  sk_ssh_connection_disconnect(conn);

  if (conn->session != NULL)
  {
    ssh_free(conn->session);
    conn->session = NULL;
  }

  g_free(conn->hostname);
  g_free(conn->username);
  g_free(conn->identity_file);
  g_mutex_clear(&conn->lock);
  g_free(conn);
}

/* ------------------------------------------------------------------ */
/*  Accessors                                                          */
/* ------------------------------------------------------------------ */

int
sk_ssh_connection_get_fd(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, -1);
  if (!sk_ssh_connection_is_connected(conn))
    return -1;
  return ssh_get_fd(conn->session);
}

gboolean
sk_ssh_connection_is_connected(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, FALSE);

  g_mutex_lock(&conn->lock);
  gboolean result = conn->connected && ssh_is_connected(conn->session);
  g_mutex_unlock(&conn->lock);

  return result;
}

const char *
sk_ssh_connection_get_error(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, "NULL connection");
  return ssh_get_error(conn->session);
}

/* ------------------------------------------------------------------ */
/*  Internal accessor for other ssh source files                        */
/* ------------------------------------------------------------------ */

ssh_session
sk_ssh_connection_get_session(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, NULL);
  return conn->session;
}

const SkSshOptions *
sk_ssh_connection_get_opts(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, NULL);
  return &conn->opts;
}

/* ------------------------------------------------------------------ */
/*  Async wrappers (INV-IO-1)                                          */
/* ------------------------------------------------------------------ */

static void
connect_thread_func(GTask *task, gpointer source_object, gpointer task_data,
                    GCancellable *cancellable)
{
  SkSshConnection *conn = task_data;
  GError *error = NULL;

  (void)source_object;
  (void)cancellable;

  if (sk_ssh_connection_connect(conn, &error))
  {
    g_task_return_boolean(task, TRUE);
  }
  else
  {
    g_task_return_error(task, error);
  }
}

void
sk_ssh_connection_connect_async(SkSshConnection *conn, GCancellable *cancellable,
                                GAsyncReadyCallback callback, gpointer user_data)
{
  g_return_if_fail(conn != NULL);

  GTask *task = g_task_new(NULL, cancellable, callback, user_data);
  g_task_set_task_data(task, conn, NULL);
  g_task_run_in_thread(task, connect_thread_func);
  g_object_unref(task);
}

gboolean
sk_ssh_connection_connect_finish(SkSshConnection *conn, GAsyncResult *result, GError **error)
{
  (void)conn;
  return g_task_propagate_boolean(G_TASK(result), error);
}

guint
sk_ssh_connection_add_io_watch(SkSshConnection *conn, GIOFunc callback, gpointer user_data)
{
  g_return_val_if_fail(conn != NULL, 0);

  int fd = sk_ssh_connection_get_fd(conn);
  if (fd < 0)
    return 0;

  GIOChannel *channel = g_io_channel_unix_new(fd);
  g_io_channel_set_close_on_unref(channel, FALSE); /* fd owned by libssh */
  guint source_id = g_io_add_watch(channel, G_IO_IN | G_IO_HUP | G_IO_ERR, callback, user_data);
  g_io_channel_unref(channel);
  return source_id;
}
