// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_auth.c
 * @brief SSH authentication: agent, pubkey, password, keyboard-interactive.
 *
 * Authentication order (FR-CONN-06): agent -> pubkey -> password -> kbd-interactive.
 * Passwords are NEVER stored (NFR-SEC-08). Sensitive memory is zeroed with
 * explicit_bzero() after use (NFR-SEC-09).
 */

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>

#include "sk_ssh_internal.h"
#include <string.h>
#include <sys/mman.h>

/* macOS does not provide explicit_bzero(); use memset_s instead. */
#ifdef __APPLE__
#define explicit_bzero(s, n) memset_s((s), (n), 0, (n))
#endif

/**
 * Securely zero and free a password string.
 * NFR-SEC-09: mlock() prevents swap, explicit_bzero() zeros after use.
 */
static void
secure_free_password(char *password)
{
  if (password == NULL)
    return;
  size_t len = strlen(password);
  /* Unlock memory before freeing (mlock was called when received). */
  munlock(password, len + 1);
  explicit_bzero(password, len);
  g_free(password);
}

/**
 * Lock password memory to prevent it from being swapped to disk.
 * NFR-SEC-09: mlock() for cryptographic material.
 */
static void
secure_lock_password(const char *password)
{
  if (password == NULL)
    return;
  size_t len = strlen(password);
  /* Best-effort: mlock may fail if RLIMIT_MEMLOCK is too low. */
  (void)mlock(password, len + 1);
}

/**
 * Securely zero and free an array of response strings.
 */
static void
secure_free_responses(char **responses, int n)
{
  if (responses == NULL)
    return;
  for (int i = 0; i < n; i++)
  {
    secure_free_password(responses[i]);
  }
  g_free(responses);
}

/* ------------------------------------------------------------------ */
/*  Agent authentication  (FR-CONN-07)                                 */
/* ------------------------------------------------------------------ */

static SkAuthResult
try_agent_auth(ssh_session session)
{
  int rc = ssh_userauth_agent(session, NULL);

  switch (rc)
  {
  case SSH_AUTH_SUCCESS:
    return SK_AUTH_SUCCESS;
  case SSH_AUTH_PARTIAL:
    return SK_AUTH_PARTIAL;
  case SSH_AUTH_DENIED:
    return SK_AUTH_DENIED;
  default:
    return SK_AUTH_ERROR;
  }
}

/* ------------------------------------------------------------------ */
/*  Public key authentication  (FR-CONN-08, FR-CONN-11)                */
/* ------------------------------------------------------------------ */

static SkAuthResult
try_pubkey_auth(ssh_session session)
{
  /* ssh_userauth_publickey_auto tries all keys in ~/.ssh/ and
   * also loads <key>-cert.pub certificates (FR-CONN-11). */
  int rc = ssh_userauth_publickey_auto(session, NULL, NULL);

  switch (rc)
  {
  case SSH_AUTH_SUCCESS:
    return SK_AUTH_SUCCESS;
  case SSH_AUTH_PARTIAL:
    return SK_AUTH_PARTIAL;
  case SSH_AUTH_DENIED:
    return SK_AUTH_DENIED;
  default:
    return SK_AUTH_ERROR;
  }
}

/* ------------------------------------------------------------------ */
/*  Password authentication  (FR-CONN-09)                              */
/* ------------------------------------------------------------------ */

static SkAuthResult
try_password_auth(ssh_session session, SkSshConnection *conn)
{
  const SkSshOptions *opts = sk_ssh_connection_get_opts(conn);

  if (opts->password_cb == NULL)
    return SK_AUTH_DENIED;

  /* NFR-SEC-08: never save password; request from user via callback. */
  char *password = opts->password_cb(conn, "Password: ", opts->cb_user_data);
  if (password == NULL)
    return SK_AUTH_CANCELLED;

  /* NFR-SEC-09: lock password memory to prevent swap. */
  secure_lock_password(password);

  int rc = ssh_userauth_password(session, NULL, password);
  secure_free_password(password);

  switch (rc)
  {
  case SSH_AUTH_SUCCESS:
    return SK_AUTH_SUCCESS;
  case SSH_AUTH_PARTIAL:
    return SK_AUTH_PARTIAL;
  case SSH_AUTH_DENIED:
    return SK_AUTH_DENIED;
  default:
    return SK_AUTH_ERROR;
  }
}

/* ------------------------------------------------------------------ */
/*  Keyboard-interactive authentication  (FR-CONN-10)                  */
/* ------------------------------------------------------------------ */

