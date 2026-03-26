// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_crypto.c
 * @brief Cryptographic algorithm configuration.
 *
 * Defines explicit cipher/MAC/KEX lists per spec.  Rejects obsolete
 * algorithms (FR-COMPAT-10..11).
 *
 * Rejected algorithms: arcfour, 3des-cbc, aes*-cbc, blowfish-cbc,
 * hmac-sha1, hmac-md5, diffie-hellman-group1-sha1.
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>

#include "sk_ssh_internal.h"
#include <string.h>

/* ------------------------------------------------------------------ */
/*  Approved algorithm lists (from spec, in preference order)          */
/* ------------------------------------------------------------------ */

/** Ciphers — preference order per spec. */
static const char SK_CIPHERS[] = "chacha20-poly1305@openssh.com,"
                                 "aes256-gcm@openssh.com,"
                                 "aes128-gcm@openssh.com,"
                                 "aes256-ctr,"
                                 "aes192-ctr,"
                                 "aes128-ctr";

/** MACs — preference order per spec. */
static const char SK_MACS[] = "hmac-sha2-256-etm@openssh.com,"
                              "hmac-sha2-512-etm@openssh.com,"
                              "hmac-sha2-256,"
                              "hmac-sha2-512";

/** Key exchange — preference order per spec. */
static const char SK_KEX[] = "curve25519-sha256,"
                             "curve25519-sha256@libssh.org,"
                             "ecdh-sha2-nistp256,"
                             "ecdh-sha2-nistp384,"
                             "ecdh-sha2-nistp521,"
                             "diffie-hellman-group16-sha512,"
                             "diffie-hellman-group18-sha512,"
                             "diffie-hellman-group14-sha256";

/** Host key types — Ed25519 preferred, then ECDSA, then RSA.
 *  FR-CONN-08: refuse DSA. */
static const char SK_HOSTKEYS[] = "ssh-ed25519,"
                                  "ssh-ed25519-cert-v01@openssh.com,"
                                  "ecdsa-sha2-nistp256,"
                                  "ecdsa-sha2-nistp384,"
                                  "ecdsa-sha2-nistp521,"
                                  "ecdsa-sha2-nistp256-cert-v01@openssh.com,"
                                  "ecdsa-sha2-nistp384-cert-v01@openssh.com,"
                                  "ecdsa-sha2-nistp521-cert-v01@openssh.com,"
                                  "rsa-sha2-512,"
                                  "rsa-sha2-256,"
                                  "rsa-sha2-512-cert-v01@openssh.com,"
                                  "rsa-sha2-256-cert-v01@openssh.com";

/* ------------------------------------------------------------------ */
/*  Apply configuration                                                */
/* ------------------------------------------------------------------ */

gboolean
sk_ssh_configure_crypto(SkSshConnection *conn, GError **error)
{
  g_return_val_if_fail(conn != NULL, FALSE);

  ssh_session session = sk_ssh_connection_get_session(conn);
  g_return_val_if_fail(session != NULL, FALSE);

  int rc;

  /* Set ciphers (client->server and server->client). */
  rc = ssh_options_set(session, SSH_OPTIONS_CIPHERS_C_S, SK_CIPHERS);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO,
                "Failed to set client->server ciphers: %s", ssh_get_error(session));
    return FALSE;
  }

  rc = ssh_options_set(session, SSH_OPTIONS_CIPHERS_S_C, SK_CIPHERS);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO,
                "Failed to set server->client ciphers: %s", ssh_get_error(session));
    return FALSE;
  }

  /* Set HMAC algorithms. */
  rc = ssh_options_set(session, SSH_OPTIONS_HMAC_C_S, SK_MACS);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO, "Failed to set client->server MACs: %s",
                ssh_get_error(session));
    return FALSE;
  }

  rc = ssh_options_set(session, SSH_OPTIONS_HMAC_S_C, SK_MACS);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO, "Failed to set server->client MACs: %s",
                ssh_get_error(session));
    return FALSE;
  }

  /* Set key exchange algorithms. */
  rc = ssh_options_set(session, SSH_OPTIONS_KEY_EXCHANGE, SK_KEX);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO,
                "Failed to set key exchange algorithms: %s", ssh_get_error(session));
    return FALSE;
  }

  /* Set host key types. */
  rc = ssh_options_set(session, SSH_OPTIONS_HOSTKEYS, SK_HOSTKEYS);
  if (rc != SSH_OK)
  {
    g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO, "Failed to set host key types: %s",
                ssh_get_error(session));
    return FALSE;
  }

  return TRUE;
}
