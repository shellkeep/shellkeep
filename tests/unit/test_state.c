// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_state.c
 * @brief Unit tests for the state persistence layer.
 *
 * Tests JSON parse/serialize, migration, corruption handling,
 * and invariant validation (FR-STATE-04..17, INV-STATE-1).
 *
 * NFR-BUILD-03..05
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

/* ---- Test: sk_state_file_new -------------------------------------------- */

static void
test_state_file_new(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("test-client");
  assert_non_null(sf);
  assert_int_equal(sf->schema_version, SK_STATE_SCHEMA_VERSION);
  assert_string_equal(sf->client_id, "test-client");
  assert_non_null(sf->last_modified);
  assert_int_equal(sf->n_environments, 0);
  assert_null(sf->last_environment);

  sk_state_file_free(sf);
}

/* ---- Test: sk_state_file_new NULL client -------------------------------- */

static void
test_state_file_new_null_client(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new(NULL);
  assert_non_null(sf);
  assert_null(sf->client_id);
  sk_state_file_free(sf);
}

/* ---- Test: Struct allocation and free ----------------------------------- */

static void
test_tab_new_and_free(void **state)
{
  (void)state;

  SkTab *tab = sk_tab_new("uuid-1", "tmux-name", "My Tab", 0);
  assert_non_null(tab);
  assert_string_equal(tab->session_uuid, "uuid-1");
  assert_string_equal(tab->tmux_session_name, "tmux-name");
  assert_string_equal(tab->title, "My Tab");
  assert_int_equal(tab->position, 0);

  sk_tab_free(tab);
  sk_tab_free(NULL); /* Should not crash. */
}

static void
test_window_new_and_free(void **state)
{
  (void)state;

  SkWindow *win = sk_window_new("win-id", "Win Title");
  assert_non_null(win);
  assert_string_equal(win->id, "win-id");
  assert_string_equal(win->title, "Win Title");
  assert_true(win->visible);
  assert_int_equal(win->n_tabs, 0);
  assert_int_equal(win->active_tab, 0);
  assert_false(win->geometry.is_set);

  sk_window_free(win);
  sk_window_free(NULL);
}

static void
test_environment_new_and_free(void **state)
{
  (void)state;

  SkEnvironment *env = sk_environment_new("production");
  assert_non_null(env);
  assert_string_equal(env->name, "production");
  assert_int_equal(env->n_windows, 0);

  sk_environment_free(env);
  sk_environment_free(NULL);
}

/* ---- Test: JSON serialization round-trip -------------------------------- */

static void
test_state_json_roundtrip(void **state)
{
  (void)state;

  /* Build a state with one environment, one window, two tabs. */
  SkStateFile *sf = sk_state_file_new("my-client");

  SkEnvironment *env = sk_environment_new("dev");
  SkWindow *win = sk_window_new("win-1", "Main");
  win->geometry.is_set = true;
  win->geometry.x = 10;
  win->geometry.y = 20;
  win->geometry.width = 800;
  win->geometry.height = 600;

  SkTab *tab1 = sk_tab_new("uuid-aaa", "my-client--dev--session1", "Tab 1", 0);
  SkTab *tab2 = sk_tab_new("uuid-bbb", "my-client--dev--session2", "Tab 2", 1);

  win->n_tabs = 2;
  win->tabs = g_new0(SkTab *, 3);
  win->tabs[0] = tab1;
  win->tabs[1] = tab2;
  win->active_tab = 1;

  env->n_windows = 1;
  env->windows = g_new0(SkWindow *, 2);
  env->windows[0] = win;

  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;
  sf->last_environment = g_strdup("dev");

  /* Serialize to JSON. */
  char *json = sk_state_to_json(sf);
  assert_non_null(json);

  /* Deserialize back. */
  GError *error = NULL;
  SkStateFile *loaded = sk_state_from_json(json, &error);
  assert_non_null(loaded);
  assert_null(error);

  /* Verify all fields. */
  assert_int_equal(loaded->schema_version, SK_STATE_SCHEMA_VERSION);
  assert_string_equal(loaded->client_id, "my-client");
  assert_string_equal(loaded->last_environment, "dev");
  assert_int_equal(loaded->n_environments, 1);

  SkEnvironment *loaded_env = loaded->environments[0];
  assert_string_equal(loaded_env->name, "dev");
  assert_int_equal(loaded_env->n_windows, 1);

  SkWindow *loaded_win = loaded_env->windows[0];
  assert_string_equal(loaded_win->id, "win-1");
  assert_string_equal(loaded_win->title, "Main");
  assert_true(loaded_win->geometry.is_set);
  assert_int_equal(loaded_win->geometry.x, 10);
  assert_int_equal(loaded_win->geometry.y, 20);
  assert_int_equal(loaded_win->geometry.width, 800);
  assert_int_equal(loaded_win->geometry.height, 600);
  assert_int_equal(loaded_win->n_tabs, 2);
  assert_int_equal(loaded_win->active_tab, 1);

  assert_string_equal(loaded_win->tabs[0]->session_uuid, "uuid-aaa");
  assert_string_equal(loaded_win->tabs[0]->title, "Tab 1");
  assert_string_equal(loaded_win->tabs[1]->session_uuid, "uuid-bbb");
  assert_string_equal(loaded_win->tabs[1]->title, "Tab 2");

  g_free(json);
  sk_state_file_free(sf);
  sk_state_file_free(loaded);
}