static SkAuthResult
try_kbdint_auth(ssh_session session, SkSshConnection *conn)
{
  const SkSshOptions *opts = sk_ssh_connection_get_opts(conn);

  if (opts->kbd_interactive_cb == NULL)
    return SK_AUTH_DENIED;

  int rc = ssh_userauth_kbdint(session, NULL, NULL);

  while (rc == SSH_AUTH_INFO)
  {
    const char *name = ssh_userauth_kbdint_getname(session);
    const char *instruction = ssh_userauth_kbdint_getinstruction(session);
    int n_prompts = ssh_userauth_kbdint_getnprompts(session);

    if (n_prompts == 0)
    {
      /* Server sent info with no prompts; acknowledge and continue. */
      rc = ssh_userauth_kbdint(session, NULL, NULL);
      continue;
    }

    /* Build arrays for the callback. */
    const char **prompts = g_new0(const char *, n_prompts);
    gboolean *show = g_new0(gboolean, n_prompts);

    for (int i = 0; i < n_prompts; i++)
    {
      char echo = 0;
      prompts[i] = ssh_userauth_kbdint_getprompt(session, i, &echo);
      show[i] = (echo != 0);
    }

    char **responses = opts->kbd_interactive_cb(conn, name, instruction, prompts, show, n_prompts,
                                                opts->cb_user_data);

    g_free(prompts);
    g_free(show);

    if (responses == NULL)
      return SK_AUTH_CANCELLED;

    /* NFR-SEC-09: lock response memory to prevent swap. */
    for (int i = 0; i < n_prompts; i++)
    {
      secure_lock_password(responses[i]);
    }

    /* Submit the responses. */
    for (int i = 0; i < n_prompts; i++)
    {
      rc = ssh_userauth_kbdint_setanswer(session, i, responses[i] ? responses[i] : "");
    }

    secure_free_responses(responses, n_prompts);

    rc = ssh_userauth_kbdint(session, NULL, NULL);
  }

  switch (rc)
  {
  case SSH_AUTH_SUCCESS:
    return SK_AUTH_SUCCESS;
  case SSH_AUTH_PARTIAL:
    return SK_AUTH_PARTIAL;
  case SSH_AUTH_DENIED:
    return SK_AUTH_DENIED;
  default:
    return SK_AUTH_ERROR;
  }
}

/* ------------------------------------------------------------------ */
/*  Main authentication sequence  (FR-CONN-06)                         */
/* ------------------------------------------------------------------ */

SkAuthResult
sk_ssh_authenticate(SkSshConnection *conn, GError **error)
{
  g_return_val_if_fail(conn != NULL, SK_AUTH_ERROR);

  ssh_session session = sk_ssh_connection_get_session(conn);
  const SkSshOptions *opts = sk_ssh_connection_get_opts(conn);
  unsigned int methods_mask = opts->auth_methods;

  if (methods_mask == 0)
    methods_mask = SK_AUTH_METHOD_ALL;

  /* Query which methods the server supports. */
  int rc = ssh_userauth_none(session, NULL);
  if (rc == SSH_AUTH_SUCCESS)
  {
    /* Server accepted 'none' auth (unusual but valid). */
    return SK_AUTH_SUCCESS;
  }

  int server_methods = ssh_userauth_list(session, NULL);
  SkAuthResult result = SK_AUTH_DENIED;

  /* FR-CONN-06: try in order: agent, pubkey, password, kbd-interactive. */

  /* 1. SSH agent (FR-CONN-07) */
  if ((methods_mask & SK_AUTH_METHOD_AGENT) && (server_methods & SSH_AUTH_METHOD_PUBLICKEY))
  {
    result = try_agent_auth(session);
    if (result == SK_AUTH_SUCCESS)
      return SK_AUTH_SUCCESS;
  }

  /* 2. Public key from disk (FR-CONN-08) */
  if ((methods_mask & SK_AUTH_METHOD_PUBKEY) && (server_methods & SSH_AUTH_METHOD_PUBLICKEY))
  {
    result = try_pubkey_auth(session);
    if (result == SK_AUTH_SUCCESS)
      return SK_AUTH_SUCCESS;
  }

  /* 3. Password (FR-CONN-09) — disabled by default, enableable per host. */
  if ((methods_mask & SK_AUTH_METHOD_PASSWORD) && (server_methods & SSH_AUTH_METHOD_PASSWORD))
  {
    result = try_password_auth(session, conn);
    if (result == SK_AUTH_SUCCESS)
      return SK_AUTH_SUCCESS;
    if (result == SK_AUTH_CANCELLED)
    {
      g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_AUTH,
                          "Authentication cancelled by user");
      return SK_AUTH_CANCELLED;
    }
  }

  /* 4. Keyboard-interactive / MFA (FR-CONN-10) */
  if ((methods_mask & SK_AUTH_METHOD_KEYBOARD_INTERACTIVE) &&
      (server_methods & SSH_AUTH_METHOD_INTERACTIVE))
  {
    result = try_kbdint_auth(session, conn);
    if (result == SK_AUTH_SUCCESS)
      return SK_AUTH_SUCCESS;
    if (result == SK_AUTH_CANCELLED)
    {
      g_set_error_literal(error, SK_SSH_ERROR, SK_SSH_ERROR_AUTH,
                          "Authentication cancelled by user");
      return SK_AUTH_CANCELLED;
    }
  }

  /* FR-CONN-12: If user has only FIDO/U2K (SK) keys, we can't handle them. */

  /* FR-CONN-17: descriptive error message with guidance. */
  g_set_error(error, SK_SSH_ERROR, SK_SSH_ERROR_AUTH,
              "Authentication failed. Server supports methods: %s%s%s%s. "
              "Check your SSH agent, key files, or enable password auth.",
              (server_methods & SSH_AUTH_METHOD_PUBLICKEY) ? "publickey " : "",
              (server_methods & SSH_AUTH_METHOD_PASSWORD) ? "password " : "",
              (server_methods & SSH_AUTH_METHOD_INTERACTIVE) ? "keyboard-interactive " : "",
              (server_methods & SSH_AUTH_METHOD_HOSTBASED) ? "hostbased " : "");

  return SK_AUTH_DENIED;
}
