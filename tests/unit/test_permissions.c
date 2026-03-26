// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_permissions.c
 * @brief Unit tests for file/directory permission enforcement.
 *
 * Tests INV-SECURITY-3, NFR-SEC-01..03: sk_permissions_fix_file,
 * sk_permissions_fix_dir, and auto-correction of wrong permissions.
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

/* ---- Test: fix_file sets 0600 ------------------------------------------- */

static void
test_permissions_fix_file(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = sk_test_write_file(tmpdir, "testfile", "data");

  /* Set overly permissive permissions first. */
  chmod(path, 0644);

  assert_true(sk_permissions_fix_file(path));

  struct stat st;
  assert_int_equal(stat(path, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0600);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: fix_file on already correct perms ----------------------------- */

static void
test_permissions_fix_file_already_correct(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = sk_test_write_file(tmpdir, "testfile", "data");
  chmod(path, 0600);

  assert_true(sk_permissions_fix_file(path));

  struct stat st;
  assert_int_equal(stat(path, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0600);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: fix_file on nonexistent file --------------------------------- */

static void
test_permissions_fix_file_nonexistent(void **state)
{
  (void)state;

  /* Nonexistent file should return true (no-op). */
  assert_true(sk_permissions_fix_file("/tmp/sk_nonexistent_file_perm_test"));
}

/* ---- Test: fix_dir sets 0700 -------------------------------------------- */

static void
test_permissions_fix_dir(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();

  /* Set overly permissive. */
  chmod(tmpdir, 0755);

  assert_true(sk_permissions_fix_dir(tmpdir));

  struct stat st;
  assert_int_equal(stat(tmpdir, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0700);

  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: fix_dir on already correct perms ------------------------------ */

static void
test_permissions_fix_dir_already_correct(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  chmod(tmpdir, 0700);

  assert_true(sk_permissions_fix_dir(tmpdir));

  struct stat st;
  assert_int_equal(stat(tmpdir, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0700);

  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: verify_and_fix runs without crash ----------------------------- */

static void
test_permissions_verify_and_fix(void **state)
{
  (void)state;

  /* This touches actual XDG dirs but should not crash. */
  bool ok = sk_permissions_verify_and_fix();
  assert_true(ok);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_permissions_fix_file),
    cmocka_unit_test(test_permissions_fix_file_already_correct),
    cmocka_unit_test(test_permissions_fix_file_nonexistent),
    cmocka_unit_test(test_permissions_fix_dir),
    cmocka_unit_test(test_permissions_fix_dir_already_correct),
    cmocka_unit_test(test_permissions_verify_and_fix),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
