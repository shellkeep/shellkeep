// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_ssh.c
 * @brief Unit tests for SSH helper functions.
 *
 * Tests client-id sanitization (FR-CONFIG-08), UUID validation,
 * and host key status enum coverage.
 * Actual SSH connections are tested in integration tests.
 *
 * NFR-BUILD-03..05
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_ssh.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: client-id sanitization via validate_client_id ---------------- */

static void
test_ssh_client_id_sanitization(void **state)
{
  (void)state;

  /* FR-CONFIG-08: client-id must be [a-zA-Z0-9_-], max 64 chars.
   * These are the same values SSH layer uses for session naming. */
  assert_true(sk_config_validate_client_id("desktop-home"));
  assert_true(sk_config_validate_client_id("laptop_work-01"));

  /* Reject path traversal attempts. */
  assert_false(sk_config_validate_client_id("../etc/passwd"));
  assert_false(sk_config_validate_client_id("foo/bar"));
  assert_false(sk_config_validate_client_id("foo;rm -rf"));
  assert_false(sk_config_validate_client_id("foo bar"));

  /* Reject empty. */
  assert_false(sk_config_validate_client_id(""));
  assert_false(sk_config_validate_client_id(NULL));
}

/* ---- Test: UUID v4 validation ------------------------------------------- */

/**
 * Validate a UUID v4 string format.
 * Expected format: xxxxxxxx-xxxx-4xxx-Nxxx-xxxxxxxxxxxx
 * where N is 8, 9, a, or b.
 */
static bool
validate_uuid_v4(const char *uuid)
{
  if (uuid == NULL)
    return false;

  size_t len = strlen(uuid);
  if (len != 36)
    return false;

  /* Check hyphen positions: 8, 13, 18, 23. */
  if (uuid[8] != '-' || uuid[13] != '-' || uuid[18] != '-' || uuid[23] != '-')
    return false;

  /* Check version nibble at position 14 == '4'. */
  if (uuid[14] != '4')
    return false;

  /* Check variant nibble at position 19 == 8, 9, a, b. */
  char v = uuid[19];
  if (v != '8' && v != '9' && v != 'a' && v != 'b')
    return false;

  /* All other chars must be hex. */
  for (size_t i = 0; i < len; i++)
  {
    if (i == 8 || i == 13 || i == 18 || i == 23)
      continue;
    char c = uuid[i];
    if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')))
      return false;
  }

  return true;
}

static void
test_ssh_uuid_v4_format(void **state)
{
  (void)state;

  /* Generate multiple UUIDs and verify format. */
  for (int i = 0; i < 10; i++)
  {
    char *uuid = g_uuid_string_random();
    assert_non_null(uuid);
    assert_true(validate_uuid_v4(uuid));
    g_free(uuid);
  }

  /* Invalid UUIDs. */
  assert_false(validate_uuid_v4(NULL));
  assert_false(validate_uuid_v4(""));
  assert_false(validate_uuid_v4("not-a-uuid"));
  assert_false(validate_uuid_v4("12345678-1234-1234-1234-123456789012"));
  /* Wrong version nibble (position 14 != '4'). */
  assert_false(validate_uuid_v4("12345678-1234-3234-8234-123456789abc"));
}

/* ---- Test: SkHostKeyStatus enum values ---------------------------------- */

static void
test_ssh_host_key_status_values(void **state)
{
  (void)state;

  /* Ensure enum values are distinct and well-defined. */
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_CHANGED);
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_UNKNOWN);
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_OTHER);
  assert_int_not_equal(SK_HOST_KEY_OK, SK_HOST_KEY_ERROR);

  assert_int_not_equal(SK_HOST_KEY_CHANGED, SK_HOST_KEY_UNKNOWN);
  assert_int_not_equal(SK_HOST_KEY_CHANGED, SK_HOST_KEY_OTHER);

  /* Verify expected values for switch coverage. */
  assert_int_equal(SK_HOST_KEY_OK, 0);
}

/* ---- Test: SkSshErrorCode enum values ----------------------------------- */

static void
test_ssh_error_codes(void **state)
{
  (void)state;

  /* Ensure all error codes are distinct. */
  int codes[] = {
    SK_SSH_ERROR_CONNECT,  SK_SSH_ERROR_HOST_KEY,     SK_SSH_ERROR_AUTH,
    SK_SSH_ERROR_CHANNEL,  SK_SSH_ERROR_SFTP,         SK_SSH_ERROR_TIMEOUT,
    SK_SSH_ERROR_PROTOCOL, SK_SSH_ERROR_DISCONNECTED, SK_SSH_ERROR_CRYPTO,
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

/* ---- Test: SkAuthResult enum values ------------------------------------- */

static void
test_ssh_auth_result_values(void **state)
{
  (void)state;

  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_DENIED);
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_PARTIAL);
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_ERROR);
  assert_int_not_equal(SK_AUTH_SUCCESS, SK_AUTH_CANCELLED);
  assert_int_equal(SK_AUTH_SUCCESS, 0);
}

/* ---- Test: SkAuthMethod bitmask ----------------------------------------- */

static void
test_ssh_auth_method_bitmask(void **state)
{
  (void)state;

  /* Each method should be a distinct bit. */
  assert_int_not_equal(SK_AUTH_METHOD_AGENT, SK_AUTH_METHOD_PUBKEY);
  assert_int_not_equal(SK_AUTH_METHOD_PUBKEY, SK_AUTH_METHOD_PASSWORD);
  assert_int_not_equal(SK_AUTH_METHOD_PASSWORD, SK_AUTH_METHOD_KEYBOARD_INTERACTIVE);

  /* ALL should be the OR of all methods. */
  unsigned int all = SK_AUTH_METHOD_AGENT | SK_AUTH_METHOD_PUBKEY | SK_AUTH_METHOD_PASSWORD |
                     SK_AUTH_METHOD_KEYBOARD_INTERACTIVE;
  assert_int_equal(SK_AUTH_METHOD_ALL, all);

  /* Individual bits are powers of 2. */
  assert_int_equal(SK_AUTH_METHOD_AGENT, 1);
  assert_int_equal(SK_AUTH_METHOD_PUBKEY, 2);
  assert_int_equal(SK_AUTH_METHOD_PASSWORD, 4);
  assert_int_equal(SK_AUTH_METHOD_KEYBOARD_INTERACTIVE, 8);
}

/* ---- Test: error quark is registered ------------------------------------ */

static void
test_ssh_error_quark(void **state)
{
  (void)state;

  GQuark q = sk_ssh_error_quark();
  assert_true(q != 0);

  /* Quark should be stable. */
  assert_int_equal(q, sk_ssh_error_quark());
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_ssh_client_id_sanitization),
    cmocka_unit_test(test_ssh_uuid_v4_format),
    cmocka_unit_test(test_ssh_host_key_status_values),
    cmocka_unit_test(test_ssh_error_codes),
    cmocka_unit_test(test_ssh_auth_result_values),
    cmocka_unit_test(test_ssh_auth_method_bitmask),
    cmocka_unit_test(test_ssh_error_quark),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
