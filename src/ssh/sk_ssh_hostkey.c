// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_hostkey.c
 * @brief Host key verification (FR-CONN-01..05).
 *
 * Uses ssh_session_is_known_server() with ~/.ssh/known_hosts.
 * Respects StrictHostKeyChecking from ~/.ssh/config (FR-CONN-05).
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>

#include "sk_ssh_internal.h"
#include <string.h>

/* FR-CONN-01: verify host key before any other operation. */
SkHostKeyStatus
sk_ssh_verify_host_key(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, SK_HOST_KEY_ERROR);

  ssh_session session = sk_ssh_connection_get_session(conn);
  g_return_val_if_fail(session != NULL, SK_HOST_KEY_ERROR);

  enum ssh_known_hosts_e state = ssh_session_is_known_server(session);

  switch (state)
  {
  case SSH_KNOWN_HOSTS_OK:
    return SK_HOST_KEY_OK;

  case SSH_KNOWN_HOSTS_CHANGED:
    /* FR-CONN-02: host key changed — possible MITM. */
    return SK_HOST_KEY_CHANGED;

  case SSH_KNOWN_HOSTS_NOT_FOUND:
    /* known_hosts file doesn't exist — treat as unknown. */
    return SK_HOST_KEY_UNKNOWN;

  case SSH_KNOWN_HOSTS_UNKNOWN:
    /* FR-CONN-03: host not in known_hosts (TOFU). */
    return SK_HOST_KEY_UNKNOWN;

  case SSH_KNOWN_HOSTS_OTHER:
    /* FR-CONN-04: different key type from what was stored. */
    return SK_HOST_KEY_OTHER;

  case SSH_KNOWN_HOSTS_ERROR:
  default:
    return SK_HOST_KEY_ERROR;
  }
}

gboolean
sk_ssh_accept_host_key(SkSshConnection *conn, GError **error)
{
  g_return_val_if_fail(conn != NULL, FALSE);

  ssh_session session = sk_ssh_connection_get_session(conn);
  int rc = ssh_session_update_known_hosts(session);

  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY, "Failed to update known_hosts: %s",
                ssh_get_error(session));
    return FALSE;
  }

  return TRUE;
}

char *
sk_ssh_get_host_fingerprint(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, NULL);

  ssh_session session = sk_ssh_connection_get_session(conn);
  ssh_key server_key = NULL;

  int rc = ssh_get_server_publickey(session, &server_key);
  if (rc != SSH_OK || server_key == NULL)
    return NULL;

  unsigned char *hash = NULL;
  size_t hash_len = 0;

  rc = ssh_get_publickey_hash(server_key, SSH_PUBLICKEY_HASH_SHA256, &hash, &hash_len);
  ssh_key_free(server_key);

  if (rc != 0 || hash == NULL)
    return NULL;

  char *fingerprint = ssh_get_fingerprint_hash(SSH_PUBLICKEY_HASH_SHA256, hash, hash_len);
  ssh_clean_pubkey_hash(&hash);

  if (fingerprint == NULL)
    return NULL;

  char *result = g_strdup(fingerprint);
  ssh_string_free_char(fingerprint);

  return result;
}

char *
sk_ssh_get_host_key_type(SkSshConnection *conn)
{
  g_return_val_if_fail(conn != NULL, NULL);

  ssh_session session = sk_ssh_connection_get_session(conn);
  ssh_key server_key = NULL;

  int rc = ssh_get_server_publickey(session, &server_key);
  if (rc != SSH_OK || server_key == NULL)
    return NULL;

  const char *type_str = ssh_key_type_to_char(ssh_key_type(server_key));
  char *result = type_str ? g_strdup(type_str) : NULL;

  ssh_key_free(server_key);
  return result;
}
