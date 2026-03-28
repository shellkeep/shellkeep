// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_recent.c
 * @brief Recent connections management.
 *
 * Implements Appendix A.3:
 * - Load/save/add recent connections
 * - Merge duplicates (same host+user+port)
 * - Max 50 entries
 * - NEVER save passwords (INV-SECURITY-2, NFR-SEC-08)
 * - File permissions 0600 (NFR-SEC-11)
 *
 * Location: $XDG_DATA_HOME/shellkeep/recent_connections.json
 */

#include "shellkeep/sk_state.h"

#include <glib.h>
#include <glib/gstdio.h>

#include <errno.h>
#include <fcntl.h>
#include <json-glib/json-glib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#ifdef _WIN32
#include <io.h>
#define fsync(fd) _commit(fd)
#endif

/* ---- Helpers ------------------------------------------------------------ */

/**
 * Get the path to the recent connections file.
 * Caller must g_free().
 */
static char *
recent_file_path(void)
{
  g_autofree char *data_dir = sk_paths_data_dir();
  return g_build_filename(data_dir, "recent_connections.json", NULL);
}

static char *
now_iso8601(void)
{
  GDateTime *now = g_date_time_new_now_utc();
  char *ts = g_date_time_format_iso8601(now);
  g_date_time_unref(now);
  return ts;
}

/* ---- Memory Management -------------------------------------------------- */

SkRecentConnections *
sk_recent_new(void)
{
  SkRecentConnections *rc = g_new0(SkRecentConnections, 1);
  rc->schema_version = SK_RECENT_SCHEMA_VERSION;
  rc->connections = NULL;
  rc->n_connections = 0;
  return rc;
}

void
sk_recent_connection_free(SkRecentConnection *conn)
{
  if (conn == NULL)
    return;
  g_free(conn->host);
  g_free(conn->user);
  g_free(conn->alias);
  g_free(conn->last_connected);
  g_free(conn->host_key_fingerprint);
  g_free(conn);
}

void
sk_recent_free(SkRecentConnections *recent)
{
  if (recent == NULL)
    return;
  for (int i = 0; i < recent->n_connections; i++)
  {
    sk_recent_connection_free(recent->connections[i]);
  }
  g_free(recent->connections);
  g_free(recent);
}

/* ---- JSON Serialization ------------------------------------------------- */

static char *
recent_to_json(const SkRecentConnections *recent)
{
  JsonObject *root = json_object_new();
  json_object_set_int_member(root, "schema_version", recent->schema_version);

  JsonArray *arr = json_array_new();
  for (int i = 0; i < recent->n_connections; i++)
  {
    SkRecentConnection *c = recent->connections[i];
    JsonObject *obj = json_object_new();

    json_object_set_string_member(obj, "host", c->host ? c->host : "");
    json_object_set_string_member(obj, "user", c->user ? c->user : "");
    json_object_set_int_member(obj, "port", c->port);

    if (c->alias != NULL && c->alias[0] != '\0')
    {
      json_object_set_string_member(obj, "alias", c->alias);
    }
    json_object_set_string_member(obj, "last_connected",
                                  c->last_connected ? c->last_connected : "");
    if (c->host_key_fingerprint != NULL && c->host_key_fingerprint[0] != '\0')
    {
      json_object_set_string_member(obj, "host_key_fingerprint", c->host_key_fingerprint);
    }

    /* INV-SECURITY-2: NEVER include password fields. */

    JsonNode *node = json_node_new(JSON_NODE_OBJECT);
    json_node_take_object(node, obj);
    json_array_add_element(arr, node);
  }

  json_object_set_array_member(root, "connections", arr);

  JsonNode *root_node = json_node_new(JSON_NODE_OBJECT);
  json_node_take_object(root_node, root);

  JsonGenerator *gen = json_generator_new();
  json_generator_set_pretty(gen, TRUE);
  json_generator_set_indent(gen, 2);
  json_generator_set_root(gen, root_node);

  gsize len;
  char *json = json_generator_to_data(gen, &len);

  g_object_unref(gen);
  json_node_unref(root_node);

  return json;
}

static SkRecentConnections *
recent_from_json(const char *json, GError **error)
{
  JsonParser *parser = json_parser_new();
  if (!json_parser_load_from_data(parser, json, -1, error))
  {
    g_object_unref(parser);
    return NULL;
  }

  JsonNode *root_node = json_parser_get_root(parser);
  if (root_node == NULL || !JSON_NODE_HOLDS_OBJECT(root_node))
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_PARSE,
                "Recent connections JSON root is not an object");
    g_object_unref(parser);
    return NULL;
  }

  JsonObject *root = json_node_get_object(root_node);

  SkRecentConnections *rc = sk_recent_new();
  rc->schema_version = (int)json_object_get_int_member_with_default(root, "schema_version", 1);

  if (json_object_has_member(root, "connections"))
  {
    JsonArray *arr = json_object_get_array_member(root, "connections");
    guint len = json_array_get_length(arr);

    rc->n_connections = (int)len;
    rc->connections = g_new0(SkRecentConnection *, len + 1);

    for (guint i = 0; i < len; i++)
    {
      JsonObject *obj = json_array_get_object_element(arr, i);
      SkRecentConnection *c = g_new0(SkRecentConnection, 1);

      c->host = g_strdup(json_object_get_string_member_with_default(obj, "host", ""));
      c->user = g_strdup(json_object_get_string_member_with_default(obj, "user", ""));
      c->port = (int)json_object_get_int_member_with_default(obj, "port", 22);
      c->last_connected =
          g_strdup(json_object_get_string_member_with_default(obj, "last_connected", ""));

      if (json_object_has_member(obj, "alias"))
      {
        c->alias = g_strdup(json_object_get_string_member(obj, "alias"));
      }
      if (json_object_has_member(obj, "host_key_fingerprint"))
      {
        c->host_key_fingerprint =
            g_strdup(json_object_get_string_member(obj, "host_key_fingerprint"));
      }

      rc->connections[i] = c;
    }
  }

  g_object_unref(parser);
  return rc;
}

