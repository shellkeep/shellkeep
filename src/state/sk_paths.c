// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_paths.c
 * @brief XDG Base Directory paths for shellkeep.
 *
 * Implements NFR-XDG-01 through NFR-XDG-06.
 * All directories created with 0700 on first use (INV-SECURITY-3).
 */

#include "shellkeep/sk_state.h"

#include <glib.h>

#include <errno.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* ---- Internal helpers --------------------------------------------------- */

/**
 * Ensure a directory exists with 0700 permissions.
 * Creates parent directories as needed.
 */
static bool
ensure_dir(const char *path)
{
  if (g_file_test(path, G_FILE_TEST_IS_DIR))
  {
    /* Directory exists — verify permissions. INV-SECURITY-3 */
    struct stat st;
    if (stat(path, &st) == 0 && (st.st_mode & 0777) != SK_DIR_PERMISSIONS)
    {
      chmod(path, SK_DIR_PERMISSIONS);
    }
    return true;
  }

  if (g_mkdir_with_parents(path, SK_DIR_PERMISSIONS) != 0)
  {
    g_warning("sk_paths: failed to create directory '%s': %s", path, g_strerror(errno));
    return false;
  }

  /* g_mkdir_with_parents may not set exact permissions due to umask. */
  chmod(path, SK_DIR_PERMISSIONS);
  return true;
}

/**
 * Build a path under a given XDG base, append "shellkeep", ensure it exists.
 * Caller must g_free() the result.
 */
static char *
xdg_dir(const char *base)
{
  char *dir = g_build_filename(base, "shellkeep", NULL);
  ensure_dir(dir);
  return dir;
}

/* ---- Public API --------------------------------------------------------- */

/* NFR-XDG-01 */
char *
sk_paths_config_dir(void)
{
  const char *base = g_get_user_config_dir(); /* $XDG_CONFIG_HOME */
  return xdg_dir(base);
}

/* NFR-XDG-02 */
char *
sk_paths_data_dir(void)
{
  const char *base = g_get_user_data_dir(); /* $XDG_DATA_HOME */
  return xdg_dir(base);
}

/* NFR-XDG-03 */
char *
sk_paths_state_dir(void)
{
  /* GLib 2.72+ has g_get_user_state_dir(); fallback to ~/.local/state */
#if GLIB_CHECK_VERSION(2, 72, 0)
  const char *base = g_get_user_state_dir();
#else
  const char *home = g_get_home_dir();
  g_autofree char *base_alloc = g_build_filename(home, ".local", "state", NULL);
  const char *base = base_alloc;
#endif
  return xdg_dir(base);
}

/* NFR-XDG-04 */
char *
sk_paths_runtime_dir(void)
{
  const char *runtime = g_get_user_runtime_dir(); /* $XDG_RUNTIME_DIR */
  return xdg_dir(runtime);
}

/* NFR-XDG-05 */
char *
sk_paths_cache_dir(void)
{
  const char *base = g_get_user_cache_dir(); /* $XDG_CACHE_HOME */
  return xdg_dir(base);
}

/* FR-STATE-01 */
char *
sk_paths_server_cache_dir(const char *host_fingerprint)
{
  g_return_val_if_fail(host_fingerprint != NULL, NULL);

  g_autofree char *data = sk_paths_data_dir();
  char *dir = g_build_filename(data, "cache", "servers", host_fingerprint, NULL);
  ensure_dir(dir);
  return dir;
}

char *
sk_paths_logs_dir(void)
{
  g_autofree char *state = sk_paths_state_dir();
  char *dir = g_build_filename(state, "logs", NULL);
  ensure_dir(dir);
  return dir;
}

char *
sk_paths_crashes_dir(void)
{
  g_autofree char *state = sk_paths_state_dir();
  char *dir = g_build_filename(state, "crashes", NULL);
  ensure_dir(dir);
  return dir;
}
