// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_history.c
 * @brief Session history JSONL/raw management.
 *
 * Implements FR-HISTORY-01 through FR-HISTORY-11:
 * - Append events to JSONL files (append-only, INV-JSONL-1, INV-JSONL-2)
 * - Read events, discarding truncated last line (FR-HISTORY-09)
 * - Rotate files exceeding max size (FR-HISTORY-05)
 * - Cleanup by age and total size (FR-HISTORY-06, FR-HISTORY-07)
 * - UTF-8 validation with replacement character (FR-HISTORY-04)
 * - File permissions 0600 (NFR-SEC-18)
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
#include <unistd.h>

#ifdef _WIN32
#include <io.h>
#define fsync(fd) _commit(fd)
#define fchmod(fd, mode) (0) /* no-op on Windows */
#endif

/* ---- Helpers ------------------------------------------------------------ */

/**
 * Build the JSONL file path for a session.
 * Validates UUID format for path safety (NFR-SEC-06).
 * Caller must g_free().
 */
static char *
history_file_path(const char *session_uuid, const char *base_dir)
{
  g_return_val_if_fail(session_uuid != NULL, NULL);
  g_return_val_if_fail(base_dir != NULL, NULL);

  /* NFR-SEC-06: validate UUID format [a-f0-9-] */
  for (const char *p = session_uuid; *p != '\0'; p++)
  {
    char c = *p;
    if (!((c >= 'a' && c <= 'f') || (c >= '0' && c <= '9') || c == '-'))
    {
      g_warning("sk_history: invalid UUID character in '%s'", session_uuid);
      return NULL;
    }
  }

  g_autofree char *filename = g_strdup_printf("%s.jsonl", session_uuid);
  char *path = g_build_filename(base_dir, filename, NULL);

  /* NFR-SEC-06: verify resolved path is within base_dir. */
#ifdef _WIN32
  g_autofree char *real_base = g_strdup(base_dir);
#else
  g_autofree char *real_base = realpath(base_dir, NULL);
#endif
  if (real_base != NULL)
  {
    /* The file may not exist yet, so check directory component. */
    g_autofree char *dir_part = g_path_get_dirname(path);
#ifdef _WIN32
    g_autofree char *real_dir = g_strdup(dir_part);
#else
    g_autofree char *real_dir = realpath(dir_part, NULL);
#endif
    if (real_dir != NULL && strncmp(real_dir, real_base, strlen(real_base)) != 0)
    {
      g_warning("sk_history: path traversal detected for '%s'", session_uuid);
      g_free(path);
      return NULL;
    }
  }

  return path;
}

static const char *
event_type_to_string(SkHistoryEventType type)
{
  switch (type)
  {
  case SK_HISTORY_OUTPUT:
    return "output";
  case SK_HISTORY_INPUT_ECHO:
    return "input_echo";
  case SK_HISTORY_RESIZE:
    return "resize";
  case SK_HISTORY_META:
    return "meta";
  default:
    return "output";
  }
}

static SkHistoryEventType
event_type_from_string(const char *str)
{
  if (str == NULL || g_strcmp0(str, "output") == 0)
  {
    return SK_HISTORY_OUTPUT;
  }
  if (g_strcmp0(str, "input_echo") == 0)
    return SK_HISTORY_INPUT_ECHO;
  if (g_strcmp0(str, "resize") == 0)
    return SK_HISTORY_RESIZE;
  if (g_strcmp0(str, "meta") == 0)
    return SK_HISTORY_META;
  return SK_HISTORY_OUTPUT;
}

/**
 * Ensure text is valid UTF-8. Replace invalid bytes with U+FFFD.
 * FR-HISTORY-04
 */
static char *
ensure_utf8(const char *text, char **raw_hex_out)
{
  if (text == NULL)
    return g_strdup("");

  if (g_utf8_validate(text, -1, NULL))
  {
    return g_strdup(text);
  }

  /* Contains invalid bytes — replace and optionally record hex. */
  GString *valid = g_string_new(NULL);
  GString *hex = g_string_new(NULL);
  const char *p = text;
  bool has_invalid = false;

  while (*p != '\0')
  {
    const char *end = NULL;
    if (g_utf8_validate(p, -1, &end))
    {
      g_string_append(valid, p);
      break;
    }
    /* Append valid prefix. */
    if (end > p)
    {
      g_string_append_len(valid, p, (gssize)(end - p));
    }
    /* Replace invalid byte. */
    g_string_append(valid, "\xEF\xBF\xBD"); /* U+FFFD */
    if (has_invalid)
      g_string_append_c(hex, ' ');
    g_string_append_printf(hex, "%02x", (unsigned char)*end);
    has_invalid = true;
    p = end + 1;
  }

  if (has_invalid && raw_hex_out != NULL)
  {
    *raw_hex_out = g_string_free(hex, FALSE);
  }
  else
  {
    g_string_free(hex, TRUE);
  }

  return g_string_free(valid, FALSE);
}

