// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_permissions.c
 * @brief File and directory permission enforcement.
 *
 * Implements INV-SECURITY-3, NFR-SEC-01, NFR-SEC-02, NFR-SEC-03.
 * All directories: 0700. All files: 0600.
 * Verified on startup and auto-corrected if more permissive.
 */

#include "shellkeep/sk_state.h"

#include <glib.h>

#include <errno.h>
#include <string.h>
#include <sys/stat.h>

/* ---- Internal ----------------------------------------------------------- */

/**
 * Check and fix permissions on a path. Returns TRUE on success.
 */
static bool
fix_perms(const char *path, mode_t desired)
{
  struct stat st;

  if (stat(path, &st) != 0)
  {
    if (errno == ENOENT)
    {
      return true; /* File doesn't exist yet — that's OK. */
    }
    g_warning("sk_permissions: stat('%s') failed: %s", path, g_strerror(errno));
    return false;
  }

  mode_t current = st.st_mode & 0777;
  if (current != desired)
  {
    g_info("sk_permissions: fixing '%s' from %03o to %03o", path, current, desired);
    if (chmod(path, desired) != 0)
    {
      g_warning("sk_permissions: chmod('%s', %03o) failed: %s", path, desired, g_strerror(errno));
      return false;
    }
  }
  return true;
}

/**
 * Recursively fix permissions on all files/dirs under the given path.
 */
static bool
fix_recursive(const char *dir_path)
{
  bool ok = true;

  /* Fix the directory itself. */
  ok = fix_perms(dir_path, SK_DIR_PERMISSIONS) && ok;

  GDir *dir = g_dir_open(dir_path, 0, NULL);
  if (dir == NULL)
  {
    return ok;
  }

  const char *name;
  while ((name = g_dir_read_name(dir)) != NULL)
  {
    g_autofree char *full = g_build_filename(dir_path, name, NULL);

    if (g_file_test(full, G_FILE_TEST_IS_DIR))
    {
      ok = fix_recursive(full) && ok;
    }
    else if (g_file_test(full, G_FILE_TEST_IS_REGULAR))
    {
      ok = fix_perms(full, SK_FILE_PERMISSIONS) && ok;
    }
  }

  g_dir_close(dir);
  return ok;
}

/* ---- Public API --------------------------------------------------------- */

/* INV-SECURITY-3 */
bool
sk_permissions_fix_file(const char *path)
{
  g_return_val_if_fail(path != NULL, false);
  return fix_perms(path, SK_FILE_PERMISSIONS);
}

/* INV-SECURITY-3 */
bool
sk_permissions_fix_dir(const char *path)
{
  g_return_val_if_fail(path != NULL, false);
  return fix_perms(path, SK_DIR_PERMISSIONS);
}

/* NFR-SEC-03 — verify and correct permissions on startup */
bool
sk_permissions_verify_and_fix(void)
{
  bool ok = true;

  /* NFR-SEC-02: Client config dir */
  g_autofree char *config_dir = sk_paths_config_dir();
  if (config_dir != NULL)
  {
    ok = fix_recursive(config_dir) && ok;
  }

  /* NFR-SEC-02: Client data dir */
  g_autofree char *data_dir = sk_paths_data_dir();
  if (data_dir != NULL)
  {
    ok = fix_recursive(data_dir) && ok;
  }

  /* State dir (logs, crashes) */
  g_autofree char *state_dir = sk_paths_state_dir();
  if (state_dir != NULL)
  {
    ok = fix_recursive(state_dir) && ok;
  }

  /* Cache dir */
  g_autofree char *cache_dir = sk_paths_cache_dir();
  if (cache_dir != NULL)
  {
    ok = fix_recursive(cache_dir) && ok;
  }

  /* Runtime dir */
  g_autofree char *runtime_dir = sk_paths_runtime_dir();
  if (runtime_dir != NULL)
  {
    ok = fix_recursive(runtime_dir) && ok;
  }

  return ok;
}
