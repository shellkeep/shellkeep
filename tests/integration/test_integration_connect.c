// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_integration_connect.c
 * @brief Integration tests for SSH connect, auth, session create, disconnect.
 *
 * Requires the shellkeep test sshd container running on localhost:2222.
 * See tests/integration/Dockerfile.
 *
 * Environment variables:
 *   SK_TEST_SSH_HOST     (default: 127.0.0.1)
 *   SK_TEST_SSH_PORT     (default: 2222)
 *   SK_TEST_SSH_USER     (default: testuser)
 *   SK_TEST_SSH_PASSWORD (default: testpass)
 *
 * These tests are skipped if the SSH server is not reachable.
 */

#include "shellkeep/sk_log.h"
#include "shellkeep/sk_session.h"
#include "shellkeep/sk_ssh.h"

#include <glib.h>
#include <glib/gstdio.h>

/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <stdlib.h>
#include <string.h>

/* ---- Test environment --------------------------------------------------- */

static const char *
get_test_host(void)
{
  const char *h = g_getenv("SK_TEST_SSH_HOST");
  return h ? h : "127.0.0.1";
}

static int
get_test_port(void)
{
  const char *p = g_getenv("SK_TEST_SSH_PORT");
  return p ? atoi(p) : 2222;
}

static const char *
get_test_user(void)
{
  const char *u = g_getenv("SK_TEST_SSH_USER");
  return u ? u : "testuser";
}

static const char *
get_test_password(void)
{
  const char *pw = g_getenv("SK_TEST_SSH_PASSWORD");
  return pw ? pw : "testpass";
}

/* Password callback for tests. */
static char *
test_password_cb(SkSshConnection *conn, const char *prompt, gpointer user_data)
{
  (void)conn;
  (void)prompt;
  (void)user_data;
  return g_strdup(get_test_password());
}

/* Host key accept callback for tests (always accept — TOFU). */
static gboolean
test_host_key_cb(SkSshConnection *conn, const char *fingerprint, const char *key_type,
                 gpointer user_data)
{
  (void)conn;
  (void)fingerprint;
  (void)key_type;
  (void)user_data;
  return TRUE;
}

/* Check if test SSH server is reachable. */
static bool
ssh_server_available(void)
{
  GSocketClient *client = g_socket_client_new();
  g_socket_client_set_timeout(client, 2);

  char *addr = g_strdup_printf("%s:%d", get_test_host(), get_test_port());
  GSocketConnection *conn =
      g_socket_client_connect_to_host(client, addr, get_test_port(), NULL, NULL);
  g_free(addr);

  bool available = (conn != NULL);
  if (conn)
    g_object_unref(conn);
  g_object_unref(client);
  return available;
}

/* ---- Test: connect and disconnect --------------------------------------- */

static void
test_connect_and_disconnect(void **state)
{
  (void)state;

  if (!ssh_server_available())
  {
    skip();
    return;
  }

  /* Clear known_hosts to avoid HOST_KEY_CHANGED when container is recreated. */
  const char *home = g_get_home_dir();
  g_autofree char *kh = g_build_filename(home, ".ssh", "known_hosts", NULL);
  g_remove(kh);

  SkSshOptions opts = {
    .hostname = get_test_host(),
    .port = get_test_port(),
    .username = get_test_user(),
    .auth_methods = SK_AUTH_METHOD_PASSWORD,
    .connect_timeout = 10,
    .password_cb = test_password_cb,
    .host_key_unknown_cb = test_host_key_cb,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);
  assert_non_null(conn);
  assert_null(error);

  /* Connect (blocking). */
  gboolean ok = sk_ssh_connection_connect(conn, &error);
  assert_true(ok);
  assert_null(error);
  assert_true(sk_ssh_connection_is_connected(conn));

  /* Get fingerprint. */
  char *fp = sk_ssh_get_host_fingerprint(conn);
  assert_non_null(fp);
  g_free(fp);

  /* Disconnect. */
  sk_ssh_connection_disconnect(conn);
  assert_false(sk_ssh_connection_is_connected(conn));

  sk_ssh_connection_free(conn);
}

/* ---- Test: connect with wrong password ---------------------------------- */

static char *
wrong_password_cb(SkSshConnection *conn, const char *prompt, gpointer user_data)
{
  (void)conn;
  (void)prompt;
  (void)user_data;
  return g_strdup("wrongpassword");
}

static void
test_connect_wrong_password(void **state)
{
  (void)state;

  if (!ssh_server_available())
  {
    skip();
    return;
  }

  SkSshOptions opts = {
    .hostname = get_test_host(),
    .port = get_test_port(),
    .username = get_test_user(),
    .auth_methods = SK_AUTH_METHOD_PASSWORD,
    .connect_timeout = 10,
    .password_cb = wrong_password_cb,
    .host_key_unknown_cb = test_host_key_cb,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);
  assert_non_null(conn);

  gboolean ok = sk_ssh_connection_connect(conn, &error);
  assert_false(ok);
  assert_non_null(error);

  sk_ssh_connection_free(conn);
  g_clear_error(&error);
}

/* ---- Test: connect to unreachable host ---------------------------------- */

static void
test_connect_unreachable(void **state)
{
  (void)state;

  SkSshOptions opts = {
    .hostname = "192.0.2.1", /* TEST-NET — unreachable */
    .port = 22,
    .username = "nobody",
    .connect_timeout = 2,
    .host_key_unknown_cb = test_host_key_cb,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);
  if (conn == NULL)
  {
    /* Some implementations fail at new(). */
    g_clear_error(&error);
    return;
  }

  gboolean ok = sk_ssh_connection_connect(conn, &error);
  assert_false(ok);
  assert_non_null(error);

  sk_ssh_connection_free(conn);
  g_clear_error(&error);
}

/* ---- Test: double disconnect is safe ------------------------------------ */

static void
test_double_disconnect(void **state)
{
  (void)state;

  if (!ssh_server_available())
  {
    skip();
    return;
  }

  SkSshOptions opts = {
    .hostname = get_test_host(),
    .port = get_test_port(),
    .username = get_test_user(),
    .auth_methods = SK_AUTH_METHOD_PASSWORD,
    .connect_timeout = 10,
    .password_cb = test_password_cb,
    .host_key_unknown_cb = test_host_key_cb,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);
  assert_non_null(conn);

  sk_ssh_connection_connect(conn, &error);
  g_clear_error(&error);

  /* Double disconnect should not crash. */
  sk_ssh_connection_disconnect(conn);
  sk_ssh_connection_disconnect(conn);

  sk_ssh_connection_free(conn);
}

/* ---- Test: free NULL is safe -------------------------------------------- */

static void
test_free_null(void **state)
{
  (void)state;
  sk_ssh_connection_free(NULL); /* Should not crash. */
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  sk_log_init(false, false, NULL);

  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_connect_and_disconnect),
    cmocka_unit_test(test_connect_wrong_password),
    cmocka_unit_test(test_connect_unreachable),
    cmocka_unit_test(test_double_disconnect),
    cmocka_unit_test(test_free_null),
  };

  int rc = cmocka_run_group_tests(tests, NULL, NULL);
  sk_log_shutdown();
  return rc;
}