/* ---- Event Serialization ------------------------------------------------ */

char *
sk_history_event_to_json(const SkHistoryEvent *event)
{
  g_return_val_if_fail(event != NULL, NULL);

  JsonObject *obj = json_object_new();

  json_object_set_string_member(obj, "ts", event->ts ? event->ts : "");

  if (event->type != SK_HISTORY_RESIZE)
  {
    /* FR-HISTORY-04: ensure valid UTF-8. */
    g_autofree char *raw_hex = NULL;
    g_autofree char *safe_text = ensure_utf8(event->text, &raw_hex);
    json_object_set_string_member(obj, "text", safe_text);
    if (raw_hex != NULL)
    {
      json_object_set_string_member(obj, "raw_hex", raw_hex);
    }
  }

  json_object_set_string_member(obj, "type", event_type_to_string(event->type));

  if (event->type == SK_HISTORY_RESIZE)
  {
    JsonObject *size = json_object_new();
    json_object_set_int_member(size, "cols", event->size.cols);
    json_object_set_int_member(size, "rows", event->size.rows);
    json_object_set_object_member(obj, "size", size);
  }

  JsonNode *node = json_node_new(JSON_NODE_OBJECT);
  json_node_take_object(node, obj);

  JsonGenerator *gen = json_generator_new();
  json_generator_set_pretty(gen, FALSE);
  json_generator_set_root(gen, node);

  gsize len;
  char *json = json_generator_to_data(gen, &len);

  g_object_unref(gen);
  json_node_unref(node);

  return json;
}

SkHistoryEvent *
sk_history_event_from_json(const char *json_line)
{
  g_return_val_if_fail(json_line != NULL, NULL);

  JsonParser *parser = json_parser_new();
  GError *err = NULL;
  if (!json_parser_load_from_data(parser, json_line, -1, &err))
  {
    g_clear_error(&err);
    g_object_unref(parser);
    return NULL;
  }

  JsonNode *root = json_parser_get_root(parser);
  if (root == NULL || !JSON_NODE_HOLDS_OBJECT(root))
  {
    g_object_unref(parser);
    return NULL;
  }

  JsonObject *obj = json_node_get_object(root);

  SkHistoryEvent *event = g_new0(SkHistoryEvent, 1);
  event->ts = g_strdup(json_object_get_string_member_with_default(obj, "ts", ""));

  const char *type_str = json_object_get_string_member_with_default(obj, "type", "output");
  event->type = event_type_from_string(type_str);

  if (json_object_has_member(obj, "text"))
  {
    event->text = g_strdup(json_object_get_string_member(obj, "text"));
  }

  if (json_object_has_member(obj, "raw_hex"))
  {
    event->raw_hex = g_strdup(json_object_get_string_member(obj, "raw_hex"));
  }

  if (event->type == SK_HISTORY_RESIZE && json_object_has_member(obj, "size"))
  {
    JsonObject *size = json_object_get_object_member(obj, "size");
    if (size != NULL)
    {
      event->size.cols = (int)json_object_get_int_member_with_default(size, "cols", 80);
      event->size.rows = (int)json_object_get_int_member_with_default(size, "rows", 24);
    }
  }

  g_object_unref(parser);
  return event;
}

void
sk_history_event_free(SkHistoryEvent *event)
{
  if (event == NULL)
    return;
  g_free(event->ts);
  g_free(event->text);
  g_free(event->raw_hex);
  g_free(event);
}

/* ---- Append (INV-JSONL-1, INV-JSONL-2) --------------------------------- */

bool
sk_history_append(const char *session_uuid, const SkHistoryEvent *event, const char *base_dir,
                  GError **error)
{
  g_return_val_if_fail(session_uuid != NULL, false);
  g_return_val_if_fail(event != NULL, false);
  g_return_val_if_fail(base_dir != NULL, false);

  /* Ensure base directory exists. */
  if (!g_file_test(base_dir, G_FILE_TEST_IS_DIR))
  {
    g_mkdir_with_parents(base_dir, SK_DIR_PERMISSIONS);
  }

  g_autofree char *path = history_file_path(session_uuid, base_dir);
  if (path == NULL)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Invalid session UUID for history path");
    return false;
  }

  g_autofree char *json_line = sk_history_event_to_json(event);
  if (json_line == NULL)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to serialize history event");
    return false;
  }

  /* Append with newline. FR-HISTORY-08: append-only. */
  int fd = open(path, O_WRONLY | O_CREAT | O_APPEND, SK_FILE_PERMISSIONS);
  if (fd < 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Failed to open history file '%s': %s",
                path, g_strerror(errno));
    return false;
  }

  /* Ensure 0600 on creation. NFR-SEC-18 */
  fchmod(fd, SK_FILE_PERMISSIONS);

  gsize json_len = strlen(json_line);

  /* Write JSON line + newline. Use writev-style approach for atomicity. */
  g_autofree char *line = g_strdup_printf("%s\n", json_line);
  gsize line_len = json_len + 1;
  gssize total = 0;

  while ((gsize)total < line_len)
  {
    gssize w = write(fd, line + total, line_len - (gsize)total);
    if (w < 0)
    {
      if (errno == EINTR)
        continue;
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Write to history failed: %s",
                  g_strerror(errno));
      close(fd);
      return false;
    }
    total += w;
  }

  close(fd);
  return true;
}

