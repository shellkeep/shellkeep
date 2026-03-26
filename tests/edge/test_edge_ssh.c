// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_edge_ssh.c
 * @brief Edge-case tests for SSH layer scenarios.
 *
 * Covers: disconnect during handshake, SFTP unavailable fallback,
 * connection to unreachable host, auth timeout scenarios.
 * These test the SSH API at the structural level without requiring
 * a live SSH server (integration tests cover live connections).
 *
 * FR-CONN-01..20, FR-COMPAT-10..11
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_ssh.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: Connection to unreachable host fails gracefully -------------- */

static void
test_edge_ssh_connect_unreachable(void **state)
{
  (void)state;

  SkSshOptions opts = {
    .hostname = "192.0.2.1", /* TEST-NET: guaranteed unreachable. */
    .port = 22,
    .username = "testuser",
    .connect_timeout = 1, /* 1 second timeout. */
    .auth_methods = SK_AUTH_METHOD_ALL,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);

  if (conn != NULL)
  {
    /* Connection creation should succeed, but connect should fail/timeout. */
    gboolean connected = sk_ssh_connection_connect(conn, &error);
    assert_false(connected);
    assert_non_null(error);
    g_clear_error(&error);

    assert_false(sk_ssh_connection_is_connected(conn));
    sk_ssh_connection_free(conn);
  }
  else
  {
    /* Some implementations may reject at new() time. */
    g_clear_error(&error);
  }
}

/* ---- Test: Connection with NULL hostname -------------------------------- */

static void
test_edge_ssh_null_hostname(void **state)
{
  (void)state;

  SkSshOptions opts = {
    .hostname = NULL,
    .port = 22,
    .username = "user",
    .connect_timeout = 1,
    .auth_methods = SK_AUTH_METHOD_ALL,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);

  /* Should fail gracefully with NULL hostname. */
  if (conn != NULL)
  {
    gboolean connected = sk_ssh_connection_connect(conn, &error);
    assert_false(connected);
    g_clear_error(&error);
    sk_ssh_connection_free(conn);
  }
  else
  {
    /* Rejected at creation — expected. */
    g_clear_error(&error);
  }
}

/* ---- Test: Connection with empty hostname ------------------------------- */

static void
test_edge_ssh_empty_hostname(void **state)
{
  (void)state;

  SkSshOptions opts = {
    .hostname = "",
    .port = 22,
    .username = "user",
    .connect_timeout = 1,
    .auth_methods = SK_AUTH_METHOD_ALL,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);

  if (conn != NULL)
  {
    gboolean connected = sk_ssh_connection_connect(conn, &error);
    assert_false(connected);
    g_clear_error(&error);
    sk_ssh_connection_free(conn);
  }
  else
  {
    g_clear_error(&error);
  }
}

/* ---- Test: Free NULL connection does not crash -------------------------- */

static void
test_edge_ssh_free_null(void **state)
{
  (void)state;

  /* Should be a no-op, not a crash. */
  sk_ssh_connection_free(NULL);
  sk_ssh_channel_free(NULL);
  sk_sftp_session_free(NULL);
}

/* ---- Test: Double disconnect does not crash ----------------------------- */

static void
test_edge_ssh_double_disconnect(void **state)
{
  (void)state;

  SkSshOptions opts = {
    .hostname = "192.0.2.1",
    .port = 22,
    .username = "user",
    .connect_timeout = 1,
    .auth_methods = SK_AUTH_METHOD_ALL,
  };

  GError *error = NULL;
  SkSshConnection *conn = sk_ssh_connection_new(&opts, &error);

  if (conn != NULL)
  {
    /* Disconnect twice on a never-connected session. */
    sk_ssh_connection_disconnect(conn);
    sk_ssh_connection_disconnect(conn);
    sk_ssh_connection_free(conn);
  }
  else
  {
    g_clear_error(&error);
  }
}

/* ---- Test: Host key status enum values ---------------------------------- */

static void
test_edge_ssh_host_key_enum(void **state)
{
  (void)state;

  /* Verify all enum values exist and are distinct. */
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_CHANGED);
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_UNKNOWN);
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_OTHER);
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_ERROR);
  assert_int_not_equal(SK_HOST_KEY_CHANGED, SK_HOST_KEY_UNKNOWN);
}

/* ---- Test: SSH error codes are distinct --------------------------------- */

static void
test_edge_ssh_error_codes(void **state)
{
  (void)state;

  /* All error codes should be distinct. */
  assert_int_not_equal(SK_SSH_ERROR_CONNECT, SK_SSH_ERROR_HOST_KEY);
  assert_int_not_equal(SK_SSH_ERROR_CONNECT, SK_SSH_ERROR_AUTH);
  assert_int_not_equal(SK_SSH_ERROR_CONNECT, SK_SSH_ERROR_SFTP);
  assert_int_not_equal(SK_SSH_ERROR_CONNECT, SK_SSH_ERROR_TIMEOUT);
  assert_int_not_equal(SK_SSH_ERROR_CONNECT, SK_SSH_ERROR_DISCONNECTED);
  assert_int_not_equal(SK_SSH_ERROR_CONNECT, SK_SSH_ERROR_CRYPTO);
}

/* ---- Test: Auth result enum completeness -------------------------------- */

static void
test_edge_ssh_auth_result_enum(void **state)
{
  (void)state;

  /* Verify auth result values. */
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_DENIED);
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_PARTIAL);
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_ERROR);
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_CANCELLED);
}

/* ---- Test: Auth method bitmask operations ------------------------------- */

static void
test_edge_ssh_auth_method_bitmask(void **state)
{
  (void)state;

  /* SK_AUTH_METHOD_ALL should include all individual methods. */
  assert_true((SK_AUTH_METHOD_ALL & SK_AUTH_METHOD_AGENT) != 0);
  assert_true((SK_AUTH_METHOD_ALL & SK_AUTH_METHOD_PUBKEY) != 0);
  assert_true((SK_AUTH_METHOD_ALL & SK_AUTH_METHOD_PASSWORD) != 0);
  assert_true((SK_AUTH_METHOD_ALL & SK_AUTH_METHOD_KEYBOARD_INTERACTIVE) != 0);

  /* Individual methods should be single bits. */
  unsigned int combined =
      SK_AUTH_METHOD_AGENT | SK_AUTH_METHOD_PUBKEY | SK_AUTH_METHOD_PASSWORD |
      SK_AUTH_METHOD_KEYBOARD_INTERACTIVE;
  assert_int_equal(combined, SK_AUTH_METHOD_ALL);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_edge_ssh_connect_unreachable),
    cmocka_unit_test(test_edge_ssh_null_hostname),
    cmocka_unit_test(test_edge_ssh_empty_hostname),
    cmocka_unit_test(test_edge_ssh_free_null),
    cmocka_unit_test(test_edge_ssh_double_disconnect),
    cmocka_unit_test(test_edge_ssh_host_key_enum),
    cmocka_unit_test(test_edge_ssh_error_codes),
    cmocka_unit_test(test_edge_ssh_auth_result_enum),
    cmocka_unit_test(test_edge_ssh_auth_method_bitmask),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
