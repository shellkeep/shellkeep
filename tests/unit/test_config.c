// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_config.c
 * @brief Unit tests for INI config parsing, defaults, validation, and clamping.
 *
 * Tests FR-CONFIG-01..03, FR-CONFIG-05..08.
 *
 * NFR-BUILD-03..05
 */

#include "shellkeep/sk_config.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: defaults ----------------------------------------------------- */

static void
test_config_defaults(void **state)
{
  (void)state;

  SkConfig *c = sk_config_new_defaults();
  assert_non_null(c);

  /* [general] */
  assert_null(c->client_id);
  assert_string_equal(c->theme_name, "system");
  assert_int_equal(c->startup_behavior, SK_STARTUP_WELCOME_SCREEN);

  /* [terminal] FR-CONFIG-05 */
  assert_string_equal(c->font_family, "Monospace");
  assert_int_equal(c->font_size, 12);
  assert_int_equal(c->scrollback_lines, 10000);
  assert_int_equal(c->bell, SK_BELL_VISUAL);
  assert_int_equal(c->cursor_shape, SK_CURSOR_BLOCK);
  assert_int_equal(c->cursor_blink, SK_CURSOR_BLINK_SYSTEM);
  assert_true(c->bold_is_bright);
  assert_true(c->allow_hyperlinks);

  /* [ssh] FR-CONFIG-06 */
  assert_int_equal(c->ssh_connect_timeout, 10);
  assert_int_equal(c->ssh_keepalive_interval, 15);
  assert_int_equal(c->ssh_keepalive_count_max, 3);
  assert_true(c->ssh_use_ssh_config);
  assert_int_equal(c->ssh_reconnect_max_attempts, 10);
  assert_true(c->ssh_reconnect_backoff_base > 1.9 && c->ssh_reconnect_backoff_base < 2.1);

  /* [keybindings] */
  assert_string_equal(c->kb_new_tab, "Ctrl+Shift+T");
  assert_string_equal(c->kb_copy, "Ctrl+Shift+C");

  /* [state] */
  assert_int_equal(c->history_max_size_mb, 50);
  assert_int_equal(c->history_max_days, 90);
  assert_int_equal(c->auto_save_interval, 30);

  /* [tray] */
  assert_true(c->tray_enabled);
  assert_true(c->close_to_tray);
  assert_false(c->start_minimized);

  sk_config_free(c);
}

/* ---- Test: free NULL safety --------------------------------------------- */

static void
test_config_free_null(void **state)
{
  (void)state;
  sk_config_free(NULL); /* Should not crash. */
}

/* ---- Test: load from valid INI file ------------------------------------- */

