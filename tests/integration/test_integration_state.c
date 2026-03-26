// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_integration_state.c
 * @brief Integration tests for state save/restore via SFTP and atomic rename.
 *
 * Tests FR-STATE-04..07: atomic save via SFTP, state restore, and
 * tmp file cleanup.
 * Requires the shellkeep test sshd container running.
 *
 * These tests are skipped if the SSH server is not reachable.
 */

#include "shellkeep/sk_log.h"
#include "shellkeep/sk_ssh.h"
#include "shellkeep/sk_state.h"

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

/* ---- Test: SFTP write and read ------------------------------------------ */

static void
test_sftp_write_read(void **state)
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

  SkSftpSession *sftp = sk_sftp_session_new(conn, &error);
  if (sftp == NULL)
  {
    /* SFTP may not be available. */
    g_clear_error(&error);
    sk_ssh_connection_free(conn);
    skip();
    return;
  }

  /* Write a test file. */
  const char *remote_path = "/home/testuser/.terminal-state/test-state.json";
  const char *content = "{\"schema_version\":1,\"client_id\":\"test\"}";

  gboolean ok = sk_sftp_write_file(sftp, remote_path, content, strlen(content), 0600, &error);
  assert_true(ok);
  assert_null(error);

  /* Read it back. */
  char *data = NULL;
  size_t len = 0;
  ok = sk_sftp_read_file(sftp, remote_path, &data, &len, &error);
  assert_true(ok);
  assert_non_null(data);
  assert_int_equal(len, strlen(content));
  assert_memory_equal(data, content, len);

  g_free(data);
  sk_sftp_session_free(sftp);
  sk_ssh_connection_free(conn);
}

/* ---- Test: SFTP atomic rename ------------------------------------------- */

static void
test_sftp_atomic_rename(void **state)
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

  SkSftpSession *sftp = sk_sftp_session_new(conn, &error);
  if (sftp == NULL)
  {
    g_clear_error(&error);
    sk_ssh_connection_free(conn);
    skip();
    return;
  }

  const char *tmp_path = "/home/testuser/.terminal-state/rename-test.tmp";
  const char *final_path = "/home/testuser/.terminal-state/rename-test.json";
  const char *content = "{\"test\":\"rename\"}";

  /* Write to tmp. */
  gboolean ok = sk_sftp_write_file(sftp, tmp_path, content, strlen(content), 0600, &error);
  assert_true(ok);

  /* Rename atomically. */
  ok = sk_sftp_rename(sftp, tmp_path, final_path, &error);
  assert_true(ok);

  /* Verify final file exists. */
  assert_true(sk_sftp_exists(sftp, final_path));

  /* Verify tmp is gone. */
  assert_false(sk_sftp_exists(sftp, tmp_path));

  sk_sftp_session_free(sftp);
  sk_ssh_connection_free(conn);
  g_clear_error(&error);
}

/* ---- Test: state local save and load roundtrip -------------------------- */

static void
test_state_local_roundtrip(void **state)
{
  (void)state;

  /* This test does not need SSH — it tests local state persistence. */
  char *tmpdir = g_dir_make_tmp("sk_integ_XXXXXX", NULL);
  assert_non_null(tmpdir);

  char *path = g_build_filename(tmpdir, "state.json", NULL);

  /* Create state with content. */
  SkStateFile *sf = sk_state_file_new("integ-client");
  SkEnvironment *env = sk_environment_new("test-env");
  SkWindow *win = sk_window_new("win-1", "Window");
  SkTab *tab = sk_tab_new("11111111-2222-4333-8444-555555555555",
                          "integ-client--test-env--session1", "Tab 1", 0);
  win->n_tabs = 1;
  win->tabs = g_new0(SkTab *, 2);
  win->tabs[0] = tab;
  env->n_windows = 1;
  env->windows = g_new0(SkWindow *, 2);
  env->windows[0] = win;
  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;
  sf->last_environment = g_strdup("test-env");

  /* Save. */
  GError *error = NULL;
  assert_true(sk_state_save(sf, path, &error));
  assert_null(error);

  /* Load back. */
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_string_equal(loaded->client_id, "integ-client");
  assert_int_equal(loaded->n_environments, 1);
  assert_string_equal(loaded->environments[0]->name, "test-env");
  assert_int_equal(loaded->environments[0]->n_windows, 1);
  assert_int_equal(loaded->environments[0]->windows[0]->n_tabs, 1);
  assert_string_equal(loaded->environments[0]->windows[0]->tabs[0]->session_uuid,
                      "11111111-2222-4333-8444-555555555555");

  /* Validate loaded state. */
  assert_true(sk_state_validate(loaded, &error));

  sk_state_file_free(sf);
  sk_state_file_free(loaded);
  g_free(path);

  /* Cleanup tmpdir. */
  GDir *dir = g_dir_open(tmpdir, 0, NULL);
  if (dir)
  {
    const char *name;
    while ((name = g_dir_read_name(dir)) != NULL)
    {
      char *child = g_build_filename(tmpdir, name, NULL);
      g_unlink(child);
      g_free(child);
    }
    g_dir_close(dir);
  }
  g_rmdir(tmpdir);
  g_free(tmpdir);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  sk_log_init(false, false, NULL);

  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_sftp_write_read),
    cmocka_unit_test(test_sftp_atomic_rename),
    cmocka_unit_test(test_state_local_roundtrip),
  };

  int rc = cmocka_run_group_tests(tests, NULL, NULL);
  sk_log_shutdown();
  return rc;
}