/* ---- Public API --------------------------------------------------------- */

SkRecentConnections *
sk_recent_load(GError **error)
{
  g_autofree char *path = recent_file_path();

  if (!g_file_test(path, G_FILE_TEST_EXISTS))
  {
    /* No file yet — return empty list. */
    return sk_recent_new();
  }

  gchar *contents = NULL;
  gsize length = 0;
  GError *read_err = NULL;

  if (!g_file_get_contents(path, &contents, &length, &read_err))
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to read recent connections: %s",
                read_err ? read_err->message : "unknown");
    g_clear_error(&read_err);
    return sk_recent_new();
  }

  SkRecentConnections *rc = recent_from_json(contents, error);
  g_free(contents);

  if (rc == NULL)
  {
    return sk_recent_new();
  }

  return rc;
}

/* NFR-SEC-11: file permissions 0600. */
bool
sk_recent_save(const SkRecentConnections *recent, GError **error)
{
  g_return_val_if_fail(recent != NULL, false);

  g_autofree char *path = recent_file_path();
  char *json = recent_to_json(recent);
  if (json == NULL)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to serialize recent connections");
    return false;
  }

  /* Atomic write via tmp+rename. */
  g_autofree char *tmp = g_strdup_printf("%s.tmp", path);

  int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, SK_FILE_PERMISSIONS);
  if (fd < 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to open '%s': %s", tmp,
                g_strerror(errno));
    g_free(json);
    return false;
  }

  gsize len = strlen(json);
  gssize total = 0;
  while ((gsize)total < len)
  {
    gssize w = write(fd, json + total, len - (gsize)total);
    if (w < 0)
    {
      if (errno == EINTR)
        continue;
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Write failed: %s", g_strerror(errno));
      close(fd);
      g_unlink(tmp);
      g_free(json);
      return false;
    }
    total += w;
  }

  fsync(fd);
  close(fd);
  g_free(json);

  if (g_rename(tmp, path) != 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Rename failed: %s", g_strerror(errno));
    g_unlink(tmp);
    return false;
  }

  sk_permissions_fix_file(path);
  return true;
}

/* Add or merge a connection. Duplicates (same host+user+port) merged. */
void
sk_recent_add(SkRecentConnections *recent, const char *host, const char *user, int port,
              const char *alias, const char *host_key_fingerprint)
{
  g_return_if_fail(recent != NULL);
  g_return_if_fail(host != NULL);
  g_return_if_fail(user != NULL);

  /* INV-SECURITY-2: This function never accepts or stores passwords. */

  g_autofree char *ts = now_iso8601();

  /* Look for existing entry with same host+user+port. */
  for (int i = 0; i < recent->n_connections; i++)
  {
    SkRecentConnection *c = recent->connections[i];
    if (g_strcmp0(c->host, host) == 0 && g_strcmp0(c->user, user) == 0 && c->port == port)
    {
      /* Merge: update timestamp and optional fields. */
      g_free(c->last_connected);
      c->last_connected = g_strdup(ts);

      if (alias != NULL)
      {
        g_free(c->alias);
        c->alias = g_strdup(alias);
      }
      if (host_key_fingerprint != NULL)
      {
        g_free(c->host_key_fingerprint);
        c->host_key_fingerprint = g_strdup(host_key_fingerprint);
      }

      /* Move to front (most recent first). */
      if (i > 0)
      {
        SkRecentConnection *tmp = recent->connections[i];
        memmove(&recent->connections[1], &recent->connections[0],
                (size_t)i * sizeof(SkRecentConnection *));
        recent->connections[0] = tmp;
      }
      return;
    }
  }

  /* New entry — add at front. */
  SkRecentConnection *c = g_new0(SkRecentConnection, 1);
  c->host = g_strdup(host);
  c->user = g_strdup(user);
  c->port = port;
  c->alias = g_strdup(alias);
  c->last_connected = g_strdup(ts);
  c->host_key_fingerprint = g_strdup(host_key_fingerprint);

  /* Grow array. */
  int new_count = recent->n_connections + 1;
  recent->connections =
      g_realloc(recent->connections, sizeof(SkRecentConnection *) * (size_t)(new_count + 1));

  /* Shift existing entries right. */
  if (recent->n_connections > 0)
  {
    memmove(&recent->connections[1], &recent->connections[0],
            (size_t)recent->n_connections * sizeof(SkRecentConnection *));
  }
  recent->connections[0] = c;
  recent->connections[new_count] = NULL;
  recent->n_connections = new_count;

  /* Enforce max 50 entries. */
  while (recent->n_connections > SK_RECENT_MAX_ENTRIES)
  {
    int last = recent->n_connections - 1;
    sk_recent_connection_free(recent->connections[last]);
    recent->connections[last] = NULL;
    recent->n_connections--;
  }
}