/* ---- Read (FR-HISTORY-09) ----------------------------------------------- */

SkHistoryEvent **
sk_history_read(const char *session_uuid, const char *base_dir, int *n_events, GError **error)
{
  g_return_val_if_fail(session_uuid != NULL, NULL);
  g_return_val_if_fail(base_dir != NULL, NULL);
  g_return_val_if_fail(n_events != NULL, NULL);

  *n_events = 0;

  g_autofree char *path = history_file_path(session_uuid, base_dir);
  if (path == NULL)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Invalid session UUID");
    return NULL;
  }

  if (!g_file_test(path, G_FILE_TEST_EXISTS))
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "History file not found: %s", path);
    return NULL;
  }

  gchar *contents = NULL;
  gsize length = 0;
  if (!g_file_get_contents(path, &contents, &length, error))
  {
    return NULL;
  }

  /* Split into lines. */
  gchar **lines = g_strsplit(contents, "\n", -1);
  g_free(contents);

  /* Count non-empty lines. */
  int count = 0;
  for (int i = 0; lines[i] != NULL; i++)
  {
    if (lines[i][0] != '\0')
      count++;
  }

  if (count == 0)
  {
    g_strfreev(lines);
    return NULL;
  }

  GPtrArray *events = g_ptr_array_new();

  for (int i = 0; lines[i] != NULL; i++)
  {
    if (lines[i][0] == '\0')
      continue;

    SkHistoryEvent *ev = sk_history_event_from_json(lines[i]);
    if (ev != NULL)
    {
      g_ptr_array_add(events, ev);
    }
    else
    {
      /*
       * FR-HISTORY-09: discard invalid lines.
       * If this is the last non-empty line, it may be truncated
       * from a crash during append — expected behavior.
       * For mid-file invalid lines, log a warning but continue.
       */
      bool is_last = true;
      for (int j = i + 1; lines[j] != NULL; j++)
      {
        if (lines[j][0] != '\0')
        {
          is_last = false;
          break;
        }
      }
      if (!is_last)
      {
        g_info("sk_history: discarding invalid line %d in %s", i + 1, path);
      }
    }
  }

  g_strfreev(lines);

  *n_events = (int)events->len;
  if (events->len == 0)
  {
    g_ptr_array_free(events, TRUE);
    return NULL;
  }

  /* Convert GPtrArray to plain array. */
  g_ptr_array_add(events, NULL); /* NULL-terminate */
  SkHistoryEvent **result = (SkHistoryEvent **)g_ptr_array_free(events, FALSE);
  return result;
}

/* ---- Rotation (FR-HISTORY-05) ------------------------------------------- */

bool
sk_history_rotate(const char *session_uuid, const char *base_dir, int max_size_mb, GError **error)
{
  g_return_val_if_fail(session_uuid != NULL, false);
  g_return_val_if_fail(base_dir != NULL, false);

  g_autofree char *path = history_file_path(session_uuid, base_dir);
  if (path == NULL)
    return false;

  if (!g_file_test(path, G_FILE_TEST_EXISTS))
  {
    return true; /* Nothing to rotate. */
  }

  struct stat st;
  if (stat(path, &st) != 0)
    return true;

  gint64 max_bytes = (gint64)max_size_mb * 1024 * 1024;
  if (st.st_size <= max_bytes)
  {
    return true; /* Under limit. */
  }

  /* FR-HISTORY-05: Truncate oldest 25% via temp+rename. */
  gchar *contents = NULL;
  gsize length = 0;
  if (!g_file_get_contents(path, &contents, &length, error))
  {
    return false;
  }

  /* Find the byte offset at 25% of content. */
  gsize cut_point = length / 4;

  /* Advance to the next newline after cut_point to avoid splitting a line. */
  while (cut_point < length && contents[cut_point] != '\n')
  {
    cut_point++;
  }
  if (cut_point < length)
  {
    cut_point++; /* Skip the newline itself. */
  }

  if (cut_point >= length)
  {
    /* File is effectively all one line — just truncate. */
    g_free(contents);
    return true;
  }

  /* Write remaining content to tmp, then rename. Atomic operation. */
  g_autofree char *tmp = g_strdup_printf("%s.tmp", path);

  int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, SK_FILE_PERMISSIONS);
  if (fd < 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO,
                "Failed to open tmp file for rotation: %s", g_strerror(errno));
    g_free(contents);
    return false;
  }

  gsize remaining = length - cut_point;
  gssize total = 0;
  while ((gsize)total < remaining)
  {
    gssize w = write(fd, contents + cut_point + total, remaining - (gsize)total);
    if (w < 0)
    {
      if (errno == EINTR)
        continue;
      g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Write during rotation failed: %s",
                  g_strerror(errno));
      close(fd);
      g_unlink(tmp);
      g_free(contents);
      return false;
    }
    total += w;
  }

  fsync(fd);
  close(fd);
  g_free(contents);

  if (g_rename(tmp, path) != 0)
  {
    g_set_error(error, SK_STATE_ERROR, SK_STATE_ERROR_IO, "Rename during rotation failed: %s",
                g_strerror(errno));
    g_unlink(tmp);
    return false;
  }

  sk_permissions_fix_file(path);

  g_info("sk_history: rotated '%s' — removed oldest 25%% "
         "(cut at byte %zu of %zu)",
         path, cut_point, length);

  return true;
}

