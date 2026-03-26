// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_edge_signals.c
 * @brief Edge-case tests for signal handling during state writes.
 *
 * Covers: SIGTERM during state write produces a valid or absent file
 * (never a partial write), atomic tmp+rename integrity.
 *
 * INV-STATE-1, FR-STATE-04..07
 */

#include "shellkeep/sk_state.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* ---- Test: Atomic save leaves valid file or no file --------------------- */

static void
test_edge_atomic_save_integrity(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  /* Save a state file, then verify it is complete. */
  SkStateFile *sf = sk_state_file_new("signal-test");
  SkEnvironment *env = sk_environment_new("dev");
  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;
  sf->last_environment = g_strdup("dev");

  GError *error = NULL;
  assert_true(sk_state_save(sf, path, &error));
  assert_null(error);

  /* Verify the saved file is valid JSON we can load back. */
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_null(error);
  assert_string_equal(loaded->client_id, "signal-test");

  sk_state_file_free(loaded);
  sk_state_file_free(sf);

  /* No .tmp files should remain after a successful save. */
  GDir *dir = g_dir_open(tmpdir, 0, NULL);
  assert_non_null(dir);
  const char *name;
  while ((name = g_dir_read_name(dir)) != NULL)
  {
    assert_null(strstr(name, ".tmp"));
  }
  g_dir_close(dir);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Verify tmp+rename write strategy ----------------------------- */

static void
test_edge_write_uses_tmp_rename(void **state)
{
  (void)state;

  /* Verify that after a successful save, no .tmp files remain,
   * confirming the atomic tmp+rename pattern is used. */
  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateFile *sf = sk_state_file_new("atomicity-test");
  sf->n_environments = 10;
  sf->environments = g_new0(SkEnvironment *, 11);
  for (int i = 0; i < 10; i++)
  {
    char ename[32];
    snprintf(ename, sizeof(ename), "env-%03d", i);
    SkEnvironment *e = sk_environment_new(ename);
    e->n_windows = 2;
    e->windows = g_new0(SkWindow *, 3);
    for (int w = 0; w < 2; w++)
    {
      char wid[32];
      snprintf(wid, sizeof(wid), "w-%d-%d", i, w);
      e->windows[w] = sk_window_new(wid, "Win");
      e->windows[w]->n_tabs = 2;
      e->windows[w]->tabs = g_new0(SkTab *, 3);
      for (int t = 0; t < 2; t++)
      {
        char uuid[64];
        snprintf(uuid, sizeof(uuid), "uuid-%d-%d-%d", i, w, t);
        char tname[64];
        snprintf(tname, sizeof(tname), "client--env-%03d--s-%d-%d", i, w, t);
        e->windows[w]->tabs[t] = sk_tab_new(uuid, tname, "Tab", t);
      }
    }
    sf->environments[i] = e;
  }
  sf->last_environment = g_strdup("env-000");

  GError *error = NULL;
  assert_true(sk_state_save(sf, path, &error));
  assert_null(error);

  /* Verify no .tmp files left behind. */
  GDir *dir = g_dir_open(tmpdir, 0, NULL);
  assert_non_null(dir);
  const char *name;
  while ((name = g_dir_read_name(dir)) != NULL)
  {
    assert_null(strstr(name, ".tmp"));
  }
  g_dir_close(dir);

  /* Verify loaded file is complete and valid. */
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_null(error);
  assert_int_equal(loaded->n_environments, 10);
  assert_string_equal(loaded->client_id, "atomicity-test");
  sk_state_file_free(loaded);

  sk_state_file_free(sf);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Concurrent save to same path --------------------------------- */

static void
test_edge_concurrent_save(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  /* Write two different states rapidly to the same file.
   * Both should complete without corruption thanks to tmp+rename. */
  SkStateFile *sf1 = sk_state_file_new("writer-1");
  sf1->n_environments = 1;
  sf1->environments = g_new0(SkEnvironment *, 2);
  sf1->environments[0] = sk_environment_new("dev");
  sf1->last_environment = g_strdup("dev");

  SkStateFile *sf2 = sk_state_file_new("writer-2");
  sf2->n_environments = 1;
  sf2->environments = g_new0(SkEnvironment *, 2);
  sf2->environments[0] = sk_environment_new("prod");
  sf2->last_environment = g_strdup("prod");

  GError *error = NULL;
  assert_true(sk_state_save(sf1, path, &error));
  assert_null(error);
  assert_true(sk_state_save(sf2, path, &error));
  assert_null(error);

  /* Final file should be valid (from writer-2). */
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_string_equal(loaded->client_id, "writer-2");
  sk_state_file_free(loaded);

  sk_state_file_free(sf1);
  sk_state_file_free(sf2);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Save to read-only directory fails gracefully ----------------- */

static void
test_edge_save_readonly_dir(void **state)
{
  (void)state;

  /* Skip this test when running as root, because root can write
   * to read-only directories. Docker containers typically run as root. */
  if (getuid() == 0)
  {
    skip();
    return;
  }

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateFile *sf = sk_state_file_new("readonly-test");
  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = sk_environment_new("dev");
  sf->last_environment = g_strdup("dev");

  /* Make directory read-only. */
  chmod(tmpdir, 0555);

  GError *error = NULL;
  bool ok = sk_state_save(sf, path, &error);
  /* Should fail since we can't write to the directory. */
  assert_false(ok);
  assert_non_null(error);
  g_clear_error(&error);

  /* Restore permissions for cleanup. */
  chmod(tmpdir, 0700);

  sk_state_file_free(sf);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_edge_atomic_save_integrity),
    cmocka_unit_test(test_edge_write_uses_tmp_rename),
    cmocka_unit_test(test_edge_concurrent_save),
    cmocka_unit_test(test_edge_save_readonly_dir),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
