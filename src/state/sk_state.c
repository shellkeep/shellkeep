// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_state.c
 * @brief Core state persistence — structs, JSON I/O, versioning, corruption.
 *
 * Implements:
 * - State structs (Appendix A.1)
 * - Atomic read/write via tmp+rename (INV-STATE-1, FR-STATE-04)
 * - Schema versioning and migration (FR-STATE-08)
 * - Corruption handling (FR-STATE-13..17, Appendix A.5)
 * - Orphan .tmp cleanup (FR-STATE-07)
 * - State validation (FR-STATE-16)
 * - Local cache (FR-STATE-01..02)
 */

#include "shellkeep/sk_state.h"

#include <glib.h>
#include <glib/gstdio.h>

#include <errno.h>
#include <fcntl.h>
#include <json-glib/json-glib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#ifdef _WIN32
#include <io.h>
#define fsync(fd) _commit(fd)
#endif

/* ---- Error Quark -------------------------------------------------------- */

G_DEFINE_QUARK(sk - state - error - quark, sk_state_error)

/* ---- Memory Management -------------------------------------------------- */

SkTab *
sk_tab_new(const char *session_uuid, const char *tmux_session_name, const char *title, int position)
{
  SkTab *tab = g_new0(SkTab, 1);
  tab->session_uuid = g_strdup(session_uuid);
  tab->tmux_session_name = g_strdup(tmux_session_name);
  tab->title = g_strdup(title);
  tab->position = position;
  return tab;
}

void
sk_tab_free(SkTab *tab)
{
  if (tab == NULL)
    return;
  g_free(tab->session_uuid);
  g_free(tab->tmux_session_name);
  g_free(tab->title);
  g_free(tab);
}

SkWindow *
sk_window_new(const char *id, const char *title)
{
  SkWindow *win = g_new0(SkWindow, 1);
  win->id = g_strdup(id);
  win->title = g_strdup(title);
  win->visible = true;
  win->tabs = NULL;
  win->n_tabs = 0;
  win->active_tab = 0;
  win->geometry.is_set = false;
  return win;
}

void
sk_window_free(SkWindow *win)
{
  if (win == NULL)
    return;
  g_free(win->id);
  g_free(win->title);
  for (int i = 0; i < win->n_tabs; i++)
  {
    sk_tab_free(win->tabs[i]);
  }
  g_free(win->tabs);
  g_free(win);
}

SkEnvironment *
sk_environment_new(const char *name)
{
  SkEnvironment *env = g_new0(SkEnvironment, 1);
  env->name = g_strdup(name);
  env->windows = NULL;
  env->n_windows = 0;
  return env;
}

void
sk_environment_free(SkEnvironment *env)
{
  if (env == NULL)
    return;
  g_free(env->name);
  for (int i = 0; i < env->n_windows; i++)
  {
    sk_window_free(env->windows[i]);
  }
  g_free(env->windows);
  g_free(env);
}

SkStateFile *
sk_state_file_new(const char *client_id)
{
  SkStateFile *state = g_new0(SkStateFile, 1);
  state->schema_version = SK_STATE_SCHEMA_VERSION;
  state->client_id = g_strdup(client_id);
  state->environments = NULL;
  state->n_environments = 0;
  state->last_environment = NULL;

  /* Set last_modified to current UTC. */
  GDateTime *now = g_date_time_new_now_utc();
  state->last_modified = g_date_time_format_iso8601(now);
  g_date_time_unref(now);

  return state;
}

void
sk_state_file_free(SkStateFile *state)
{
  if (state == NULL)
    return;
  g_free(state->last_modified);
  g_free(state->client_id);
  g_free(state->last_environment);
  for (int i = 0; i < state->n_environments; i++)
  {
    sk_environment_free(state->environments[i]);
  }
  g_free(state->environments);
  g_free(state);
}

/* ---- Deep Copy (for debounce) ------------------------------------------- */

static SkTab *
sk_tab_copy(const SkTab *src)
{
  if (src == NULL)
    return NULL;
  return sk_tab_new(src->session_uuid, src->tmux_session_name, src->title, src->position);
}

