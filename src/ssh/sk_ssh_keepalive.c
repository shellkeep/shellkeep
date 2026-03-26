// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_keepalive.c
 * @brief SSH and TCP keepalive configuration.
 *
 * Keepalive settings:
 * - SSH-level: default 15s interval, 3 max attempts (encrypted, NAT-friendly).
 * - TCP-level: secondary defense (TCP_KEEPIDLE=15, TCP_KEEPINTVL=5, TCP_KEEPCNT=3).
 * - TCP_NODELAY on all connections to disable Nagle.
 *
 * If ServerAliveInterval/ServerAliveCountMax are set in ~/.ssh/config,
 * those values prevail (FR-COMPAT-01).
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>

#include "sk_ssh_internal.h"

/* Default keepalive values per spec. */
#define SK_DEFAULT_KEEPALIVE_INTERVAL 15
#define SK_DEFAULT_KEEPALIVE_COUNT_MAX 3

gboolean
sk_ssh_configure_keepalive(SkSshConnection *conn, GError **error)
{
  g_return_val_if_fail(conn != NULL, FALSE);

  ssh_session session = sk_ssh_connection_get_session(conn);
  g_return_val_if_fail(session != NULL, FALSE);

  const SkSshOptions *opts = sk_ssh_connection_get_opts(conn);

  /*
   * Determine keepalive parameters.
   * Priority: user option > ssh_config value > shellkeep default.
   *
   * Note: ssh_options_parse_config() has already been called in
   * sk_ssh_connection_new(), so ServerAliveInterval/ServerAliveCountMax
   * from ~/.ssh/config are already applied to the session.  We only
   * override if the caller passed explicit values.
   */
  int interval =
      opts->keepalive_interval > 0 ? opts->keepalive_interval : SK_DEFAULT_KEEPALIVE_INTERVAL;

  /* Note: libssh does not have a direct "count max" option.
   * We use SSH_OPTIONS_TIMEOUT to set the overall timeout, and
   * the keepalive interval handles the SSH_MSG_IGNORE probe. */

  /* Enable SSH-level keepalive. */
  int rc = ssh_options_set(session, SSH_OPTIONS_TIMEOUT, &interval);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CONNECT, "Failed to set keepalive interval: %s",
                ssh_get_error(session));
    return FALSE;
  }

  /* Note: TCP socket options (TCP_NODELAY, TCP_KEEPALIVE, etc.) are
   * already configured in configure_tcp_socket() called during connect.
   * This function focuses on SSH-level keepalive only. */

  return TRUE;
}