static void
test_config_load_valid(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  const char *ini = "[general]\n"
                    "client_id = my-laptop\n"
                    "theme = dark\n"
                    "startup_behavior = last_session\n"
                    "\n"
                    "[terminal]\n"
                    "font_family = JetBrains Mono\n"
                    "font_size = 14\n"
                    "scrollback_lines = 50000\n"
                    "bell = none\n"
                    "cursor_shape = ibeam\n"
                    "cursor_blink = on\n"
                    "bold_is_bright = false\n"
                    "\n"
                    "[ssh]\n"
                    "connect_timeout = 30\n"
                    "keepalive_interval = 20\n"
                    "keepalive_count_max = 5\n"
                    "reconnect_max_attempts = 20\n"
                    "reconnect_backoff_base = 3.0\n"
                    "\n"
                    "[tray]\n"
                    "enabled = false\n"
                    "close_to_tray = false\n"
                    "start_minimized = true\n";

  char *path = sk_test_write_file(tmpdir, "config.ini", ini);

  GError *error = NULL;
  SkConfig *c = sk_config_load(path, &error);
  assert_non_null(c);

  assert_string_equal(c->client_id, "my-laptop");
  assert_string_equal(c->theme_name, "dark");
  assert_int_equal(c->startup_behavior, SK_STARTUP_LAST_SESSION);
  assert_string_equal(c->font_family, "JetBrains Mono");
  assert_int_equal(c->font_size, 14);
  assert_int_equal(c->scrollback_lines, 50000);
  assert_int_equal(c->bell, SK_BELL_NONE);
  assert_int_equal(c->cursor_shape, SK_CURSOR_IBEAM);
  assert_int_equal(c->cursor_blink, SK_CURSOR_BLINK_ON);
  assert_false(c->bold_is_bright);
  assert_int_equal(c->ssh_connect_timeout, 30);
  assert_int_equal(c->ssh_keepalive_interval, 20);
  assert_int_equal(c->ssh_keepalive_count_max, 5);
  assert_int_equal(c->ssh_reconnect_max_attempts, 20);
  assert_false(c->tray_enabled);
  assert_false(c->close_to_tray);
  assert_true(c->start_minimized);

  sk_config_free(c);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: clamping (FR-CONFIG-03) -------------------------------------- */

static void
test_config_load_clamp(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  const char *ini = "[terminal]\n"
                    "font_size = 2\n"         /* Below min 6 */
                    "scrollback_lines = -5\n" /* Below min 0 */
                    "\n"
                    "[ssh]\n"
                    "connect_timeout = 999\n"          /* Above max 300 */
                    "keepalive_interval = 9999\n"      /* Above max 600 */
                    "keepalive_count_max = 0\n"        /* Below min 1 */
                    "reconnect_max_attempts = 500\n"   /* Above max 100 */
                    "reconnect_backoff_base = 0.01\n"; /* Below min 0.5 */

  char *path = sk_test_write_file(tmpdir, "config.ini", ini);

  GError *error = NULL;
  SkConfig *c = sk_config_load(path, &error);
  assert_non_null(c);

  /* Values should be clamped to valid ranges. */
  assert_int_equal(c->font_size, 6);
  assert_int_equal(c->scrollback_lines, 0);
  assert_int_equal(c->ssh_connect_timeout, 300);
  assert_int_equal(c->ssh_keepalive_interval, 600);
  assert_int_equal(c->ssh_keepalive_count_max, 1);
  assert_int_equal(c->ssh_reconnect_max_attempts, 100);
  assert_true(c->ssh_reconnect_backoff_base >= 0.49 && c->ssh_reconnect_backoff_base <= 0.51);

  sk_config_free(c);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: missing file returns defaults -------------------------------- */

static void
test_config_load_missing_returns_defaults(void **state)
{
  (void)state;

  GError *error = NULL;
  SkConfig *c = sk_config_load("/tmp/does_not_exist_sk_config.ini", &error);
  /* sk_config_load returns defaults when file is missing. */
  assert_non_null(c);
  assert_int_equal(c->font_size, 12);
  assert_string_equal(c->font_family, "Monospace");

  sk_config_free(c);
}

/* ---- Test: client-id validation ----------------------------------------- */

static void
test_config_validate_client_id(void **state)
{
  (void)state;

  /* Valid IDs. */
  assert_true(sk_config_validate_client_id("my-laptop"));
  assert_true(sk_config_validate_client_id("a"));
  assert_true(sk_config_validate_client_id("desktop_home-01"));
  assert_true(sk_config_validate_client_id("UPPER_lower-123"));

  /* UUID v4 format is valid. */
  assert_true(sk_config_validate_client_id("a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d"));

  /* Invalid IDs. */
  assert_false(sk_config_validate_client_id(NULL));
  assert_false(sk_config_validate_client_id(""));
  assert_false(sk_config_validate_client_id("has space"));
  assert_false(sk_config_validate_client_id("has.dot"));
  assert_false(sk_config_validate_client_id("has@symbol"));
  assert_false(sk_config_validate_client_id("has/slash"));

  /* Too long (>64 chars). */
  char too_long[66];
  memset(too_long, 'a', 65);
  too_long[65] = '\0';
  assert_false(sk_config_validate_client_id(too_long));

  /* Exactly 64 chars — should be valid. */
  char exact64[65];
  memset(exact64, 'a', 64);
  exact64[64] = '\0';
  assert_true(sk_config_validate_client_id(exact64));
}

/* ---- Test: get_string accessor ------------------------------------------ */

static void
test_config_get_string(void **state)
{
  (void)state;

  SkConfig *c = sk_config_new_defaults();
  assert_non_null(c);

  const char *val = sk_config_get_string(c, "terminal.font_family");
  assert_non_null(val);
  assert_string_equal(val, "Monospace");

  /* Unknown key returns NULL. */
  val = sk_config_get_string(c, "nonexistent.key");
  assert_null(val);

  sk_config_free(c);
}

/* ---- Test: get_int accessor --------------------------------------------- */

static void
test_config_get_int(void **state)
{
  (void)state;

  SkConfig *c = sk_config_new_defaults();
  assert_non_null(c);

  assert_int_equal(sk_config_get_int(c, "terminal.font_size", -1), 12);
  assert_int_equal(sk_config_get_int(c, "ssh.connect_timeout", -1), 10);

  /* Unknown key returns default. */
  assert_int_equal(sk_config_get_int(c, "nonexistent", 42), 42);

  sk_config_free(c);
}

/* ---- Test: get_bool accessor -------------------------------------------- */

static void
test_config_get_bool(void **state)
{
  (void)state;

  SkConfig *c = sk_config_new_defaults();
  assert_non_null(c);

  assert_true(sk_config_get_bool(c, "tray.enabled", false));
  assert_false(sk_config_get_bool(c, "tray.start_minimized", true));

  /* Unknown key returns default. */
  assert_true(sk_config_get_bool(c, "nonexistent", true));

  sk_config_free(c);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_config_defaults),
    cmocka_unit_test(test_config_free_null),
    cmocka_unit_test(test_config_load_valid),
    cmocka_unit_test(test_config_load_clamp),
    cmocka_unit_test(test_config_load_missing_returns_defaults),
    cmocka_unit_test(test_config_validate_client_id),
    cmocka_unit_test(test_config_get_string),
    cmocka_unit_test(test_config_get_int),
    cmocka_unit_test(test_config_get_bool),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