static SkWindow *
sk_window_copy(const SkWindow *src)
{
  if (src == NULL)
    return NULL;
  SkWindow *win = sk_window_new(src->id, src->title);
  win->visible = src->visible;
  win->geometry = src->geometry;
  win->active_tab = src->active_tab;
  win->n_tabs = src->n_tabs;
  win->tabs = g_new0(SkTab *, src->n_tabs + 1);
  for (int i = 0; i < src->n_tabs; i++)
  {
    win->tabs[i] = sk_tab_copy(src->tabs[i]);
  }
  return win;
}

static SkEnvironment *
sk_environment_copy(const SkEnvironment *src)
{
  if (src == NULL)
    return NULL;
  SkEnvironment *env = sk_environment_new(src->name);
  env->n_windows = src->n_windows;
  env->windows = g_new0(SkWindow *, src->n_windows + 1);
  for (int i = 0; i < src->n_windows; i++)
  {
    env->windows[i] = sk_window_copy(src->windows[i]);
  }
  return env;
}

/**
 * Deep copy of an entire state file.
 * Used by debounce to snapshot state.
 */
static SkStateFile *
sk_state_file_copy(const SkStateFile *src)
{
  if (src == NULL)
    return NULL;
  SkStateFile *state = g_new0(SkStateFile, 1);
  state->schema_version = src->schema_version;
  state->last_modified = g_strdup(src->last_modified);
  state->client_id = g_strdup(src->client_id);
  state->last_environment = g_strdup(src->last_environment);
  state->n_environments = src->n_environments;
  state->environments = g_new0(SkEnvironment *, src->n_environments + 1);
  for (int i = 0; i < src->n_environments; i++)
  {
    state->environments[i] = sk_environment_copy(src->environments[i]);
  }
  return state;
}

/* ---- JSON Serialization ------------------------------------------------- */

static JsonNode *
tab_to_json(const SkTab *tab)
{
  JsonObject *obj = json_object_new();
  json_object_set_string_member(obj, "session_uuid", tab->session_uuid);
  json_object_set_string_member(obj, "tmux_session_name", tab->tmux_session_name);
  json_object_set_string_member(obj, "title", tab->title);
  json_object_set_int_member(obj, "position", tab->position);

  JsonNode *node = json_node_new(JSON_NODE_OBJECT);
  json_node_take_object(node, obj);
  return node;
}

static JsonNode *
window_to_json(const SkWindow *win)
{
  JsonObject *obj = json_object_new();
  json_object_set_string_member(obj, "id", win->id);
  json_object_set_string_member(obj, "title", win->title);
  json_object_set_boolean_member(obj, "visible", win->visible);

  if (win->geometry.is_set)
  {
    JsonObject *geo = json_object_new();
    json_object_set_int_member(geo, "x", win->geometry.x);
    json_object_set_int_member(geo, "y", win->geometry.y);
    json_object_set_int_member(geo, "width", win->geometry.width);
    json_object_set_int_member(geo, "height", win->geometry.height);
    json_object_set_object_member(obj, "geometry", geo);
  }

  JsonArray *tabs_arr = json_array_new();
  for (int i = 0; i < win->n_tabs; i++)
  {
    json_array_add_element(tabs_arr, tab_to_json(win->tabs[i]));
  }
  json_object_set_array_member(obj, "tabs", tabs_arr);
  json_object_set_int_member(obj, "active_tab", win->active_tab);

  JsonNode *node = json_node_new(JSON_NODE_OBJECT);
  json_node_take_object(node, obj);
  return node;
}

static JsonNode *
environment_to_json(const SkEnvironment *env)
{
  JsonArray *windows_arr = json_array_new();
  for (int i = 0; i < env->n_windows; i++)
  {
    json_array_add_element(windows_arr, window_to_json(env->windows[i]));
  }

  JsonObject *obj = json_object_new();
  json_object_set_array_member(obj, "windows", windows_arr);

  JsonNode *node = json_node_new(JSON_NODE_OBJECT);
  json_node_take_object(node, obj);
  return node;
}

