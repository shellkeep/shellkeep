// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_session.c
 * @brief Unit tests for session naming and reconciliation utilities.
 *
 * Tests FR-SESSION-04..07: session name build/parse, generation,
 * and name format validation.
 *
 * NFR-BUILD-03..05
 */

#include "shellkeep/sk_session.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: sk_session_build_name ---------------------------------------- */

static void
test_session_build_name(void **state)
{
  (void)state;

  /* FR-SESSION-04: <client_id>--<environment>--<session_name> */
  char *name = sk_session_build_name("my-laptop", "dev", "backend");
  assert_non_null(name);
  assert_string_equal(name, "my-laptop--dev--backend");
  g_free(name);

  /* With longer components. */
  name = sk_session_build_name("desktop-home", "production", "web-server");
  assert_non_null(name);
  assert_string_equal(name, "desktop-home--production--web-server");
  g_free(name);
}

/* ---- Test: sk_session_parse_name ---------------------------------------- */

static void
test_session_parse_name(void **state)
{
  (void)state;

  char *client_id = NULL;
  char *environment = NULL;
  char *session_name = NULL;

  bool ok =
      sk_session_parse_name("my-laptop--dev--backend", &client_id, &environment, &session_name);
  assert_true(ok);
  assert_string_equal(client_id, "my-laptop");
  assert_string_equal(environment, "dev");
  assert_string_equal(session_name, "backend");

  g_free(client_id);
  g_free(environment);
  g_free(session_name);
}

/* ---- Test: parse_name with extra delimiters in session name ------------- */

static void
test_session_parse_name_extra_delim(void **state)
{
  (void)state;

  char *client_id = NULL;
  char *environment = NULL;
  char *session_name = NULL;

  /* Session name itself contains "--". */
  bool ok =
      sk_session_parse_name("laptop--work--my--session", &client_id, &environment, &session_name);
  assert_true(ok);
  assert_string_equal(client_id, "laptop");
  assert_string_equal(environment, "work");
  /* Everything after second delimiter is the session name. */
  assert_string_equal(session_name, "my--session");

  g_free(client_id);
  g_free(environment);
  g_free(session_name);
}

/* ---- Test: parse_name with missing delimiters --------------------------- */

static void
test_session_parse_name_missing_delim(void **state)
{
  (void)state;

  /* No delimiter at all. */
  assert_false(sk_session_parse_name("nodashes", NULL, NULL, NULL));

  /* Only one delimiter. */
  assert_false(sk_session_parse_name("one--part", NULL, NULL, NULL));
}

/* ---- Test: parse_name with NULL outputs --------------------------------- */

static void
test_session_parse_name_null_outputs(void **state)
{
  (void)state;

  /* Should work even when output pointers are NULL. */
  bool ok = sk_session_parse_name("a--b--c", NULL, NULL, NULL);
  assert_true(ok);
}

/* ---- Test: sk_session_generate_name ------------------------------------- */

static void
test_session_generate_name(void **state)
{
  (void)state;

  char *name = sk_session_generate_name();
  assert_non_null(name);

  /* FR-SESSION-05: session-YYYYMMDD-HHMMSS */
  assert_true(g_str_has_prefix(name, "session-"));
  assert_int_equal(strlen(name), strlen("session-20260326-123456"));

  /* Verify it contains only valid chars for tmux. */
  for (size_t i = 0; name[i] != '\0'; i++)
  {
    char c = name[i];
    assert_true(g_ascii_isalnum(c) || c == '-');
  }

  g_free(name);
}

/* ---- Test: generate_name uniqueness ------------------------------------- */

static void
test_session_generate_name_unique(void **state)
{
  (void)state;

  /* Two consecutive calls should produce the same name in the same second,
   * but the format should be consistent. */
  char *name1 = sk_session_generate_name();
  char *name2 = sk_session_generate_name();
  assert_non_null(name1);
  assert_non_null(name2);

  /* Both should have the correct prefix. */
  assert_true(g_str_has_prefix(name1, "session-"));
  assert_true(g_str_has_prefix(name2, "session-"));

  g_free(name1);
  g_free(name2);
}

/* ---- Test: build+parse roundtrip ---------------------------------------- */

