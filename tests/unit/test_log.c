// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_log.c
 * @brief Unit tests for the logging subsystem.
 *
 * Tests log format, level filtering, level/component string conversion,
 * and level get/set. Uses the stub implementation for unit testing
 * since the full async logger requires thread infrastructure.
 *
 * NFR-OBS-01..03, NFR-OBS-06
 * NFR-BUILD-03..05
 */

#include "shellkeep/sk_log.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: level to string ---------------------------------------------- */

static void
test_log_level_to_string(void **state)
{
  (void)state;

  assert_string_equal(sk_log_level_to_string(SK_LOG_LEVEL_ERROR), "ERROR");
  assert_string_equal(sk_log_level_to_string(SK_LOG_LEVEL_WARN), "WARN");
  assert_string_equal(sk_log_level_to_string(SK_LOG_LEVEL_INFO), "INFO");
  assert_string_equal(sk_log_level_to_string(SK_LOG_LEVEL_DEBUG), "DEBUG");
  assert_string_equal(sk_log_level_to_string(SK_LOG_LEVEL_TRACE), "TRACE");

  /* Out of range. */
  assert_string_equal(sk_log_level_to_string((SkLogLevel)99), "UNKNOWN");
}

/* ---- Test: level from string -------------------------------------------- */

static void
test_log_level_from_string(void **state)
{
  (void)state;

  assert_int_equal(sk_log_level_from_string("error"), SK_LOG_LEVEL_ERROR);
  assert_int_equal(sk_log_level_from_string("WARN"), SK_LOG_LEVEL_WARN);
  assert_int_equal(sk_log_level_from_string("Info"), SK_LOG_LEVEL_INFO);
  assert_int_equal(sk_log_level_from_string("debug"), SK_LOG_LEVEL_DEBUG);
  assert_int_equal(sk_log_level_from_string("TRACE"), SK_LOG_LEVEL_TRACE);

  /* Unknown defaults to INFO. */
  assert_int_equal(sk_log_level_from_string("bogus"), SK_LOG_LEVEL_INFO);
  assert_int_equal(sk_log_level_from_string(NULL), SK_LOG_LEVEL_INFO);
  assert_int_equal(sk_log_level_from_string(""), SK_LOG_LEVEL_INFO);
}

/* ---- Test: component to string ------------------------------------------ */

static void
test_log_component_to_string(void **state)
{
  (void)state;

  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_SSH), "ssh");
  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_TERMINAL), "terminal");
  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_STATE), "state");
  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_UI), "ui");
  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_TMUX), "tmux");
  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_SFTP), "sftp");
  assert_string_equal(sk_log_component_to_string(SK_LOG_COMPONENT_GENERAL), "general");

  /* Out of range. */
  assert_string_equal(sk_log_component_to_string((SkLogComponent)99), "unknown");
}

/* ---- Test: component from string ---------------------------------------- */

static void
test_log_component_from_string(void **state)
{
  (void)state;

  assert_int_equal(sk_log_component_from_string("ssh"), SK_LOG_COMPONENT_SSH);
  assert_int_equal(sk_log_component_from_string("TERMINAL"), SK_LOG_COMPONENT_TERMINAL);
  assert_int_equal(sk_log_component_from_string("State"), SK_LOG_COMPONENT_STATE);
  assert_int_equal(sk_log_component_from_string("ui"), SK_LOG_COMPONENT_UI);
  assert_int_equal(sk_log_component_from_string("tmux"), SK_LOG_COMPONENT_TMUX);
  assert_int_equal(sk_log_component_from_string("sftp"), SK_LOG_COMPONENT_SFTP);
  assert_int_equal(sk_log_component_from_string("general"), SK_LOG_COMPONENT_GENERAL);

  /* Unknown defaults to GENERAL. */
  assert_int_equal(sk_log_component_from_string("bogus"), SK_LOG_COMPONENT_GENERAL);
  assert_int_equal(sk_log_component_from_string(NULL), SK_LOG_COMPONENT_GENERAL);
}

/* ---- Test: get/set level ------------------------------------------------ */

static void
test_log_get_set_level(void **state)
{
  (void)state;

  SkLogLevel original = sk_log_get_level();

  sk_log_set_level(SK_LOG_LEVEL_TRACE);
  assert_int_equal(sk_log_get_level(), SK_LOG_LEVEL_TRACE);

  sk_log_set_level(SK_LOG_LEVEL_ERROR);
  assert_int_equal(sk_log_get_level(), SK_LOG_LEVEL_ERROR);

  /* Restore. */
  sk_log_set_level(original);
}