char *
sk_state_to_json(const SkStateFile *state)
{
  g_return_val_if_fail(state != NULL, NULL);

  JsonObject *root = json_object_new();
  json_object_set_int_member(root, "schema_version", state->schema_version);
  json_object_set_string_member(root, "last_modified",
                                state->last_modified ? state->last_modified : "");
  json_object_set_string_member(root, "client_id", state->client_id ? state->client_id : "");

  /* Environments map. */
  JsonObject *envs = json_object_new();
  for (int i = 0; i < state->n_environments; i++)
  {
    SkEnvironment *env = state->environments[i];
    json_object_set_member(envs, env->name, environment_to_json(env));
  }
  json_object_set_object_member(root, "environments", envs);

  json_object_set_string_member(root, "last_environment",
                                state->last_environment ? state->last_environment : "");

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

/* ---- JSON Deserialization ----------------------------------------------- */

static SkTab *
tab_from_json(JsonObject *obj)
{
  if (obj == NULL)
    return NULL;

  const char *uuid = json_object_get_string_member_with_default(obj, "session_uuid", "");
  const char *tmux = json_object_get_string_member_with_default(obj, "tmux_session_name", "");
  const char *title = json_object_get_string_member_with_default(obj, "title", "");
  int pos = (int)json_object_get_int_member_with_default(obj, "position", 0);

  return sk_tab_new(uuid, tmux, title, pos);
}

static SkWindow *
window_from_json(JsonObject *obj)
{
  if (obj == NULL)
    return NULL;

  const char *id = json_object_get_string_member_with_default(obj, "id", "");
  const char *title = json_object_get_string_member_with_default(obj, "title", "");

  SkWindow *win = sk_window_new(id, title);
  win->visible = json_object_get_boolean_member_with_default(obj, "visible", true);
  win->active_tab = (int)json_object_get_int_member_with_default(obj, "active_tab", 0);

  /* Geometry (optional). */
  if (json_object_has_member(obj, "geometry"))
  {
    JsonObject *geo = json_object_get_object_member(obj, "geometry");
    if (geo != NULL)
    {
      win->geometry.is_set = true;
      win->geometry.x = (int)json_object_get_int_member_with_default(geo, "x", 0);
      win->geometry.y = (int)json_object_get_int_member_with_default(geo, "y", 0);
      win->geometry.width = (int)json_object_get_int_member_with_default(geo, "width", 800);
      win->geometry.height = (int)json_object_get_int_member_with_default(geo, "height", 600);
    }
  }

  /* Tabs. */
  if (json_object_has_member(obj, "tabs"))
  {
    JsonArray *arr = json_object_get_array_member(obj, "tabs");
    guint len = json_array_get_length(arr);
    win->n_tabs = (int)len;
    win->tabs = g_new0(SkTab *, len + 1);
    for (guint i = 0; i < len; i++)
    {
      JsonObject *tab_obj = json_array_get_object_element(arr, i);
      win->tabs[i] = tab_from_json(tab_obj);
    }
  }

  return win;
}

static SkEnvironment *
environment_from_json(const char *name, JsonObject *obj)
{
  if (obj == NULL)
    return NULL;

  SkEnvironment *env = sk_environment_new(name);

  if (json_object_has_member(obj, "windows"))
  {
    JsonArray *arr = json_object_get_array_member(obj, "windows");
    guint len = json_array_get_length(arr);
    env->n_windows = (int)len;
    env->windows = g_new0(SkWindow *, len + 1);
    for (guint i = 0; i < len; i++)
    {
      JsonObject *win_obj = json_array_get_object_element(arr, i);
      env->windows[i] = window_from_json(win_obj);
    }
  }

  return env;
}

SkStateFile *
sk_state_from_json(const char *json, GError **error)
{
  g_return_val_if_fail(json != NULL, NULL);

  JsonParser *parser = json_parser_new();
  if (!json_parser_load_from_data(parser, json, -1, error))
  {
    g_object_unref(parser);
    return NULL;
  }

  JsonNode *root_node = json_parser_get_root(parser);
  if (root_node == NULL || !JSON_NODE_HOLDS_OBJECT(root_node))
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_PARSE, "State JSON root is not an object");
    g_object_unref(parser);
    return NULL;
  }

  JsonObject *root = json_node_get_object(root_node);

  SkStateFile *state = g_new0(SkStateFile, 1);
  state->schema_version = (int)json_object_get_int_member_with_default(root, "schema_version", 0);
  state->last_modified =
      g_strdup(json_object_get_string_member_with_default(root, "last_modified", ""));
  state->client_id = g_strdup(json_object_get_string_member_with_default(root, "client_id", ""));
  state->last_environment =
      g_strdup(json_object_get_string_member_with_default(root, "last_environment", ""));

  /* Parse environments map. */
  if (json_object_has_member(root, "environments"))
  {
    JsonObject *envs_obj = json_object_get_object_member(root, "environments");
    if (envs_obj != NULL)
    {
      GList *members = json_object_get_members(envs_obj);
      guint count = g_list_length(members);
      state->n_environments = (int)count;
      state->environments = g_new0(SkEnvironment *, count + 1);

      int idx = 0;
      for (GList *l = members; l != NULL; l = l->next)
      {
        const char *env_name = (const char *)l->data;
        JsonObject *env_obj = json_object_get_object_member(envs_obj, env_name);
        state->environments[idx] = environment_from_json(env_name, env_obj);
        idx++;
      }
      g_list_free(members);
    }
  }

  g_object_unref(parser);
  return state;
}