/* ---- Test: Parse invalid JSON ------------------------------------------- */

static void
test_state_from_json_invalid(void **state)
{
  (void)state;

  GError *error = NULL;

  /* Totally broken JSON. */
  SkStateFile *sf = sk_state_from_json("{not valid", &error);
  assert_null(sf);
  assert_non_null(error);
  g_clear_error(&error);

  /* Valid JSON but not an object. */
  sf = sk_state_from_json("[]", &error);
  assert_null(sf);
  assert_non_null(error);
  g_clear_error(&error);

  /* Empty JSON object — should succeed with defaults. */
  sf = sk_state_from_json("{}", &error);
  assert_non_null(sf);
  assert_int_equal(sf->schema_version, 0);
  sk_state_file_free(sf);
}

/* ---- Test: Validation — valid state ------------------------------------- */

static void
test_state_validate_valid(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("my-client");

  SkEnvironment *env = sk_environment_new("dev");
  SkWindow *win = sk_window_new("win-1", "Main");

  SkTab *tab = sk_tab_new("uuid-111", "my-client--dev--session1", "Tab 1", 0);
  win->n_tabs = 1;
  win->tabs = g_new0(SkTab *, 2);
  win->tabs[0] = tab;

  env->n_windows = 1;
  env->windows = g_new0(SkWindow *, 2);
  env->windows[0] = win;

  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;
  sf->last_environment = g_strdup("dev");

  GError *error = NULL;
  assert_true(sk_state_validate(sf, &error));
  assert_null(error);

  sk_state_file_free(sf);
}

/* ---- Test: Validation — invalid schema_version -------------------------- */

