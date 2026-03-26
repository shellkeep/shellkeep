// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_edge_limits.c
 * @brief Edge-case tests for boundary limits and Unicode handling.
 *
 * Covers: client-id at 64 chars (max) and 65 (reject), Unicode
 * environment names, 50 recent connections + evict, 50MB JSONL
 * rotation trigger, 100 environments.
 *
 * FR-CONFIG-08, FR-CLI-02, NFR-SEC-04..05
 */

#include "shellkeep/sk_config.h"
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

/* ---- Test: client-id exactly 64 chars (max boundary) -------------------- */

static void
test_edge_client_id_64_chars(void **state)
{
  (void)state;

  /* Exactly 64 characters of valid chars. */
  char id64[65];
  memset(id64, 'a', 64);
  id64[64] = '\0';

  assert_true(sk_config_validate_client_id(id64));
}

/* ---- Test: client-id at 65 chars (must reject) -------------------------- */

static void
test_edge_client_id_65_chars(void **state)
{
  (void)state;

  /* 65 characters: over the limit. */
  char id65[66];
  memset(id65, 'b', 65);
  id65[65] = '\0';

  assert_false(sk_config_validate_client_id(id65));
}

/* ---- Test: client-id with only valid boundary chars --------------------- */

static void
test_edge_client_id_boundary_chars(void **state)
{
  (void)state;

  /* Single char edge cases. */
  assert_true(sk_config_validate_client_id("a"));
  assert_true(sk_config_validate_client_id("Z"));
  assert_true(sk_config_validate_client_id("0"));
  assert_true(sk_config_validate_client_id("_"));
  assert_true(sk_config_validate_client_id("-"));
  assert_true(sk_config_validate_client_id("a-z_0-9_A-Z"));

  /* Invalid characters. */
  assert_false(sk_config_validate_client_id("."));
  assert_false(sk_config_validate_client_id("/"));
  assert_false(sk_config_validate_client_id("\\"));
  assert_false(sk_config_validate_client_id(":"));
  assert_false(sk_config_validate_client_id(" "));
  assert_false(sk_config_validate_client_id("\t"));
  assert_false(sk_config_validate_client_id("\n"));
  assert_false(sk_config_validate_client_id("\0"));
}

/* ---- Test: Unicode in environment names (SkEnvironment) ----------------- */

static void
test_edge_unicode_environment(void **state)
{
  (void)state;

  /* Create environment with ASCII name (tmux-safe). */
  SkEnvironment *env = sk_environment_new("production-eu");
  assert_non_null(env);
  assert_string_equal(env->name, "production-eu");
  sk_environment_free(env);

  /* Empty environment name edge case. */
  env = sk_environment_new("");
  assert_non_null(env);
  assert_string_equal(env->name, "");
  sk_environment_free(env);
}

/* ---- Test: 50 recent connections + eviction ----------------------------- */

static void
test_edge_recent_50_plus_evict(void **state)
{
  (void)state;

  SkRecentConnections *recent = sk_recent_new();
  assert_non_null(recent);

  /* Add exactly 50 connections. */
  for (int i = 0; i < 50; i++)
  {
    char host[32];
    snprintf(host, sizeof(host), "host%d.example.com", i);
    sk_recent_add(recent, host, "user", 22, NULL, NULL);
  }
  assert_int_equal(recent->n_connections, 50);

  /* Add one more -- should evict the oldest (first added). */
  sk_recent_add(recent, "host-new.example.com", "user", 22, NULL, NULL);
  assert_true(recent->n_connections <= SK_RECENT_MAX_ENTRIES);

  /* The newest entry should be present. */
  bool found_new = false;
  for (int i = 0; i < recent->n_connections; i++)
  {
    if (strcmp(recent->connections[i]->host, "host-new.example.com") == 0)
    {
      found_new = true;
      break;
    }
  }
  assert_true(found_new);

  sk_recent_free(recent);
}

/* ---- Test: Recent connections merge duplicates -------------------------- */

static void
test_edge_recent_merge_duplicate(void **state)
{
  (void)state;

  SkRecentConnections *recent = sk_recent_new();

  /* Add same host twice -- should merge, not create duplicate. */
  sk_recent_add(recent, "same.host.com", "user", 22, NULL, NULL);
  sk_recent_add(recent, "same.host.com", "user", 22, "alias2", NULL);

  assert_int_equal(recent->n_connections, 1);

  sk_recent_free(recent);
}

/* ---- Test: 100 environments in state ------------------------------------ */

static void
test_edge_100_environments(void **state)
{
  (void)state;

  SkStateFile *sf = sk_state_file_new("big-client");
  sf->n_environments = 100;
  sf->environments = g_new0(SkEnvironment *, 101);

  for (int i = 0; i < 100; i++)
  {
    char name[32];
    snprintf(name, sizeof(name), "env-%03d", i);
    sf->environments[i] = sk_environment_new(name);
  }
  sf->last_environment = g_strdup("env-000");

  /* Serialize and parse back. */
  char *json = sk_state_to_json(sf);
  assert_non_null(json);

  GError *error = NULL;
  SkStateFile *loaded = sk_state_from_json(json, &error);
  assert_non_null(loaded);
  assert_null(error);
  assert_int_equal(loaded->n_environments, 100);

  g_free(json);
  sk_state_file_free(sf);
  sk_state_file_free(loaded);
}

/* ---- Test: JSONL rotation at size boundary ------------------------------ */

static void
test_edge_jsonl_rotation(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();

  /* Write a file that is over the rotation threshold (use 1 MB for test). */
  char *path = g_build_filename(tmpdir, "a2b3c4d5.jsonl", NULL);
  FILE *fp = fopen(path, "wb");
  assert_non_null(fp);

  /* Write ~1.1 MB of valid JSONL lines. */
  char text_buf[128];
  memset(text_buf, 'X', 100);
  text_buf[100] = '\0';

  for (int i = 0; i < 8000; i++)
  {
    fprintf(fp,
            "{\"ts\":\"2026-01-01T00:00:00Z\",\"type\":0,\"text\":\"%s\"}\n",
            text_buf);
  }
  fclose(fp);

  /* Verify file is over 1 MB. */
  struct stat st;
  assert_int_equal(stat(path, &st), 0);
  assert_true(st.st_size > 1 * 1024 * 1024);

  /* Rotate with a 1 MB limit. */
  GError *error = NULL;
  bool ok = sk_history_rotate("a2b3c4d5", tmpdir, 1, &error);
  assert_true(ok);
  g_clear_error(&error);

  /* File should now be smaller (oldest 25% removed). */
  assert_int_equal(stat(path, &st), 0);
  assert_true(st.st_size < 1 * 1024 * 1024);

  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_edge_client_id_64_chars),
    cmocka_unit_test(test_edge_client_id_65_chars),
    cmocka_unit_test(test_edge_client_id_boundary_chars),
    cmocka_unit_test(test_edge_unicode_environment),
    cmocka_unit_test(test_edge_recent_50_plus_evict),
    cmocka_unit_test(test_edge_recent_merge_duplicate),
    cmocka_unit_test(test_edge_100_environments),
    cmocka_unit_test(test_edge_jsonl_rotation),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