/* ---- Validation (FR-STATE-16) ------------------------------------------- */

/**
 * Validate tmux session name regex: ^[a-zA-Z0-9_][a-zA-Z0-9_.:-]*$
 */
static bool
validate_tmux_name(const char *name)
{
  if (name == NULL || name[0] == '\0')
    return false;

  /* First char: [a-zA-Z0-9_] */
  char c = name[0];
  if (!g_ascii_isalnum(c) && c != '_')
    return false;

  /* Rest: [a-zA-Z0-9_.:-] */
  for (int i = 1; name[i] != '\0'; i++)
  {
    c = name[i];
    if (!g_ascii_isalnum(c) && c != '_' && c != '.' && c != ':' && c != '-')
    {
      return false;
    }
  }
  return true;
}

bool
sk_state_validate(const SkStateFile *state, GError **error)
{
  g_return_val_if_fail(state != NULL, false);

  /* 1. schema_version must be positive integer. */
  if (state->schema_version <= 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_SCHEMA,
                "schema_version must be a positive integer, got %d", state->schema_version);
    return false;
  }

  /* 2. last_environment must reference an existing key. */
  if (state->last_environment != NULL && state->last_environment[0] != '\0')
  {
    bool found = false;
    for (int i = 0; i < state->n_environments; i++)
    {
      if (g_strcmp0(state->environments[i]->name, state->last_environment) == 0)
      {
        found = true;
        break;
      }
    }
    if (!found)
    {
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_SCHEMA,
                  "last_environment '%s' not found in environments", state->last_environment);
      return false;
    }
  }

  /* 3. All session_uuid must be unique across the entire file. */
  GHashTable *uuids = g_hash_table_new(g_str_hash, g_str_equal);
  for (int ei = 0; ei < state->n_environments; ei++)
  {
    SkEnvironment *env = state->environments[ei];
    for (int wi = 0; wi < env->n_windows; wi++)
    {
      SkWindow *win = env->windows[wi];
      for (int ti = 0; ti < win->n_tabs; ti++)
      {
        SkTab *tab = win->tabs[ti];
        if (tab->session_uuid != NULL && tab->session_uuid[0] != '\0')
        {
          if (g_hash_table_contains(uuids, tab->session_uuid))
          {
            g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_SCHEMA, "Duplicate session_uuid: %s",
                        tab->session_uuid);
            g_hash_table_destroy(uuids);
            return false;
          }
          g_hash_table_add(uuids, tab->session_uuid);
        }
      }
    }
  }
  g_hash_table_destroy(uuids);

  /* 4. active_tab is valid index. */
  for (int ei = 0; ei < state->n_environments; ei++)
  {
    SkEnvironment *env = state->environments[ei];
    for (int wi = 0; wi < env->n_windows; wi++)
    {
      SkWindow *win = env->windows[wi];
      if (win->n_tabs > 0 && (win->active_tab < 0 || win->active_tab >= win->n_tabs))
      {
        g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_SCHEMA,
                    "active_tab %d out of range [0, %d) in window %s", win->active_tab, win->n_tabs,
                    win->id);
        return false;
      }
    }
  }

  /* 5. tmux_session_name matches regex. */
  for (int ei = 0; ei < state->n_environments; ei++)
  {
    SkEnvironment *env = state->environments[ei];
    for (int wi = 0; wi < env->n_windows; wi++)
    {
      SkWindow *win = env->windows[wi];
      for (int ti = 0; ti < win->n_tabs; ti++)
      {
        SkTab *tab = win->tabs[ti];
        if (tab->tmux_session_name != NULL && tab->tmux_session_name[0] != '\0' &&
            !validate_tmux_name(tab->tmux_session_name))
        {
          g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_SCHEMA,
                      "Invalid tmux_session_name: '%s'", tab->tmux_session_name);
          return false;
        }
      }
    }
  }

  return true;
}

