// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_helpers.h
 * @brief Shared helpers and mock utilities for shellkeep unit tests.
 *
 * Provides temporary directory management, fixture loading, and
 * common assert macros for cmocka-based tests.
 */

#ifndef SK_TEST_HELPERS_H
#define SK_TEST_HELPERS_H

/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */

#include <glib.h>
#include <glib/gstdio.h>

#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/**
 * Create a temporary directory for test use.
 * Caller must g_free() the returned path and clean up after use.
 */
static inline char *
sk_test_mkdtemp(void)
{
  char tmpl[] = "/tmp/sk_test_XXXXXX";
  char *dir = g_mkdtemp(g_strdup(tmpl));
  assert_non_null(dir);
  return dir;
}

/**
 * Recursively remove a temporary directory.
 */
static inline void
sk_test_rm_rf(const char *path)
{
  if (path == NULL)
    return;

  /* Use GLib to iterate directory contents. */
  GDir *dir = g_dir_open(path, 0, NULL);
  if (dir == NULL)
  {
    g_unlink(path);
    return;
  }

  const char *name;
  while ((name = g_dir_read_name(dir)) != NULL)
  {
    char *child = g_build_filename(path, name, NULL);
    if (g_file_test(child, G_FILE_TEST_IS_DIR))
    {
      sk_test_rm_rf(child);
    }
    else
    {
      g_unlink(child);
    }
    g_free(child);
  }
  g_dir_close(dir);
  g_rmdir(path);
}

/**
 * Write a string to a file in a temp directory. Returns absolute path.
 * Caller must g_free() the returned path.
 */
static inline char *
sk_test_write_file(const char *dir, const char *filename, const char *content)
{
  char *path = g_build_filename(dir, filename, NULL);
  GError *error = NULL;
  gboolean ok = g_file_set_contents(path, content, -1, &error);
  if (!ok)
  {
    fprintf(stderr, "sk_test_write_file: %s\n", error ? error->message : "unknown");
    g_clear_error(&error);
  }
  assert_true(ok);
  return path;
}

/**
 * Read a fixture file from tests/fixtures/.
 * Returns NULL-terminated string; caller must g_free().
 */
static inline char *
sk_test_read_fixture(const char *filename)
{
  /* Walk up from test binary to find tests/fixtures. */
  const char *srcdir = g_getenv("MESON_SOURCE_ROOT");
  char *path;
  if (srcdir != NULL)
  {
    path = g_build_filename(srcdir, "tests", "fixtures", filename, NULL);
  }
  else
  {
    path = g_build_filename("tests", "fixtures", filename, NULL);
  }

  char *contents = NULL;
  gsize length = 0;
  g_file_get_contents(path, &contents, &length, NULL);
  g_free(path);
  return contents;
}

#endif /* SK_TEST_HELPERS_H */
