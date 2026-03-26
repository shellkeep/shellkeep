// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_control_parse.c
 * @brief Unit tests for tmux control mode notification parsing.
 *
 * Tests sk_ctrl_parse_notification for all notification types:
 * %begin, %end, %error, %output, %session-changed, %exit, unknown.
 * Also tests sk_ctrl_notification_free NULL safety.
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

/* ---- Test: parse NULL returns NULL -------------------------------------- */

static void
test_ctrl_parse_null(void **state)
{
  (void)state;
  assert_null(sk_ctrl_parse_notification(NULL));
}

/* ---- Test: parse non-% line returns NULL -------------------------------- */

static void
test_ctrl_parse_non_percent(void **state)
{
  (void)state;
  assert_null(sk_ctrl_parse_notification("regular output line"));
  assert_null(sk_ctrl_parse_notification(""));
}

/* ---- Test: parse %begin ------------------------------------------------- */

static void
test_ctrl_parse_begin(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%begin 1711234567 42 0");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_BEGIN);
  assert_int_equal(n->cmd_number, 42);

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse %end --------------------------------------------------- */

static void
test_ctrl_parse_end(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%end 1711234567 42 0");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_END);
  assert_int_equal(n->cmd_number, 42);

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse %error ------------------------------------------------- */

static void
test_ctrl_parse_error(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%error something went wrong");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_ERROR);
  assert_non_null(n->data);
  assert_string_equal(n->data, "something went wrong");

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse %output ------------------------------------------------ */

static void
test_ctrl_parse_output(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%output %0 hello world");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_OUTPUT);
  assert_non_null(n->data);
  assert_string_equal(n->data, "%0 hello world");

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse %session-changed --------------------------------------- */

static void
test_ctrl_parse_session_changed(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%session-changed $1 my-session");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_SESSION_CHANGED);
  assert_non_null(n->data);
  assert_string_equal(n->data, "$1 my-session");

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse %exit -------------------------------------------------- */

static void
test_ctrl_parse_exit(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%exit server exited");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_EXIT);
  assert_non_null(n->data);
  assert_string_equal(n->data, "server exited");

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse %exit without reason ----------------------------------- */

static void
test_ctrl_parse_exit_no_reason(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%exit");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_EXIT);
  assert_null(n->data);

  sk_ctrl_notification_free(n);
}

/* ---- Test: parse unknown %notification ---------------------------------- */

static void
test_ctrl_parse_unknown(void **state)
{
  (void)state;

  SkCtrlNotification *n = sk_ctrl_parse_notification("%something-new data");
  assert_non_null(n);
  assert_int_equal(n->type, SK_CTRL_NOTIFICATION_UNKNOWN);
  assert_non_null(n->data);

  sk_ctrl_notification_free(n);
}

/* ---- Test: sk_ctrl_notification_free NULL safety ------------------------ */

static void
test_ctrl_notification_free_null(void **state)
{
  (void)state;
  sk_ctrl_notification_free(NULL); /* Should not crash. */
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_ctrl_parse_null),
    cmocka_unit_test(test_ctrl_parse_non_percent),
    cmocka_unit_test(test_ctrl_parse_begin),
    cmocka_unit_test(test_ctrl_parse_end),
    cmocka_unit_test(test_ctrl_parse_error),
    cmocka_unit_test(test_ctrl_parse_output),
    cmocka_unit_test(test_ctrl_parse_session_changed),
    cmocka_unit_test(test_ctrl_parse_exit),
    cmocka_unit_test(test_ctrl_parse_exit_no_reason),
    cmocka_unit_test(test_ctrl_parse_unknown),
    cmocka_unit_test(test_ctrl_notification_free_null),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