/* ---- Test: level filtering --------------------------------------------- */

static void
test_log_level_filtering(void **state)
{
  (void)state;

  /* Set level to WARN — DEBUG and TRACE should be filtered out.
   * We can't easily capture output from the stub, but we verify
   * that the function doesn't crash and the level check works
   * by inspecting the set/get behavior. */
  sk_log_set_level(SK_LOG_LEVEL_WARN);
  assert_int_equal(sk_log_get_level(), SK_LOG_LEVEL_WARN);

  /* These calls should not crash regardless of level. */
  sk_log_write(SK_LOG_LEVEL_ERROR, SK_LOG_COMPONENT_GENERAL, __FILE__, __LINE__, "test error %d",
               42);
  sk_log_write(SK_LOG_LEVEL_WARN, SK_LOG_COMPONENT_GENERAL, __FILE__, __LINE__, "test warn");
  sk_log_write(SK_LOG_LEVEL_DEBUG, SK_LOG_COMPONENT_GENERAL, __FILE__, __LINE__,
               "test debug (filtered)");
  sk_log_write(SK_LOG_LEVEL_TRACE, SK_LOG_COMPONENT_GENERAL, __FILE__, __LINE__,
               "test trace (filtered)");

  /* Restore. */
  sk_log_set_level(SK_LOG_LEVEL_INFO);
}

/* ---- Test: init and shutdown -------------------------------------------- */

static void
test_log_init_shutdown(void **state)
{
  (void)state;

  /* The stub's init/shutdown are idempotent and safe to call. */
  int rc = sk_log_init(false, false, NULL);
  assert_int_equal(rc, 0);

  /* After init, level should be INFO (default). */
  assert_int_equal(sk_log_get_level(), SK_LOG_LEVEL_INFO);

  sk_log_shutdown();
}

/* ---- Test: init with debug mode ----------------------------------------- */

static void
test_log_init_debug(void **state)
{
  (void)state;

  int rc = sk_log_init(true, false, NULL);
  assert_int_equal(rc, 0);

  assert_int_equal(sk_log_get_level(), SK_LOG_LEVEL_DEBUG);

  sk_log_shutdown();
  /* Reset to default for other tests. */
  sk_log_set_level(SK_LOG_LEVEL_INFO);
}

/* ---- Test: init with trace mode ----------------------------------------- */

static void
test_log_init_trace(void **state)
{
  (void)state;

  int rc = sk_log_init(false, true, NULL);
  assert_int_equal(rc, 0);

  assert_int_equal(sk_log_get_level(), SK_LOG_LEVEL_TRACE);

  sk_log_shutdown();
  sk_log_set_level(SK_LOG_LEVEL_INFO);
}

/* ---- Test: SK_LOG_COMPONENT_COUNT --------------------------------------- */

static void
test_log_component_count(void **state)
{
  (void)state;

  /* Verify the count matches expected number of components. */
  assert_int_equal(SK_LOG_COMPONENT_COUNT, 7);
}

/* ---- Test: macro calls don't crash -------------------------------------- */

static void
test_log_macros(void **state)
{
  (void)state;

  sk_log_set_level(SK_LOG_LEVEL_TRACE);

  /* Exercise all macros. None should crash. */
  SK_LOG_ERROR(SK_LOG_COMPONENT_SSH, "error test %s", "msg");
  SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "warn test %d", 1);
  SK_LOG_INFO(SK_LOG_COMPONENT_STATE, "info test");
  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "debug test %p", (void *)NULL);
  SK_LOG_TRACE(SK_LOG_COMPONENT_TMUX, "trace test");

  sk_log_set_level(SK_LOG_LEVEL_INFO);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_log_level_to_string),
    cmocka_unit_test(test_log_level_from_string),
    cmocka_unit_test(test_log_component_to_string),
    cmocka_unit_test(test_log_component_from_string),
    cmocka_unit_test(test_log_get_set_level),
    cmocka_unit_test(test_log_level_filtering),
    cmocka_unit_test(test_log_init_shutdown),
    cmocka_unit_test(test_log_init_debug),
    cmocka_unit_test(test_log_init_trace),
    cmocka_unit_test(test_log_component_count),
    cmocka_unit_test(test_log_macros),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
