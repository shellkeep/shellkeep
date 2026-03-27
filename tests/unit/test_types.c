// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_types.c
 * @brief Unit tests for common types, error quarks, and constants.
 *
 * Tests sk_error_quark, sk_state_error_quark, sk_connect_error_quark,
 * SkResult enum, SkConnectionState enum, version constants.
 */

#include "shellkeep/sk_state.h"
#include "shellkeep/sk_types.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: sk_error_quark ----------------------------------------------- */

static void
test_error_quark(void **state)
{
  (void)state;

  GQuark q = sk_error_quark();
  assert_true(q != 0);
  /* Stable across calls. */
  assert_int_equal(q, sk_error_quark());
}

/* ---- Test: sk_state_error_quark ----------------------------------------- */

static void
test_state_error_quark(void **state)
{
  (void)state;

  GQuark q = sk_state_error_quark();
  assert_true(q != 0);
  assert_int_equal(q, sk_state_error_quark());
  /* Different from SK_ERROR. */
  assert_int_not_equal(q, sk_error_quark());
}

/* ---- Test: SkResult enum values ----------------------------------------- */

static void
test_result_enum(void **state)
{
  (void)state;

  assert_int_equal(SK_OK, 0);
  assert_true(SK_ERROR_GENERIC < 0);
  assert_true(SK_ERROR_IO < 0);
  assert_true(SK_ERROR_TIMEOUT < 0);
  assert_true(SK_ERROR_AUTH < 0);
  assert_true(SK_ERROR_ALLOC < 0);

  /* All error values distinct. */
  assert_int_not_equal(SK_ERROR_GENERIC, SK_ERROR_IO);
  assert_int_not_equal(SK_ERROR_IO, SK_ERROR_TIMEOUT);
  assert_int_not_equal(SK_ERROR_TIMEOUT, SK_ERROR_AUTH);
  assert_int_not_equal(SK_ERROR_AUTH, SK_ERROR_ALLOC);
}

/* ---- Test: SkConnectionState enum values -------------------------------- */

static void
test_connection_state_enum(void **state)
{
  (void)state;

  assert_int_equal(SK_CONN_STATE_DISCONNECTED, 0);
  assert_int_not_equal(SK_CONN_STATE_DISCONNECTED, SK_CONN_STATE_CONNECTING);
  assert_int_not_equal(SK_CONN_STATE_CONNECTING, SK_CONN_STATE_AUTHENTICATING);
  assert_int_not_equal(SK_CONN_STATE_AUTHENTICATING, SK_CONN_STATE_CONNECTED);
  assert_int_not_equal(SK_CONN_STATE_CONNECTED, SK_CONN_STATE_RECONNECTING);
  assert_int_not_equal(SK_CONN_STATE_RECONNECTING, SK_CONN_STATE_ERROR);
}

/* ---- Test: Version constants -------------------------------------------- */

static void
test_version_constants(void **state)
{
  (void)state;

  assert_int_equal(SK_VERSION_MAJOR, 0);
  assert_int_equal(SK_VERSION_MINOR, 2);
  assert_int_equal(SK_VERSION_PATCH, 0);
  assert_string_equal(SK_VERSION_STRING, "0.2.0");
}

/* ---- Test: Application constants ---------------------------------------- */

static void
test_app_constants(void **state)
{
  (void)state;

  assert_string_equal(SK_APPLICATION_ID, "org.shellkeep.ShellKeep");
  assert_string_equal(SK_APPLICATION_NAME, "shellkeep");
}

/* ---- Test: State constants ---------------------------------------------- */

static void
test_state_constants(void **state)
{
  (void)state;

  assert_int_equal(SK_STATE_SCHEMA_VERSION, 1);
  assert_int_equal(SK_RECENT_SCHEMA_VERSION, 1);
  assert_int_equal(SK_RECENT_MAX_ENTRIES, 50);
  assert_int_equal(SK_HISTORY_MAX_FILE_SIZE_MB, 50);
  assert_int_equal(SK_HISTORY_MAX_TOTAL_SIZE_MB, 500);
  assert_int_equal(SK_HISTORY_DEFAULT_MAX_DAYS, 90);
  assert_int_equal(SK_STATE_DEBOUNCE_INTERVAL_MS, 2000);
  assert_int_equal(SK_DIR_PERMISSIONS, 0700);
  assert_int_equal(SK_FILE_PERMISSIONS, 0600);
}

/* ---- Test: SkStateError enum values -------------------------------------- */

static void
test_state_error_codes(void **state)
{
  (void)state;

  int codes[] = {
    SK_STATE_ERROR_PARSE,
    SK_STATE_ERROR_SCHEMA,
    SK_STATE_ERROR_VERSION_FUTURE,
    SK_STATE_ERROR_IO,
    SK_STATE_ERROR_CORRUPT,
    SK_STATE_ERROR_PERMISSION,
  };
  int n = sizeof(codes) / sizeof(codes[0]);

  for (int i = 0; i < n; i++)
  {
    for (int j = i + 1; j < n; j++)
    {
      assert_int_not_equal(codes[i], codes[j]);
    }
  }
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_error_quark),
    cmocka_unit_test(test_state_error_quark),
    cmocka_unit_test(test_result_enum),
    cmocka_unit_test(test_connection_state_enum),
    cmocka_unit_test(test_version_constants),
    cmocka_unit_test(test_app_constants),
    cmocka_unit_test(test_state_constants),
    cmocka_unit_test(test_state_error_codes),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