/* ---- Versioning & Migration (FR-STATE-08) ------------------------------- */

/**
 * Create a backup of the file as <file>.v<version>.bak.
 */
static bool
create_version_backup(const char *path, int old_version)
{
  g_autofree char *backup = g_strdup_printf("%s.v%d.bak", path, old_version);

  GError *err = NULL;
  GFile *src = g_file_new_for_path(path);
  GFile *dst = g_file_new_for_path(backup);
  bool ok = g_file_copy(src, dst, G_FILE_COPY_OVERWRITE, NULL, NULL, NULL, &err);
  if (!ok)
  {
    g_warning("sk_state: failed to create backup '%s': %s", backup, err ? err->message : "unknown");
    g_clear_error(&err);
  }
  g_object_unref(src);
  g_object_unref(dst);
  return ok;
}

/**
 * Migrate state from old_version to current version.
 * Currently only version 1 exists, so this is a placeholder for future
 * migrations. Each migration step transforms in place.
 */
static bool
migrate_state(SkStateFile *state, int old_version, GError **error)
{
  (void)error;

  /*
   * Migration chain: v1 -> v2 -> v3 -> ...
   * When we add version 2, add a migration step here.
   */
  if (old_version < 1)
  {
    /* Pre-v1 state is not supported. */
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_SCHEMA,
                "State version %d is too old to migrate", old_version);
    return false;
  }

  /* Already at current version — nothing to do. */
  state->schema_version = SK_STATE_SCHEMA_VERSION;
  return true;
}

/* ---- Corruption Handling (Appendix A.5) --------------------------------- */

/**
 * Rename a corrupt file to <path>.corrupt.<timestamp>.
 */
static void
rename_corrupt(const char *path)
{
  GDateTime *now = g_date_time_new_now_utc();
  g_autofree char *ts = g_date_time_format(now, "%Y%m%dT%H%M%SZ");
  g_date_time_unref(now);

  g_autofree char *corrupt_path = g_strdup_printf("%s.corrupt.%s", path, ts);

  if (g_rename(path, corrupt_path) != 0)
  {
    g_warning("sk_state: failed to rename corrupt file '%s' to '%s': %s", path, corrupt_path,
              g_strerror(errno));
  }
  else
  {
    g_info("sk_state: renamed corrupt file '%s' to '%s'", path, corrupt_path);
  }
}

/* ---- Atomic File Operations --------------------------------------------- */

/**
 * Generate an ISO 8601 UTC timestamp string. Caller must g_free().
 */
static char *
now_iso8601(void)
{
  GDateTime *now = g_date_time_new_now_utc();
  char *ts = g_date_time_format_iso8601(now);
  g_date_time_unref(now);
  return ts;
}

/* FR-STATE-07: clean orphan .tmp files. */
void
sk_state_cleanup_tmp_files(const char *dir_path)
{
  g_return_if_fail(dir_path != NULL);

  GDir *dir = g_dir_open(dir_path, 0, NULL);
  if (dir == NULL)
    return;

  const char *name;
  while ((name = g_dir_read_name(dir)) != NULL)
  {
    if (g_str_has_suffix(name, ".tmp"))
    {
      g_autofree char *full = g_build_filename(dir_path, name, NULL);
      g_info("sk_state: removing orphan tmp file '%s'", full);
      g_unlink(full);
    }
  }
  g_dir_close(dir);
}

