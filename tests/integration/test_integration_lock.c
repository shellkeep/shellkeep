// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_integration_lock.c
 * @brief Integration tests for the lock mechanism.
 *
 * Tests FR-LOCK-01..10: acquire, conflict detection, orphan detection.
 * Requires the shellkeep test sshd container running.
 *
 * These tests are skipped if the SSH server is not reachable.
 */

#include "shellkeep/sk_compat.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_session.h"
#include "shellkeep/sk_ssh.h"

#include <glib.h>

/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* ---- Test environment (same as connect tests) --------------------------- */

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

static char *
test_password_cb(SkSshConnection *conn, const char *prompt, gpointer user_data)
{
  (void)conn;
  (void)prompt;
  (void)user_data;
  return g_strdup(get_test_password());
}

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

static bool
ssh_server_available(void)
{
  GSocketClient *client = g_socket_client_new();
  g_socket_client_set_timeout(client, 2);
  char *addr = g_strdup_printf("%s:%d", get_test_host(), get_test_port());
  GSocketConnection *conn =
      g_socket_client_connect_to_host(client, addr, get_test_port(), NULL, NULL);
  g_free(addr);
  bool ok = (conn != NULL);
  if (conn)
    g_object_unref(conn);
  g_object_unref(client);
  return ok;
}

static SkSshConnection *
make_connection(GError **error)
{
  SkSshOptions opts = {
    .hostname = get_test_host(),
    .port = get_test_port(),
    .username = get_test_user(),
    .auth_methods = SK_AUTH_METHOD_PASSWORD,
    .connect_timeout = 10,
    .password_cb = test_password_cb,
    .host_key_unknown_cb = test_host_key_cb,
  };
  SkSshConnection *conn = sk_ssh_connection_new(&opts, error);
  if (conn == NULL)
    return NULL;
  if (!sk_ssh_connection_connect(conn, error))
  {
    sk_ssh_connection_free(conn);
    return NULL;
  }
  return conn;
}

/* ---- Test: lock acquire and release ------------------------------------- */

static void
test_lock_acquire_release(void **state)
{
  (void)state;

  if (!ssh_server_available())
  {
    skip();
    return;
  }

  GError *error = NULL;
  SkSshConnection *conn = make_connection(&error);
  if (conn == NULL)
  {
    g_clear_error(&error);
    skip();
    return;
  }

  SkSessionManager *mgr = sk_session_manager_new(conn);
  assert_non_null(mgr);

  const char *client_id = "test-lock-client";
  char hostname[256];
  gethostname(hostname, sizeof(hostname));

  /* Acquire lock. */
  bool ok = sk_lock_acquire(mgr, client_id, hostname, &error);
  assert_true(ok);
  assert_null(error);

  /* Check lock exists. */
  SkLockInfo *info = sk_lock_check(mgr, client_id, &error);
  assert_non_null(info);
  assert_true(info->valid);
  assert_string_equal(info->client_id, client_id);
  sk_lock_info_free(info);

  /* Release lock. */
  ok = sk_lock_release(mgr, client_id, &error);
  assert_true(ok);

  /* Lock should be gone. */
  info = sk_lock_check(mgr, client_id, &error);
  assert_null(info);

  sk_session_manager_free(mgr);
  sk_ssh_connection_free(conn);
  g_clear_error(&error);
}

/* ---- Test: lock conflict ------------------------------------------------ */

static void
test_lock_conflict(void **state)
{
  (void)state;

  if (!ssh_server_available())
  {
    skip();
    return;
  }

  GError *error = NULL;
  SkSshConnection *conn = make_connection(&error);
  if (conn == NULL)
  {
    g_clear_error(&error);
    skip();
    return;
  }

  SkSessionManager *mgr = sk_session_manager_new(conn);
  assert_non_null(mgr);

  const char *client_id = "test-conflict-client";

  /* First acquire should succeed. */
  bool ok = sk_lock_acquire(mgr, client_id, "host1", &error);
  assert_true(ok);
  g_clear_error(&error);

  /* Second acquire should fail with conflict. */
  ok = sk_lock_acquire(mgr, client_id, "host2", &error);
  assert_false(ok);
  assert_non_null(error);
  assert_int_equal(error->code, SK_SESSION_ERROR_LOCK_CONFLICT);
  g_clear_error(&error);

  /* Cleanup. */
  sk_lock_release(mgr, client_id, &error);
  g_clear_error(&error);

  sk_session_manager_free(mgr);
  sk_ssh_connection_free(conn);
}

/* ---- Test: orphan detection --------------------------------------------- */

static void
test_lock_orphan_detection(void **state)
{
  (void)state;

  /* Test the orphan detection logic without requiring actual orphans.
   * We test sk_lock_is_orphaned with synthetic data. */

  SkLockInfo info = {
    .client_id = g_strdup("test"),
    .hostname = g_strdup("host"),
    .connected_at = g_strdup("2020-01-01T00:00:00Z"), /* Very old. */
    .pid = g_strdup("1234"),
    .valid = true,
    .orphaned = false,
  };

  /* With a keepalive_timeout of 45s and connected_at in the past,
   * the lock should be considered orphaned. */
  bool orphaned = sk_lock_is_orphaned(&info, 45);
  assert_true(orphaned);

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.connected_at);
  g_free(info.pid);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  sk_log_init(false, false, NULL);

  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_lock_acquire_release),
    cmocka_unit_test(test_lock_conflict),
    cmocka_unit_test(test_lock_orphan_detection),
  };

  int rc = cmocka_run_group_tests(tests, NULL, NULL);
  sk_log_shutdown();
  return rc;
}
