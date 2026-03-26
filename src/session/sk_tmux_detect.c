// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_tmux_detect.c
 * @brief Tmux detection: run `tmux -V` via SSH, parse version, check minimum.
 *
 * Implements FR-CONN-13..15: verify tmux presence and version on the server.
 */

#include "sk_session_internal.h"
#include <ctype.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/* Version parsing                                                     */
/* ------------------------------------------------------------------ */

/* FR-CONN-13 */
bool
sk_tmux_parse_version(const char *version_str, int *out_major, int *out_minor)
{
  g_return_val_if_fail(version_str != NULL, false);
  g_return_val_if_fail(out_major != NULL, false);
  g_return_val_if_fail(out_minor != NULL, false);

  *out_major = 0;
  *out_minor = 0;

  /* Expected format: "tmux 3.3a" or "tmux 3.0" or just "3.3a".
   * Skip any leading text to find the first digit. */
  const char *p = version_str;
  while (*p != '\0' && !isdigit((unsigned char)*p))
  {
    p++;
  }

  if (*p == '\0')
  {
    return false;
  }

  /* Parse major version. */
  char *end = NULL;
  long major = strtol(p, &end, 10);
  if (end == p || major < 0 || major > 999)
  {
    return false;
  }

  *out_major = (int)major;

  /* Parse minor version if present. */
  if (*end == '.')
  {
    p = end + 1;
    long minor = strtol(p, &end, 10);
    if (end != p && minor >= 0 && minor <= 999)
    {
      *out_minor = (int)minor;
    }
  }

  return true;
}

bool
sk_tmux_version_ok(int major, int minor)
{
  if (major > SK_TMUX_MIN_VERSION_MAJOR)
  {
    return true;
  }
  if (major == SK_TMUX_MIN_VERSION_MAJOR && minor >= SK_TMUX_MIN_VERSION_MINOR)
  {
    return true;
  }
  return false;
}

/* ------------------------------------------------------------------ */
/* Detection via SSH                                                   */
/* ------------------------------------------------------------------ */

/* FR-CONN-13..15 */
bool
sk_tmux_detect(SkSessionManager *mgr, SkTmuxVersion *version, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(version != NULL, false);

  memset(version, 0, sizeof(*version));

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, "tmux -V", &output, error);

  if (rc < 0)
  {
    /* Channel error — error already set. */
    return false;
  }

  if (rc != 0 || output == NULL || output[0] == '\0')
  {
    /* FR-CONN-14: tmux not found. */
    g_free(output);
    g_set_error_literal(error, SK_SESSION_ERROR, SK_SESSION_ERROR_TMUX_NOT_FOUND,
                        "tmux is not installed on the server. "
                        "Install it with: apt install tmux, "
                        "dnf install tmux, pacman -S tmux, "
                        "or brew install tmux.");
    return false;
  }

  /* Strip trailing whitespace. */
  g_strstrip(output);

  int major = 0, minor = 0;
  if (!sk_tmux_parse_version(output, &major, &minor))
  {
    g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_PARSE,
                "Failed to parse tmux version from: %s", output);
    g_free(output);
    return false;
  }

  version->major = major;
  version->minor = minor;
  version->version_string = output; /* Transfer ownership. */

  /* FR-CONN-15: warn if below minimum. */
  if (!sk_tmux_version_ok(major, minor))
  {
    g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_TMUX_VERSION,
                "tmux version %d.%d found, but >= %d.%d is required "
                "(found: %s)",
                major, minor, SK_TMUX_MIN_VERSION_MAJOR, SK_TMUX_MIN_VERSION_MINOR,
                version->version_string);
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Legacy compat wrapper                                               */
/* ------------------------------------------------------------------ */

bool
sk_session_check_tmux_compat(SkSessionManager *mgr, char **version_out, GError **error)
{
  SkTmuxVersion ver = { 0 };
  bool ok = sk_tmux_detect(mgr, &ver, error);

  if (version_out != NULL)
  {
    *version_out = ver.version_string;
  }
  else
  {
    g_free(ver.version_string);
  }

  return ok;
}
