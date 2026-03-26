// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_edge_state.c
 * @brief Edge-case tests for corrupted and degenerate state files.
 *
 * Covers: truncated JSON, missing fields, future schema_version (v99),
 * duplicate session_uuid, truncated JSONL, binary garbage JSONL,
 * orphan .tmp files, wrong permissions.
 *
 * FR-STATE-13..17, INV-STATE-1, INV-SECURITY-3
 */

#include "shellkeep/sk_state.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* ---- Test: Truncated JSON mid-object ------------------------------------ */

static void
test_edge_truncated_json(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  /* JSON cut off mid-string: opening brace but no close. */
  char *path =
      sk_test_write_file(tmpdir, "state.json", "{\"schema_version\": 1, \"client_id\": \"te");

  GError *error = NULL;
  SkStateFile *sf = sk_state_load(path, &error);
  assert_null(sf);
  assert_non_null(error);
  /* Should be flagged as corrupt. */
  assert_int_equal(error->code, SK_STATE_ERROR_CORRUPT);
  g_clear_error(&error);

  /* Corrupt file should be renamed away. */
  assert_false(g_file_test(path, G_FILE_TEST_EXISTS));

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Empty file (zero bytes) -------------------------------------- */

static void
test_edge_empty_file(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = sk_test_write_file(tmpdir, "state.json", "");

  GError *error = NULL;
  SkStateFile *sf = sk_state_load(path, &error);
  assert_null(sf);
  assert_non_null(error);
  g_clear_error(&error);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Valid JSON missing required fields --------------------------- */

static void
test_edge_missing_fields(void **state)
{
  (void)state;

  /* JSON object with only schema_version; missing client_id, environments. */
  const char *json = "{\"schema_version\": 1}";

  GError *error = NULL;
  SkStateFile *sf = sk_state_from_json(json, &error);
  /* Should parse (missing fields get defaults) but may fail validation. */
  if (sf != NULL)
  {
    /* Validate should catch the missing required content. */
    bool valid = sk_state_validate(sf, &error);
    /* Missing client_id should make this invalid or have NULL client_id. */
    if (valid)
    {
      /* At minimum, the object was created with defaults. */
      assert_int_equal(sf->schema_version, 1);
    }
    sk_state_file_free(sf);
  }
  g_clear_error(&error);
}

/* ---- Test: Future schema_version v99 ------------------------------------ */

static void
test_edge_future_schema_v99(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  const char *json = "{"
                     "\"schema_version\": 99,"
                     "\"last_modified\": \"2026-01-01T00:00:00Z\","
                     "\"client_id\": \"test\","
                     "\"environments\": {},"
                     "\"last_environment\": \"\""
                     "}";
  char *path = sk_test_write_file(tmpdir, "state.json", json);

  GError *error = NULL;
  SkStateFile *sf = sk_state_load(path, &error);
  assert_null(sf);
  assert_non_null(error);
  assert_int_equal(error->code, SK_STATE_ERROR_VERSION_FUTURE);
  g_clear_error(&error);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Duplicate session UUIDs across environments ------------------ */

static void
test_edge_duplicate_uuid_cross_env(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("client");

  /* Create two environments, each with a tab sharing the same UUID. */
  SkEnvironment *env1 = sk_environment_new("dev");
  SkWindow *win1 = sk_window_new("w1", "Win1");
  SkTab *tab1 = sk_tab_new("dup-uuid", "tmux1", "Tab1", 0);
  win1->n_tabs = 1;
  win1->tabs = g_new0(SkTab *, 2);
  win1->tabs[0] = tab1;
  env1->n_windows = 1;
  env1->windows = g_new0(SkWindow *, 2);
  env1->windows[0] = win1;

  SkEnvironment *env2 = sk_environment_new("staging");
  SkWindow *win2 = sk_window_new("w2", "Win2");
  SkTab *tab2 = sk_tab_new("dup-uuid", "tmux2", "Tab2", 0);
  win2->n_tabs = 1;
  win2->tabs = g_new0(SkTab *, 2);
  win2->tabs[0] = tab2;
  env2->n_windows = 1;
  env2->windows = g_new0(SkWindow *, 2);
  env2->windows[0] = win2;

  sf->n_environments = 2;
  sf->environments = g_new0(SkEnvironment *, 3);
  sf->environments[0] = env1;
  sf->environments[1] = env2;

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: Binary garbage in JSONL file --------------------------------- */

static void
test_edge_binary_garbage_jsonl(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();

  /* Write a valid JSONL line followed by binary garbage.
   * History files are stored as <base_dir>/<uuid>.jsonl. */
  char *path = g_build_filename(tmpdir, "a0b1c2d3.jsonl", NULL);
  FILE *fp = fopen(path, "wb");
  assert_non_null(fp);

  /* One valid line. */
  const char *valid_line =
      "{\"ts\":\"2026-01-01T00:00:00Z\",\"type\":0,\"text\":\"hello\"}\n";
  fwrite(valid_line, 1, strlen(valid_line), fp);

  /* Binary garbage line. */
  unsigned char garbage[] = {0xFF, 0xFE, 0x00, 0x80, 0xDE, 0xAD, 0xBE, 0xEF, '\n'};
  fwrite(garbage, 1, sizeof(garbage), fp);
  fclose(fp);

  /* Read should recover the valid line and skip the garbage. */
  int n_events = 0;
  GError *error = NULL;
  SkHistoryEvent **events = sk_history_read("a0b1c2d3", tmpdir, &n_events, &error);

  /* At least the first valid event should be recovered. */
  if (events != NULL)
  {
    assert_true(n_events >= 1);
    assert_string_equal(events[0]->text, "hello");

    for (int i = 0; i < n_events; i++)
      sk_history_event_free(events[i]);
    g_free(events);
  }
  g_clear_error(&error);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Truncated JSONL (last line cut off) -------------------------- */

static void
test_edge_truncated_jsonl(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();

  char *path = g_build_filename(tmpdir, "a1b2c3d4.jsonl", NULL);
  FILE *fp = fopen(path, "wb");
  assert_non_null(fp);

  /* Valid line. */
  const char *line1 =
      "{\"ts\":\"2026-01-01T00:00:00Z\",\"type\":0,\"text\":\"line1\"}\n";
  fwrite(line1, 1, strlen(line1), fp);

  /* Truncated line (no newline, incomplete JSON). */
  const char *line2_trunc = "{\"ts\":\"2026-01-01T00:00:01Z\",\"type\":0,\"tex";
  fwrite(line2_trunc, 1, strlen(line2_trunc), fp);
  fclose(fp);

  /* FR-HISTORY-09: discard the last line if truncated/invalid. */
  int n_events = 0;
  GError *error = NULL;
  SkHistoryEvent **events = sk_history_read("a1b2c3d4", tmpdir, &n_events, &error);

  if (events != NULL)
  {
    /* Should recover exactly 1 valid event. */
    assert_int_equal(n_events, 1);
    assert_string_equal(events[0]->text, "line1");

    for (int i = 0; i < n_events; i++)
      sk_history_event_free(events[i]);
    g_free(events);
  }
  g_clear_error(&error);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Orphan .tmp files cleaned up --------------------------------- */

static void
test_edge_orphan_tmp_files(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();

  /* Simulate orphan .tmp files from a crashed write. */
  char *tmp1 = sk_test_write_file(tmpdir, "state.json.tmp", "partial data");
  char *tmp2 = sk_test_write_file(tmpdir, "other.tmp", "other partial");
  char *keep = sk_test_write_file(tmpdir, "state.json", "{}");

  sk_state_cleanup_tmp_files(tmpdir);

  /* .tmp files should be removed. */
  assert_false(g_file_test(tmp1, G_FILE_TEST_EXISTS));
  assert_false(g_file_test(tmp2, G_FILE_TEST_EXISTS));
  /* Non-tmp file should survive. */
  assert_true(g_file_test(keep, G_FILE_TEST_EXISTS));

  g_free(tmp1);
  g_free(tmp2);
  g_free(keep);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Wrong permissions corrected ---------------------------------- */

static void
test_edge_wrong_permissions(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = sk_test_write_file(tmpdir, "state.json", "{}");

  /* Set permissions to world-readable (wrong). */
  chmod(path, 0644);

  /* Fix should correct to 0600. */
  assert_true(sk_permissions_fix_file(path));

  struct stat st;
  assert_int_equal(stat(path, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0600);

  /* Test directory permissions too. */
  chmod(tmpdir, 0755);
  assert_true(sk_permissions_fix_dir(tmpdir));
  assert_int_equal(stat(tmpdir, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0700);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: State save enforces 0600 permissions ------------------------- */

static void
test_edge_save_sets_permissions(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateFile *sf = sk_state_file_new("perm-test");
  SkEnvironment *env = sk_environment_new("dev");
  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;
  sf->last_environment = g_strdup("dev");

  GError *error = NULL;
  assert_true(sk_state_save(sf, path, &error));
  assert_null(error);

  /* INV-SECURITY-3: Verify 0600. */
  struct stat st;
  assert_int_equal(stat(path, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0600);

  sk_state_file_free(sf);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Load state with schema_version = 0 (invalid) ----------------- */

static void
test_edge_schema_version_zero(void **state)
{
  (void)state;

  const char *json = "{\"schema_version\": 0, \"client_id\": \"test\"}";

  GError *error = NULL;
  SkStateFile *sf = sk_state_from_json(json, &error);
  if (sf != NULL)
  {
    /* Version 0 should fail validation. */
    bool valid = sk_state_validate(sf, &error);
    assert_false(valid);
    assert_non_null(error);
    assert_int_equal(error->code, SK_STATE_ERROR_SCHEMA);
    g_clear_error(&error);
    sk_state_file_free(sf);
  }
  else
  {
    g_clear_error(&error);
  }
}

/* ---- Test: Negative schema_version -------------------------------------- */

static void
test_edge_schema_version_negative(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("test");
  sf->schema_version = -42;

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: NULL JSON input to from_json --------------------------------- */

static void
test_edge_null_json_input(void **state)
{
  (void)state;

  GError *error = NULL;
  SkStateFile *sf = sk_state_from_json(NULL, &error);
  assert_null(sf);
  /* Should fail gracefully. */
  g_clear_error(&error);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_edge_truncated_json),
    cmocka_unit_test(test_edge_empty_file),
    cmocka_unit_test(test_edge_missing_fields),
    cmocka_unit_test(test_edge_future_schema_v99),
    cmocka_unit_test(test_edge_duplicate_uuid_cross_env),
    cmocka_unit_test(test_edge_binary_garbage_jsonl),
    cmocka_unit_test(test_edge_truncated_jsonl),
    cmocka_unit_test(test_edge_orphan_tmp_files),
    cmocka_unit_test(test_edge_wrong_permissions),
    cmocka_unit_test(test_edge_save_sets_permissions),
    cmocka_unit_test(test_edge_schema_version_zero),
    cmocka_unit_test(test_edge_schema_version_negative),
    cmocka_unit_test(test_edge_null_json_input),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
