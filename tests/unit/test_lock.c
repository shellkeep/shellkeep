// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_lock.c
 * @brief Unit tests for lock utility functions.
 *
 * Tests sk_lock_is_orphaned (FR-LOCK-07), sk_lock_is_own (FR-LOCK-06),
 * sk_lock_info_free, and SkLockInfo field handling.
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
#include <time.h>

/* ---- Test: sk_lock_is_orphaned — recent lock is not orphaned ------------- */

static void
test_lock_is_orphaned_recent(void **state)
{
  (void)state;

  /* Create a lock with a recent timestamp. */
  GDateTime *now = g_date_time_new_now_utc();
  char *ts = g_date_time_format_iso8601(now);
  g_date_time_unref(now);

  SkLockInfo info = {
    .client_id = g_strdup("test"),
    .hostname = g_strdup("host"),
    .connected_at = ts,
    .pid = g_strdup("1234"),
    .valid = true,
    .orphaned = false,
  };

  /* With a 45-second timeout, a fresh lock should not be orphaned. */
  assert_false(sk_lock_is_orphaned(&info, 45));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.connected_at);
  g_free(info.pid);
}

/* ---- Test: sk_lock_is_orphaned — old lock is orphaned ------------------- */

static void
test_lock_is_orphaned_old(void **state)
{
  (void)state;

  /* Use a timestamp from the distant past. */
  SkLockInfo info = {
    .client_id = g_strdup("test"),
    .hostname = g_strdup("host"),
    .connected_at = g_strdup("2020-01-01T00:00:00Z"),
    .pid = g_strdup("1234"),
    .valid = true,
    .orphaned = false,
  };

  /* With any reasonable timeout, a 2020 lock is orphaned. */
  assert_true(sk_lock_is_orphaned(&info, 45));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.connected_at);
  g_free(info.pid);
}

/* ---- Test: sk_lock_is_orphaned — NULL connected_at ---------------------- */

static void
test_lock_is_orphaned_null_timestamp(void **state)
{
  (void)state;

  SkLockInfo info = {
    .client_id = g_strdup("test"),
    .hostname = g_strdup("host"),
    .connected_at = NULL,
    .pid = g_strdup("1234"),
    .valid = true,
    .orphaned = false,
  };

  /* NULL timestamp means we can't determine — should treat as orphaned. */
  assert_true(sk_lock_is_orphaned(&info, 45));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.pid);
}

/* ---- Test: sk_lock_is_own — matching hostname and PID ------------------- */

static void
test_lock_is_own_match(void **state)
{
  (void)state;

  SkLockInfo info = {
    .client_id = g_strdup("my-client"),
    .hostname = g_strdup("myhost"),
    .pid = g_strdup("12345"),
    .valid = true,
  };

  assert_true(sk_lock_is_own(&info, "myhost", "12345"));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.pid);
}

/* ---- Test: sk_lock_is_own — different hostname -------------------------- */

static void
test_lock_is_own_different_host(void **state)
{
  (void)state;

  SkLockInfo info = {
    .client_id = g_strdup("my-client"),
    .hostname = g_strdup("myhost"),
    .pid = g_strdup("12345"),
    .valid = true,
  };

  assert_false(sk_lock_is_own(&info, "otherhost", "12345"));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.pid);
}

/* ---- Test: sk_lock_is_own — different PID ------------------------------- */

static void
test_lock_is_own_different_pid(void **state)
{
  (void)state;

  SkLockInfo info = {
    .client_id = g_strdup("my-client"),
    .hostname = g_strdup("myhost"),
    .pid = g_strdup("12345"),
    .valid = true,
  };

  assert_false(sk_lock_is_own(&info, "myhost", "99999"));

  g_free(info.client_id);
  g_free(info.hostname);
  g_free(info.pid);
}

/* ---- Test: session constants -------------------------------------------- */

static void
test_session_constants(void **state)
{
  (void)state;

  assert_int_equal(SK_TMUX_MIN_VERSION_MAJOR, 3);
  assert_int_equal(SK_TMUX_MIN_VERSION_MINOR, 0);
  assert_string_equal(SK_SESSION_NAME_DELIM, "--");
  assert_string_equal(SK_LOCK_SESSION_PREFIX, "shellkeep-lock-");
  assert_int_equal(SK_LOCK_HEARTBEAT_INTERVAL, 30);
  assert_int_equal(SK_LOCK_ORPHAN_MULTIPLIER, 2);
  assert_int_equal(SK_LOCK_DEFAULT_KEEPALIVE_TIMEOUT, 45);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_lock_is_orphaned_recent),
    cmocka_unit_test(test_lock_is_orphaned_old),
    cmocka_unit_test(test_lock_is_orphaned_null_timestamp),
    cmocka_unit_test(test_lock_is_own_match),
    cmocka_unit_test(test_lock_is_own_different_host),
    cmocka_unit_test(test_lock_is_own_different_pid),
    cmocka_unit_test(test_session_constants),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
