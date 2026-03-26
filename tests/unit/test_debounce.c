// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_debounce.c
 * @brief Unit tests for debounced state saving.
 *
 * Tests sk_state_debounce_new, sk_state_debounce_free,
 * sk_state_debounce_flush, and sk_state_schedule_save.
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

/* ---- Test: debounce new and free ---------------------------------------- */

static void
test_debounce_new_and_free(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateDebounce *db = sk_state_debounce_new(path, NULL);
  assert_non_null(db);

  sk_state_debounce_free(db);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: debounce free NULL safety ------------------------------------ */

static void
test_debounce_free_null(void **state)
{
  (void)state;
  sk_state_debounce_free(NULL); /* Should not crash. */
}

/* ---- Test: debounce flush with no pending ------------------------------- */

static void
test_debounce_flush_no_pending(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateDebounce *db = sk_state_debounce_new(path, NULL);
  assert_non_null(db);

  /* Flush with nothing pending should be a no-op. */
  sk_state_debounce_flush(db);

  /* File should not exist since we never scheduled a save. */
  assert_false(g_file_test(path, G_FILE_TEST_EXISTS));

  sk_state_debounce_free(db);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: debounce with fingerprint ------------------------------------ */

static void
test_debounce_new_with_fingerprint(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateDebounce *db = sk_state_debounce_new(path, "SHA256:testfp");
  assert_non_null(db);

  sk_state_debounce_free(db);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: schedule + flush writes file --------------------------------- */

static void
test_debounce_schedule_and_flush(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateDebounce *db = sk_state_debounce_new(path, NULL);
  assert_non_null(db);

  /* Schedule a state save. */
  SkStateFile *sf = sk_state_file_new("test-debounce");
  sk_state_schedule_save(db, sf);

  /* Flush should write immediately. */
  sk_state_debounce_flush(db);

  /* File should exist now. */
  assert_true(g_file_test(path, G_FILE_TEST_EXISTS));

  /* Verify it's valid state JSON. */
  GError *error = NULL;
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_null(error);
  assert_string_equal(loaded->client_id, "test-debounce");

  sk_state_file_free(loaded);
  sk_state_file_free(sf);
  sk_state_debounce_free(db);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: multiple schedules only latest is saved ---------------------- */

static void
test_debounce_latest_wins(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateDebounce *db = sk_state_debounce_new(path, NULL);
  assert_non_null(db);

  /* Schedule multiple saves — only the latest should survive. */
  SkStateFile *sf1 = sk_state_file_new("first");
  SkStateFile *sf2 = sk_state_file_new("second");
  SkStateFile *sf3 = sk_state_file_new("third");

  sk_state_schedule_save(db, sf1);
  sk_state_schedule_save(db, sf2);
  sk_state_schedule_save(db, sf3);

  sk_state_debounce_flush(db);

  /* The last scheduled state should be the one saved. */
  GError *error = NULL;
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_string_equal(loaded->client_id, "third");

  sk_state_file_free(loaded);
  sk_state_file_free(sf1);
  sk_state_file_free(sf2);
  sk_state_file_free(sf3);
  sk_state_debounce_free(db);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_debounce_new_and_free),
    cmocka_unit_test(test_debounce_free_null),
    cmocka_unit_test(test_debounce_flush_no_pending),
    cmocka_unit_test(test_debounce_new_with_fingerprint),
    cmocka_unit_test(test_debounce_schedule_and_flush),
    cmocka_unit_test(test_debounce_latest_wins),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