/* FR-STATE-04, INV-STATE-1: atomic save via tmp+rename. */
bool
sk_state_save(SkStateFile *state, const char *path, GError **error)
{
  g_return_val_if_fail(state != NULL, false);
  g_return_val_if_fail(path != NULL, false);

  /* Update last_modified. */
  g_free(state->last_modified);
  state->last_modified = now_iso8601();

  char *json = sk_state_to_json(state);
  if (json == NULL)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to serialize state to JSON");
    return false;
  }

  /* Write to tmp file in same directory, then rename. */
  g_autofree char *dir = g_path_get_dirname(path);
  g_autofree char *tmp = g_strdup_printf("%s.tmp", path);

  /* Ensure parent directory exists. */
  if (!g_file_test(dir, G_FILE_TEST_IS_DIR))
  {
    g_mkdir_with_parents(dir, SK_DIR_PERMISSIONS);
  }

  /* Write tmp file. */
  int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, SK_FILE_PERMISSIONS);
  if (fd < 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to open tmp file '%s': %s", tmp,
                g_strerror(errno));
    g_free(json);
    return false;
  }

  gsize len = strlen(json);
  gssize written = 0;
  gssize total = 0;
  while ((gsize)total < len)
  {
    written = write(fd, json + total, len - (gsize)total);
    if (written < 0)
    {
      if (errno == EINTR)
        continue;
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to write tmp file '%s': %s",
                  tmp, g_strerror(errno));
      close(fd);
      g_unlink(tmp);
      g_free(json);
      return false;
    }
    total += written;
  }

  if (fsync(fd) != 0)
  {
    g_warning("sk_state: fsync('%s') failed: %s", tmp, g_strerror(errno));
  }
  close(fd);
  g_free(json);

  /* Atomic rename. */
  if (g_rename(tmp, path) != 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to rename '%s' -> '%s': %s", tmp,
                path, g_strerror(errno));
    g_unlink(tmp);
    return false;
  }

  /* Ensure 0600 permissions. INV-SECURITY-3 */
  sk_permissions_fix_file(path);

  return true;
}

/* Load state with versioning, migration, corruption handling. */
SkStateFile *
sk_state_load(const char *path, GError **error)
{
  g_return_val_if_fail(path != NULL, NULL);

  if (!g_file_test(path, G_FILE_TEST_EXISTS))
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "State file not found: %s", path);
    return NULL;
  }

  gchar *contents = NULL;
  gsize length = 0;
  GError *load_err = NULL;

  if (!g_file_get_contents(path, &contents, &length, &load_err))
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to read '%s': %s", path,
                load_err ? load_err->message : "unknown");
    g_clear_error(&load_err);
    return NULL;
  }

  /* Try to parse JSON. On failure: corruption handling (Appendix A.5). */
  GError *parse_err = NULL;
  SkStateFile *state = sk_state_from_json(contents, &parse_err);
  g_free(contents);

  if (state == NULL)
  {
    /* FR-STATE-13..17, FR-CONN-19: corruption detected. */
    g_warning("sk_state: corrupt state file '%s': %s", path,
              parse_err ? parse_err->message : "unknown");
    g_clear_error(&parse_err);
    rename_corrupt(path);
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_CORRUPT,
                "State file was corrupt and renamed. A fresh state "
                "should be created.");
    return NULL;
  }

  /* FR-STATE-08: version check. */
  int file_version = state->schema_version;

  if (file_version > SK_STATE_SCHEMA_VERSION)
  {
    /* Refuse future versions. */
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_VERSION_FUTURE,
                "State file version %d is newer than supported (%d). "
                "Please upgrade shellkeep.",
                file_version, SK_STATE_SCHEMA_VERSION);
    sk_state_file_free(state);
    return NULL;
  }

  if (file_version < SK_STATE_SCHEMA_VERSION)
  {
    /* Auto-migrate with backup. */
    create_version_backup(path, file_version);

    GError *mig_err = NULL;
    if (!migrate_state(state, file_version, &mig_err))
    {
      g_warning("sk_state: migration failed for '%s': %s", path,
                mig_err ? mig_err->message : "unknown");
      g_propagate_error(error, mig_err);
      sk_state_file_free(state);
      return NULL;
    }
  }

  /* Validate integrity. */
  GError *val_err = NULL;
  if (!sk_state_validate(state, &val_err))
  {
    g_warning("sk_state: validation failed for '%s': %s", path,
              val_err ? val_err->message : "unknown");
    g_propagate_error(error, val_err);
    /* Don't treat validation failure as corruption — return state
     * with warning so caller can decide. For strict mode,
     * caller can choose to discard. */
  }

  return state;
}

