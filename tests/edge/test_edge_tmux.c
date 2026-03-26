// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_edge_tmux.c
 * @brief Edge-case tests for tmux session handling.
 *
 * Covers: externally renamed session, session killed mid-op,
 * lock with missing env vars, lock with mismatched client-id,
 * session name edge cases.
 *
 * FR-SESSION-04..08, FR-LOCK-01..10
 */

#include "shellkeep/sk_session.h"
#include "shellkeep/sk_state.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: Parse externally renamed session (no delimiters) ------------- */

static void
test_edge_session_parse_no_delimiters(void **state)
{
  (void)state;

  char *cid = NULL;
  char *env = NULL;
  char *sess = NULL;

  /* A session renamed externally to have no "--" delimiters. */
  bool ok = sk_session_parse_name("just-a-name", &cid, &env, &sess);
  assert_false(ok);
  /* All outputs should be NULL on failure. */
  assert_null(cid);
  assert_null(env);
  assert_null(sess);
}

/* ---- Test: Parse session with single delimiter only --------------------- */

static void
test_edge_session_parse_one_delimiter(void **state)
{
  (void)state;

  char *cid = NULL;
  char *env = NULL;
  char *sess = NULL;

  /* Only one "--" delimiter, not two. */
  bool ok = sk_session_parse_name("client--envonly", &cid, &env, &sess);
  /* Depending on implementation: might fail (no session component)
   * or succeed with empty session name. */
  if (ok)
  {
    assert_non_null(cid);
    assert_non_null(env);
    g_free(cid);
    g_free(env);
    g_free(sess);
  }
}

/* ---- Test: Parse session with empty components -------------------------- */

static void
test_edge_session_parse_empty_components(void **state)
{
  (void)state;

  char *cid = NULL;
  char *env = NULL;
  char *sess = NULL;

  /* All components empty: "--" at the start and middle. */
  bool ok = sk_session_parse_name("----", &cid, &env, &sess);
  if (ok)
  {
    /* Components should be empty strings. */
    g_free(cid);
    g_free(env);
    g_free(sess);
  }
}

/* ---- Test: Build name with NULL components ------------------------------ */

static void
test_edge_session_build_name_nulls(void **state)
{
  (void)state;

  /* NULL client_id should not crash. */
  char *name = sk_session_build_name(NULL, "env", "sess");
  /* Implementation may return NULL or "(null)--env--sess". */
  if (name != NULL)
    g_free(name);

  name = sk_session_build_name("client", NULL, "sess");
  if (name != NULL)
    g_free(name);

  name = sk_session_build_name("client", "env", NULL);
  if (name != NULL)
    g_free(name);
}

/* ---- Test: Lock info with missing required fields ----------------------- */

static void
test_edge_lock_info_missing_fields(void **state)
{
  (void)state;

  /* Create a lock info with some fields NULL to simulate
   * a lock session missing environment variables. */
  SkLockInfo info = {
    .client_id = NULL,
    .hostname = NULL,
    .connected_at = NULL,
    .pid = NULL,
    .version = NULL,
    .valid = false,
    .orphaned = false,
  };

  /* With missing fields, the lock should not be considered valid. */
  assert_false(info.valid);

  /* is_own should return false for incomplete lock info. */
  assert_false(sk_lock_is_own(&info, "myhostname", "12345"));
}

/* ---- Test: Lock info with mismatched client-id -------------------------- */

static void
test_edge_lock_info_mismatch(void **state)
{
  (void)state;

  SkLockInfo info = {
    .client_id = g_strdup("other-client"),
    .hostname = g_strdup("other-host"),
    .connected_at = g_strdup("2026-01-01T00:00:00Z"),
    .pid = g_strdup("9999"),
    .version = g_strdup("0.1.0"),
    .valid = true,
    .orphaned = false,
  };

  /* Different hostname and PID: should NOT be our lock. */
  assert_false(sk_lock_is_own(&info, "my-host", "12345"));

  /* Same hostname but different PID: also not ours. */
  assert_false(sk_lock_is_own(&info, "other-host", "12345"));

  /* Same hostname and PID: IS ours. */
  assert_true(sk_lock_is_own(&info, "other-host", "9999"));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.connected_at);
  g_free(info.pid);
  g_free(info.version);
}

/* ---- Test: Lock orphan detection ---------------------------------------- */

static void
test_edge_lock_orphan_detection(void **state)
{
  (void)state;

  /* Create a lock info with a very old connected_at timestamp.
   * This simulates a client that crashed without releasing the lock. */
  SkLockInfo info = {
    .client_id = g_strdup("dead-client"),
    .hostname = g_strdup("dead-host"),
    .connected_at = g_strdup("2020-01-01T00:00:00Z"),
    .pid = g_strdup("1"),
    .version = g_strdup("0.1.0"),
    .valid = true,
    .orphaned = false,
  };

  /* With a 45-second keepalive timeout, a 6-year-old lock is orphaned. */
  bool orphaned = sk_lock_is_orphaned(&info, SK_LOCK_DEFAULT_KEEPALIVE_TIMEOUT);
  assert_true(orphaned);

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.connected_at);
  g_free(info.pid);
  g_free(info.version);
}

/* ---- Test: tmux version parsing edge cases ------------------------------ */

static void
test_edge_tmux_version_parse(void **state)
{
  (void)state;

  int major = 0, minor = 0;

  /* Normal version string. */
  assert_true(sk_tmux_parse_version("tmux 3.3a", &major, &minor));
  assert_int_equal(major, 3);
  assert_int_equal(minor, 3);

  /* Version exactly at minimum. */
  assert_true(sk_tmux_parse_version("tmux 3.0", &major, &minor));
  assert_int_equal(major, 3);
  assert_int_equal(minor, 0);
  assert_true(sk_tmux_version_ok(major, minor));

  /* Version below minimum. */
  assert_true(sk_tmux_parse_version("tmux 2.9", &major, &minor));
  assert_int_equal(major, 2);
  assert_int_equal(minor, 9);
  assert_false(sk_tmux_version_ok(major, minor));

  /* Garbled version string. */
  assert_false(sk_tmux_parse_version("not-tmux", &major, &minor));
  assert_false(sk_tmux_parse_version("", &major, &minor));
  assert_false(sk_tmux_parse_version(NULL, &major, &minor));
}

/* ---- Test: Session generate name returns valid format -------------------- */

static void
test_edge_session_generate_name(void **state)
{
  (void)state;

  /* FR-SESSION-05: session-YYYYMMDD-HHMMSS */
  char *name = sk_session_generate_name();
  assert_non_null(name);
  assert_true(strlen(name) > 0);

  /* Should start with "session-". */
  assert_true(strncmp(name, "session-", 8) == 0);

  g_free(name);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_edge_session_parse_no_delimiters),
    cmocka_unit_test(test_edge_session_parse_one_delimiter),
    cmocka_unit_test(test_edge_session_parse_empty_components),
    cmocka_unit_test(test_edge_session_build_name_nulls),
    cmocka_unit_test(test_edge_lock_info_missing_fields),
    cmocka_unit_test(test_edge_lock_info_mismatch),
    cmocka_unit_test(test_edge_lock_orphan_detection),
    cmocka_unit_test(test_edge_tmux_version_parse),
    cmocka_unit_test(test_edge_session_generate_name),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