static void
test_state_validate_bad_version(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("client");
  sf->schema_version = 0;

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  assert_int_equal(error->code, SK_STATE_ERROR_SCHEMA);
  g_clear_error(&error);

  sf->schema_version = -1;
  assert_false(sk_state_validate(sf, &error));
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: Validation — last_environment references missing env --------- */

static void
test_state_validate_bad_last_env(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("client");
  sf->last_environment = g_strdup("nonexistent");

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: Validation — duplicate UUIDs --------------------------------- */

static void
test_state_validate_duplicate_uuid(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("client");

  SkEnvironment *env = sk_environment_new("dev");
  SkWindow *win = sk_window_new("win-1", "Win");

  SkTab *tab1 = sk_tab_new("same-uuid", "tmux1", "Tab 1", 0);
  SkTab *tab2 = sk_tab_new("same-uuid", "tmux2", "Tab 2", 1);

  win->n_tabs = 2;
  win->tabs = g_new0(SkTab *, 3);
  win->tabs[0] = tab1;
  win->tabs[1] = tab2;

  env->n_windows = 1;
  env->windows = g_new0(SkWindow *, 2);
  env->windows[0] = win;

  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: Validation — active_tab out of range ------------------------- */

static void
test_state_validate_active_tab_range(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("client");

  SkEnvironment *env = sk_environment_new("dev");
  SkWindow *win = sk_window_new("win-1", "Win");

  SkTab *tab = sk_tab_new("uuid-ok", "tmux1", "Tab", 0);
  win->n_tabs = 1;
  win->tabs = g_new0(SkTab *, 2);
  win->tabs[0] = tab;
  win->active_tab = 5; /* Out of range. */

  env->n_windows = 1;
  env->windows = g_new0(SkWindow *, 2);
  env->windows[0] = win;

  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: Validation — invalid tmux session name ----------------------- */

static void
test_state_validate_bad_tmux_name(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("client");

  SkEnvironment *env = sk_environment_new("dev");
  SkWindow *win = sk_window_new("win-1", "Win");

  /* Name starts with a dot — invalid per tmux regex. */
  SkTab *tab = sk_tab_new("uuid-1", ".invalid-name", "Tab", 0);
  win->n_tabs = 1;
  win->tabs = g_new0(SkTab *, 2);
  win->tabs[0] = tab;

  env->n_windows = 1;
  env->windows = g_new0(SkWindow *, 2);
  env->windows[0] = win;

  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;

  GError *error = NULL;
  assert_false(sk_state_validate(sf, &error));
  assert_non_null(error);
  g_clear_error(&error);

  sk_state_file_free(sf);
}

/* ---- Test: Atomic save and load ----------------------------------------- */

static void
test_state_save_and_load(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "state.json", NULL);

  SkStateFile *sf = sk_state_file_new("test-save");
  SkEnvironment *env = sk_environment_new("dev");
  sf->n_environments = 1;
  sf->environments = g_new0(SkEnvironment *, 2);
  sf->environments[0] = env;
  sf->last_environment = g_strdup("dev");

  GError *error = NULL;
  assert_true(sk_state_save(sf, path, &error));
  assert_null(error);

  /* Verify file permissions. INV-SECURITY-3 */
  struct stat st;
  assert_int_equal(stat(path, &st), 0);
  assert_int_equal(st.st_mode & 0777, 0600);

  /* Load back. */
  SkStateFile *loaded = sk_state_load(path, &error);
  assert_non_null(loaded);
  assert_null(error);
  assert_string_equal(loaded->client_id, "test-save");
  assert_string_equal(loaded->last_environment, "dev");
  assert_int_equal(loaded->n_environments, 1);

  sk_state_file_free(sf);
  sk_state_file_free(loaded);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Load from nonexistent path ----------------------------------- */

static void
test_state_load_nonexistent(void **state)
{
  (void)state;

  GError *error = NULL;
  SkStateFile *sf = sk_state_load("/tmp/does_not_exist_sk_test.json", &error);
  assert_null(sf);
  assert_non_null(error);
  assert_int_equal(error->code, SK_STATE_ERROR_IO);
  g_clear_error(&error);
}

/* ---- Test: Load corrupt file triggers rename ---------------------------- */

static void
test_state_load_corrupt_renames(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = sk_test_write_file(tmpdir, "state.json", "{not valid json!!!");

  GError *error = NULL;
  SkStateFile *sf = sk_state_load(path, &error);
  assert_null(sf);
  assert_non_null(error);
  assert_int_equal(error->code, SK_STATE_ERROR_CORRUPT);
  g_clear_error(&error);

  /* Original file should have been renamed. */
  assert_false(g_file_test(path, G_FILE_TEST_EXISTS));

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: Load future version ------------------------------------------ */

static void
test_state_load_future_version(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  const char *json = "{"
                     "\"schema_version\": 999,"
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

/* ---- Test: Cleanup tmp files -------------------------------------------- */

static void
test_state_cleanup_tmp(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();

  /* Create some .tmp files. */
  char *tmp1 = sk_test_write_file(tmpdir, "state.json.tmp", "data");
  char *tmp2 = sk_test_write_file(tmpdir, "other.tmp", "data");
  char *keep = sk_test_write_file(tmpdir, "state.json", "{}");

  sk_state_cleanup_tmp_files(tmpdir);

  assert_false(g_file_test(tmp1, G_FILE_TEST_EXISTS));
  assert_false(g_file_test(tmp2, G_FILE_TEST_EXISTS));
  assert_true(g_file_test(keep, G_FILE_TEST_EXISTS));

  g_free(tmp1);
  g_free(tmp2);
  g_free(keep);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: to_json on empty state --------------------------------------- */

static void
test_state_to_json_empty(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("empty-client");
  char *json = sk_state_to_json(sf);
  assert_non_null(json);

  /* Should contain expected keys. */
  assert_non_null(strstr(json, "\"schema_version\""));
  assert_non_null(strstr(json, "\"client_id\""));
  assert_non_null(strstr(json, "\"environments\""));

  g_free(json);
  sk_state_file_free(sf);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_state_file_new),
    cmocka_unit_test(test_state_file_new_null_client),
    cmocka_unit_test(test_tab_new_and_free),
    cmocka_unit_test(test_window_new_and_free),
    cmocka_unit_test(test_environment_new_and_free),
    cmocka_unit_test(test_state_json_roundtrip),
    cmocka_unit_test(test_state_from_json_invalid),
    cmocka_unit_test(test_state_validate_valid),
    cmocka_unit_test(test_state_validate_bad_version),
    cmocka_unit_test(test_state_validate_bad_last_env),
    cmocka_unit_test(test_state_validate_duplicate_uuid),
    cmocka_unit_test(test_state_validate_active_tab_range),
    cmocka_unit_test(test_state_validate_bad_tmux_name),
    cmocka_unit_test(test_state_save_and_load),
    cmocka_unit_test(test_state_load_nonexistent),
    cmocka_unit_test(test_state_load_corrupt_renames),
    cmocka_unit_test(test_state_load_future_version),
    cmocka_unit_test(test_state_cleanup_tmp),
    cmocka_unit_test(test_state_to_json_empty),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