/* ---- Cleanup (FR-HISTORY-06, FR-HISTORY-07) ----------------------------- */

/** Helper struct for sorting files by modification time. */
typedef struct
{
  char *path;
  time_t mtime;
  gint64 size;
} HistoryFileInfo;

static int
compare_by_mtime_asc(const void *a, const void *b)
{
  const HistoryFileInfo *fa = *(const HistoryFileInfo *const *)a;
  const HistoryFileInfo *fb = *(const HistoryFileInfo *const *)b;
  if (fa->mtime < fb->mtime)
    return -1;
  if (fa->mtime > fb->mtime)
    return 1;
  return 0;
}

bool
sk_history_cleanup(const char *base_dir, int max_days, int max_total_mb, GError **error)
{
  g_return_val_if_fail(base_dir != NULL, false);
  (void)error; /* cleanup is best-effort */

  GDir *dir = g_dir_open(base_dir, 0, NULL);
  if (dir == NULL)
    return true;

  time_t now = time(NULL);
  time_t max_age_sec = (time_t)max_days * 24 * 60 * 60;
  gint64 max_total_bytes = (gint64)max_total_mb * 1024 * 1024;

  GPtrArray *files = g_ptr_array_new();
  gint64 total_size = 0;

  const char *name;
  while ((name = g_dir_read_name(dir)) != NULL)
  {
    if (!g_str_has_suffix(name, ".jsonl") && !g_str_has_suffix(name, ".raw"))
    {
      continue;
    }

    g_autofree char *full = g_build_filename(base_dir, name, NULL);
    struct stat st;
    if (stat(full, &st) != 0)
      continue;

    HistoryFileInfo *info = g_new0(HistoryFileInfo, 1);
    info->path = g_strdup(full);
    info->mtime = st.st_mtime;
    info->size = st.st_size;
    total_size += st.st_size;

    g_ptr_array_add(files, info);
  }
  g_dir_close(dir);

  /* Sort by mtime ascending (oldest first). */
  g_ptr_array_sort(files, compare_by_mtime_asc);

  /* FR-HISTORY-06: remove files older than max_days. */
  for (guint i = 0; i < files->len; i++)
  {
    HistoryFileInfo *info = g_ptr_array_index(files, i);
    double age = difftime(now, info->mtime);
    if (age > (double)max_age_sec)
    {
      g_info("sk_history: removing aged file '%s' (%.0f days old)", info->path, age / 86400.0);
      g_unlink(info->path);
      total_size -= info->size;
      info->size = 0; /* Mark as removed. */
    }
  }

  /* FR-HISTORY-07: if total still exceeds max, remove oldest until under. */
  for (guint i = 0; i < files->len && total_size > max_total_bytes; i++)
  {
    HistoryFileInfo *info = g_ptr_array_index(files, i);
    if (info->size == 0)
      continue; /* Already removed. */

    g_info("sk_history: removing '%s' to reduce total size "
           "(total=%" G_GINT64_FORMAT ", limit=%" G_GINT64_FORMAT ")",
           info->path, total_size, max_total_bytes);
    g_unlink(info->path);
    total_size -= info->size;
    info->size = 0;
  }

  /* Free file info array. */
  for (guint i = 0; i < files->len; i++)
  {
    HistoryFileInfo *info = g_ptr_array_index(files, i);
    g_free(info->path);
    g_free(info);
  }
  g_ptr_array_free(files, TRUE);

  return true;
}
