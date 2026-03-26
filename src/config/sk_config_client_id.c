// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_config_client_id.c
 * @brief Client-ID resolution: config, file, or auto-generated UUID v4.
 *
 * FR-CONFIG-08, FR-CLI-02: client-id identifies this device.
 * Stored in $XDG_CONFIG_HOME/shellkeep/client-id.
 * Format: [a-zA-Z0-9_-], max 64 chars.
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_log.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/* Validation — FR-CONFIG-08                                           */
/* ------------------------------------------------------------------ */

bool
sk_config_validate_client_id(const char *id)
{
  size_t len;

  if (id == NULL || id[0] == '\0')
    return false;

  len = strlen(id);
  if (len > 64)
    return false;

  for (size_t i = 0; i < len; i++)
  {
    char ch = id[i];
    if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') ||
          ch == '_' || ch == '-'))
      return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Client-ID file path                                                 */
/* ------------------------------------------------------------------ */

static char *
client_id_file_path(void)
{
  char *dir = sk_config_get_dir();
  char *path = g_build_filename(dir, "client-id", NULL);
  g_free(dir);
  return path;
}

/* ------------------------------------------------------------------ */
/* UUID v4 generation                                                  */
/* ------------------------------------------------------------------ */

/**
 * Generate a UUID v4 string using GLib's random number generator.
 * Format: xxxxxxxx-xxxx-4xxx-Nxxx-xxxxxxxxxxxx where N is 8,9,a,b.
 */
static char *
generate_uuid_v4(void)
{
  /* Use g_uuid_string_random which generates RFC 4122 v4 UUIDs. */
  return g_uuid_string_random();
}

/* ------------------------------------------------------------------ */
/* Read from file                                                      */
/* ------------------------------------------------------------------ */

static char *
read_client_id_file(void)
{
  char *path = client_id_file_path();
  char *contents = NULL;
  gsize length = 0;

  if (!g_file_get_contents(path, &contents, &length, NULL))
  {
    g_free(path);
    return NULL;
  }
  g_free(path);

  /* Trim whitespace */
  g_strstrip(contents);

  if (contents[0] == '\0' || !sk_config_validate_client_id(contents))
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: invalid client-id in file, will regenerate");
    g_free(contents);
    return NULL;
  }

  return contents;
}

/* ------------------------------------------------------------------ */
/* Save to file                                                        */
/* ------------------------------------------------------------------ */

static bool
save_client_id_file(const char *id)
{
  char *dir = sk_config_get_dir();
  char *path;

  /* Ensure directory exists with 0700 permissions */
  if (g_mkdir_with_parents(dir, 0700) != 0)
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: cannot create config directory: %s: %s", dir,
                 strerror(errno));
    g_free(dir);
    return false;
  }
  g_free(dir);

  path = client_id_file_path();

  /* INV-SECURITY-3: write with 0600 from the start to avoid a race
   * window where the file is briefly world-readable (g_file_set_contents
   * creates with 0666 minus umask, then we chmod — but umask may be 0).
   * Use open()+write() to set permissions atomically at creation time. */
  {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0)
    {
      SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: cannot save client-id to %s: %s", path,
                   strerror(errno));
      g_free(path);
      return false;
    }
    size_t id_len = strlen(id);
    ssize_t w = write(fd, id, id_len);
    close(fd);
    if (w < 0 || (size_t)w != id_len)
    {
      SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: write failed for client-id at %s", path);
      g_free(path);
      return false;
    }
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: client-id saved to %s", path);
  g_free(path);
  return true;
}

/* ------------------------------------------------------------------ */
/* Public API — FR-CONFIG-08, FR-CLI-02                                */
/* ------------------------------------------------------------------ */

char *
sk_config_resolve_client_id(const SkConfig *config, GError **error)
{
  char *id;

  /* Priority 1: client_id from config [general] section */
  if (config != NULL && config->client_id != NULL && config->client_id[0] != '\0')
  {
    if (sk_config_validate_client_id(config->client_id))
    {
      SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: using client-id from config");
      return g_strdup(config->client_id);
    }
    else
    {
      SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: invalid client_id in config, ignoring");
    }
  }

  /* Priority 2: read from $XDG_CONFIG_HOME/shellkeep/client-id */
  id = read_client_id_file();
  if (id != NULL)
  {
    SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: using client-id from file");
    return id;
  }

  /* Priority 3: generate new UUID v4 and save */
  id = generate_uuid_v4();
  if (id == NULL)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_GENERIC, "Failed to generate UUID v4 for client-id");
    return NULL;
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: generated new client-id");

  if (!save_client_id_file(id))
  {
    /* Non-fatal: we still return the generated ID for this session */
    SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: could not persist client-id to disk");
  }

  return id;
}
