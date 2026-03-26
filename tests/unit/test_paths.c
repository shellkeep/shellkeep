// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_paths.c
 * @brief Unit tests for XDG path functions.
 *
 * Tests NFR-XDG-01..05, FR-STATE-01: path construction,
 * directory creation with 0700, server cache paths.
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

/* ---- Test: sk_paths_config_dir ------------------------------------------ */

static void
test_paths_config_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_config_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/shellkeep"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  /* Verify permissions are 0700. */
  struct stat st;
  assert_int_equal(stat(dir, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0700);

  g_free(dir);
}

/* ---- Test: sk_paths_data_dir -------------------------------------------- */

static void
test_paths_data_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_data_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/shellkeep"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  g_free(dir);
}

/* ---- Test: sk_paths_state_dir ------------------------------------------- */

static void
test_paths_state_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_state_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/shellkeep"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  g_free(dir);
}

/* ---- Test: sk_paths_runtime_dir ----------------------------------------- */

static void
test_paths_runtime_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_runtime_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/shellkeep"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  g_free(dir);
}

/* ---- Test: sk_paths_cache_dir ------------------------------------------- */

static void
test_paths_cache_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_cache_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/shellkeep"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  g_free(dir);
}

/* ---- Test: sk_paths_server_cache_dir ------------------------------------ */

static void
test_paths_server_cache_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_server_cache_dir("SHA256-test-fingerprint");
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/SHA256-test-fingerprint"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  struct stat st;
  assert_int_equal(stat(dir, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0700);

  g_free(dir);
}

/* ---- Test: sk_paths_logs_dir -------------------------------------------- */

static void
test_paths_logs_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_logs_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/logs"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  g_free(dir);
}

/* ---- Test: sk_paths_crashes_dir ----------------------------------------- */

static void
test_paths_crashes_dir(void **state)
{
  (void)state;

  char *dir = sk_paths_crashes_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/crashes"));
  assert_true(g_file_test(dir, G_FILE_TEST_IS_DIR));

  g_free(dir);
}

/* ---- Test: idempotent calls --------------------------------------------- */

static void
test_paths_idempotent(void **state)
{
  (void)state;

  /* Calling twice should return the same path and not error. */
  char *dir1 = sk_paths_config_dir();
  char *dir2 = sk_paths_config_dir();
  assert_non_null(dir1);
  assert_non_null(dir2);
  assert_string_equal(dir1, dir2);

  g_free(dir1);
  g_free(dir2);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_paths_config_dir),
    cmocka_unit_test(test_paths_data_dir),
    cmocka_unit_test(test_paths_state_dir),
    cmocka_unit_test(test_paths_runtime_dir),
    cmocka_unit_test(test_paths_cache_dir),
    cmocka_unit_test(test_paths_server_cache_dir),
    cmocka_unit_test(test_paths_logs_dir),
    cmocka_unit_test(test_paths_crashes_dir),
    cmocka_unit_test(test_paths_idempotent),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