/* ---- Local Cache (FR-STATE-01..02) -------------------------------------- */

bool
sk_state_save_local_cache(const SkStateFile *state, const char *host_fingerprint, GError **error)
{
  g_return_val_if_fail(state != NULL, false);
  g_return_val_if_fail(host_fingerprint != NULL, false);

  /* NFR-SEC-06: validate host_fingerprint and client_id to prevent
   * path traversal via crafted values containing '/' or '..'. */
  for (const char *p = host_fingerprint; *p != '\0'; p++)
  {
    char c = *p;
    if (c == '/' || c == '\\')
    {
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO,
                  "Invalid host fingerprint: contains path separator");
      return false;
    }
  }
  if (state->client_id != NULL)
  {
    for (const char *p = state->client_id; *p != '\0'; p++)
    {
      char c = *p;
      if (c == '/' || c == '\\')
      {
        g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO,
                    "Invalid client_id: contains path separator");
        return false;
      }
    }
  }

  g_autofree char *cache_dir = sk_paths_server_cache_dir(host_fingerprint);
  if (cache_dir == NULL)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to get server cache directory");
    return false;
  }

  g_autofree char *path = g_build_filename(cache_dir, state->client_id, NULL);
  g_autofree char *json_path = g_strdup_printf("%s.json", path);

  /* NFR-SEC-06: verify resolved path is within data directory. */
  g_autofree char *data_dir = sk_paths_data_dir();
#ifdef _WIN32
  g_autofree char *real_data = g_strdup(data_dir);
  g_autofree char *real_cache = g_strdup(cache_dir);
#else
  g_autofree char *real_data = realpath(data_dir, NULL);
  g_autofree char *real_cache = realpath(cache_dir, NULL);
#endif
  if (real_data != NULL && real_cache != NULL)
  {
    if (strncmp(real_cache, real_data, strlen(real_data)) != 0)
    {
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO,
                  "Path traversal detected in local cache path");
      return false;
    }
  }

  /* We need a non-const copy to update last_modified. */
  SkStateFile *copy = sk_state_file_copy(state);
  bool ok = sk_state_save(copy, json_path, error);
  sk_state_file_free(copy);
  return ok;
}

SkStateFile *
sk_state_load_local_cache(const char *host_fingerprint, const char *client_id, GError **error)
{
  g_return_val_if_fail(host_fingerprint != NULL, NULL);
  g_return_val_if_fail(client_id != NULL, NULL);

  /* NFR-SEC-06: validate inputs to prevent path traversal. */
  for (const char *p = host_fingerprint; *p != '\0'; p++)
  {
    if (*p == '/' || *p == '\\')
      return NULL;
  }
  for (const char *p = client_id; *p != '\0'; p++)
  {
    if (*p == '/' || *p == '\\')
      return NULL;
  }

  g_autofree char *cache_dir = sk_paths_server_cache_dir(host_fingerprint);
  if (cache_dir == NULL)
    return NULL;

  g_autofree char *path = g_build_filename(cache_dir, client_id, NULL);
  g_autofree char *json_path = g_strdup_printf("%s.json", path);

  if (!g_file_test(json_path, G_FILE_TEST_EXISTS))
  {
    return NULL;
  }

  return sk_state_load(json_path, error);
}