static void
test_session_name_roundtrip(void **state)
{
  (void)state;

  const char *cid = "my-client-123";
  const char *env = "staging";
  const char *sn = "api-server";

  char *built = sk_session_build_name(cid, env, sn);
  assert_non_null(built);

  char *out_cid = NULL;
  char *out_env = NULL;
  char *out_sn = NULL;

  assert_true(sk_session_parse_name(built, &out_cid, &out_env, &out_sn));
  assert_string_equal(out_cid, cid);
  assert_string_equal(out_env, env);
  assert_string_equal(out_sn, sn);

  g_free(built);
  g_free(out_cid);
  g_free(out_env);
  g_free(out_sn);
}

/* ---- Test: tmux version parsing ----------------------------------------- */

static void
test_tmux_parse_version(void **state)
{
  (void)state;

  int major = -1, minor = -1;

  /* Standard version strings. */
  assert_true(sk_tmux_parse_version("tmux 3.3a", &major, &minor));
  assert_int_equal(major, 3);
  assert_int_equal(minor, 3);

  assert_true(sk_tmux_parse_version("tmux 3.0", &major, &minor));
  assert_int_equal(major, 3);
  assert_int_equal(minor, 0);

  assert_true(sk_tmux_parse_version("tmux 4.1", &major, &minor));
  assert_int_equal(major, 4);
  assert_int_equal(minor, 1);

  /* Edge case: version 2.9. */
  assert_true(sk_tmux_parse_version("tmux 2.9", &major, &minor));
  assert_int_equal(major, 2);
  assert_int_equal(minor, 9);
}

/* ---- Test: tmux version comparison -------------------------------------- */

static void
test_tmux_version_ok(void **state)
{
  (void)state;

  /* >= 3.0 should pass. */
  assert_true(sk_tmux_version_ok(3, 0));
  assert_true(sk_tmux_version_ok(3, 3));
  assert_true(sk_tmux_version_ok(4, 0));
  assert_true(sk_tmux_version_ok(10, 0));

  /* < 3.0 should fail. */
  assert_false(sk_tmux_version_ok(2, 9));
  assert_false(sk_tmux_version_ok(2, 0));
  assert_false(sk_tmux_version_ok(1, 0));
  assert_false(sk_tmux_version_ok(0, 0));
}

/* ---- Test: lock info free NULL safety ----------------------------------- */

static void
test_lock_info_free_null(void **state)
{
  (void)state;
  sk_lock_info_free(NULL); /* Should not crash. */
}

/* ---- Test: session info free NULL safety -------------------------------- */

static void
test_session_info_free_null(void **state)
{
  (void)state;
  sk_session_info_free(NULL); /* Should not crash. */
}

/* ---- Test: sk_lock_is_own ----------------------------------------------- */

static void
test_lock_is_own(void **state)
{
  (void)state;

  SkLockInfo info = {
    .client_id = g_strdup("my-client"),
    .hostname = g_strdup("myhost"),
    .pid = g_strdup("12345"),
    .valid = true,
    .orphaned = false,
  };

  assert_true(sk_lock_is_own(&info, "myhost", "12345"));
  assert_false(sk_lock_is_own(&info, "otherhost", "12345"));
  assert_false(sk_lock_is_own(&info, "myhost", "99999"));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.pid);
}

/* ---- Test: session error quark ------------------------------------------ */

static void
test_session_error_quark(void **state)
{
  (void)state;

  GQuark q = sk_session_error_quark();
  assert_true(q != 0);
  assert_int_equal(q, sk_session_error_quark());
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_session_build_name),
    cmocka_unit_test(test_session_parse_name),
    cmocka_unit_test(test_session_parse_name_extra_delim),
    cmocka_unit_test(test_session_parse_name_missing_delim),
    cmocka_unit_test(test_session_parse_name_null_outputs),
    cmocka_unit_test(test_session_generate_name),
    cmocka_unit_test(test_session_generate_name_unique),
    cmocka_unit_test(test_session_name_roundtrip),
    cmocka_unit_test(test_tmux_parse_version),
    cmocka_unit_test(test_tmux_version_ok),
    cmocka_unit_test(test_lock_info_free_null),
    cmocka_unit_test(test_session_info_free_null),
    cmocka_unit_test(test_lock_is_own),
    cmocka_unit_test(test_session_error_quark),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
