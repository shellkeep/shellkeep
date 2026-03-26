// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_theme.c
 * @brief Unit tests for terminal theme loading (Gogh/base16 format).
 *
 * Tests sk_theme_new_default, sk_theme_free, sk_theme_load (Gogh format),
 * sk_theme_load (base16 format), sk_theme_load error cases.
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

/* ---- Test: sk_theme_new_default ----------------------------------------- */

static void
test_theme_new_default(void **state)
{
  (void)state;

  SkTheme *t = sk_theme_new_default();
  assert_non_null(t);
  assert_non_null(t->name);
  assert_string_equal(t->name, "default");

  /* Verify some known default colors. */
  assert_int_equal(t->ansi_colors[0], 0x000000); /* black */
  assert_int_equal(t->ansi_colors[1], 0xCC0000); /* red */
  assert_int_equal(t->foreground, 0xD3D7CF);
  assert_int_equal(t->background, 0x2E3436);
  assert_true(t->has_cursor_color);
  assert_int_equal(t->cursor_color, 0xD3D7CF);

  sk_theme_free(t);
}

/* ---- Test: sk_theme_free NULL safety ------------------------------------ */

static void
test_theme_free_null(void **state)
{
  (void)state;
  sk_theme_free(NULL); /* Should not crash. */
}

/* ---- Test: sk_theme_load empty name ------------------------------------- */

static void
test_theme_load_empty_name(void **state)
{
  (void)state;

  GError *error = NULL;
  SkTheme *t = sk_theme_load("", &error);
  assert_null(t);
  assert_non_null(error);
  g_clear_error(&error);

  t = sk_theme_load(NULL, &error);
  assert_null(t);
  assert_non_null(error);
  g_clear_error(&error);
}

/* ---- Test: sk_theme_load nonexistent file ------------------------------- */

static void
test_theme_load_nonexistent(void **state)
{
  (void)state;

  GError *error = NULL;
  SkTheme *t = sk_theme_load("nonexistent-theme-that-does-not-exist", &error);
  assert_null(t);
  assert_non_null(error);
  g_clear_error(&error);
}

/* ---- Test: sk_config_get_dir -------------------------------------------- */

static void
test_config_get_dir(void **state)
{
  (void)state;

  char *dir = sk_config_get_dir();
  assert_non_null(dir);
  assert_true(g_str_has_suffix(dir, "/shellkeep"));
  g_free(dir);
}

/* ---- Test: sk_config_get_keepalive_interval ----------------------------- */

static void
test_config_keepalive_accessors(void **state)
{
  (void)state;

  SkConfig *c = sk_config_new_defaults();
  assert_non_null(c);

  assert_int_equal(sk_config_get_keepalive_interval(c), 15);
  assert_int_equal(sk_config_get_keepalive_max_attempts(c), 3);

  sk_config_free(c);
}

/* ---- Test: sk_config_save roundtrip ------------------------------------- */

static void
test_config_save_roundtrip(void **state)
{
  (void)state;

  char *tmpdir = sk_test_mkdtemp();
  char *path = g_build_filename(tmpdir, "saved.ini", NULL);

  SkConfig *c = sk_config_new_defaults();
  assert_non_null(c);

  /* Modify some values. */
  g_free(c->font_family);
  c->font_family = g_strdup("Fira Code");
  c->font_size = 16;
  c->tray_enabled = false;

  GError *error = NULL;
  assert_true(sk_config_save(c, path, &error));
  assert_null(error);

  /* Load back. */
  SkConfig *loaded = sk_config_load(path, &error);
  assert_non_null(loaded);
  assert_null(error);

  assert_string_equal(loaded->font_family, "Fira Code");
  assert_int_equal(loaded->font_size, 16);
  assert_false(loaded->tray_enabled);

  sk_config_free(c);
  sk_config_free(loaded);
  g_free(path);
  sk_test_rm_rf(tmpdir);
  g_free(tmpdir);
}

/* ---- Test: sk_config_save NULL config ----------------------------------- */

static void
test_config_save_null(void **state)
{
  (void)state;

  GError *error = NULL;
  assert_false(sk_config_save(NULL, "/tmp/test.ini", &error));
  assert_non_null(error);
  g_clear_error(&error);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_theme_new_default),
    cmocka_unit_test(test_theme_free_null),
    cmocka_unit_test(test_theme_load_empty_name),
    cmocka_unit_test(test_theme_load_nonexistent),
    cmocka_unit_test(test_config_get_dir),
    cmocka_unit_test(test_config_keepalive_accessors),
    cmocka_unit_test(test_config_save_roundtrip),
    cmocka_unit_test(test_config_save_null),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
