// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_state_debounce.c
 * @brief Debounced state saving — max 1 write per 2 seconds.
 *
 * Implements FR-STATE-03, FR-STATE-06, NFR-PERF-07.
 * Uses g_timeout_add() for debounce timing, GTask for async file I/O.
 * Never blocks the GTK main loop (FR-STATE-06).
 */

#include "shellkeep/sk_state.h"

#include <glib.h>

#include <gio/gio.h>
#include <string.h>

/* ---- Internal: deep copy via JSON round-trip ---------------------------- */

static SkStateFile *
copy_state_via_json(const SkStateFile *state)
{
  char *json = sk_state_to_json(state);
  if (json == NULL)
    return NULL;

  GError *err = NULL;
  SkStateFile *copy = sk_state_from_json(json, &err);
  g_free(json);
  if (err != NULL)
  {
    g_warning("sk_state_debounce: copy failed: %s", err->message);
    g_error_free(err);
  }
  return copy;
}

/* ---- Debounce context --------------------------------------------------- */

struct _SkStateDebounce
{
  char *save_path;
  char *host_fingerprint; /* nullable */
  SkStateFile *pending;   /* pending state to save (owned) */
  guint timer_id;         /* GSource ID for the timeout, 0 if none */
  bool save_in_progress;  /* guard against re-entry */
};

/* ---- Forward declarations ----------------------------------------------- */

static gboolean debounce_timer_cb(gpointer user_data);
static void launch_async_save(SkStateDebounce *db, SkStateFile *state);

/* ---- GTask-based async save (FR-STATE-06) ------------------------------- */

/** Data passed to the GTask worker thread. */
typedef struct
{
  SkStateFile *state;     /* owned */
  char *save_path;        /* owned */
  char *host_fingerprint; /* owned, nullable */
} SaveTaskData;

static void
save_task_data_free(SaveTaskData *data)
{
  if (data == NULL)
    return;
  sk_state_file_free(data->state);
  g_free(data->save_path);
  g_free(data->host_fingerprint);
  g_free(data);
}

/**
 * Worker thread function for async save.
 * Runs in a GTask thread pool, never on the main loop. (FR-STATE-06)
 */
static void
save_worker(GTask *task, gpointer source_object, gpointer task_data, GCancellable *cancellable)
{
  (void)source_object;
  (void)cancellable;

  SaveTaskData *data = (SaveTaskData *)task_data;

  /* Save to primary path. */
  GError *err = NULL;
  if (!sk_state_save(data->state, data->save_path, &err))
  {
    g_warning("sk_state_debounce: save to '%s' failed: %s", data->save_path,
              err ? err->message : "unknown");
    g_clear_error(&err);
  }

  /* Also save to local cache if fingerprint is set. FR-STATE-01 */
  if (data->host_fingerprint != NULL)
  {
    GError *cache_err = NULL;
    if (!sk_state_save_local_cache(data->state, data->host_fingerprint, &cache_err))
    {
      g_warning("sk_state_debounce: local cache save failed: %s",
                cache_err ? cache_err->message : "unknown");
      g_clear_error(&cache_err);
    }
  }

  g_task_return_boolean(task, TRUE);
}

/**
 * Completion callback — runs on main loop after worker finishes.
 */
static void
save_task_complete(GObject *source_object, GAsyncResult *result, gpointer user_data)
{
  (void)source_object;
  (void)result;

  SkStateDebounce *db = (SkStateDebounce *)user_data;
  db->save_in_progress = false;

  /* If another save was requested while we were writing, schedule it. */
  if (db->pending != NULL && db->timer_id == 0)
  {
    db->timer_id = g_timeout_add(SK_STATE_DEBOUNCE_INTERVAL_MS, debounce_timer_cb, db);
  }
}

static void
launch_async_save(SkStateDebounce *db, SkStateFile *state)
{
  db->save_in_progress = true;

  SaveTaskData *data = g_new0(SaveTaskData, 1);
  data->state = state; /* takes ownership */
  data->save_path = g_strdup(db->save_path);
  data->host_fingerprint = g_strdup(db->host_fingerprint);

  GTask *task = g_task_new(NULL, NULL, save_task_complete, db);
  g_task_set_task_data(task, data, (GDestroyNotify)save_task_data_free);
  g_task_run_in_thread(task, save_worker);
  g_object_unref(task);
}

/* ---- Timer callback ----------------------------------------------------- */

static gboolean
debounce_timer_cb(gpointer user_data)
{
  SkStateDebounce *db = (SkStateDebounce *)user_data;

  db->timer_id = 0; /* one-shot timer */

  if (db->pending == NULL)
  {
    return G_SOURCE_REMOVE;
  }

  if (db->save_in_progress)
  {
    /* A save is still running — reschedule. */
    db->timer_id = g_timeout_add(SK_STATE_DEBOUNCE_INTERVAL_MS, debounce_timer_cb, db);
    return G_SOURCE_REMOVE;
  }

  SkStateFile *to_save = db->pending;
  db->pending = NULL;

  launch_async_save(db, to_save);

  return G_SOURCE_REMOVE;
}

/* ---- Synchronous save (for flush/shutdown) ------------------------------ */

static void
save_sync(SkStateDebounce *db, SkStateFile *state)
{
  GError *err = NULL;
  if (!sk_state_save(state, db->save_path, &err))
  {
    g_warning("sk_state_debounce: sync save to '%s' failed: %s", db->save_path,
              err ? err->message : "unknown");
    g_clear_error(&err);
  }

  if (db->host_fingerprint != NULL)
  {
    GError *cache_err = NULL;
    if (!sk_state_save_local_cache(state, db->host_fingerprint, &cache_err))
    {
      g_warning("sk_state_debounce: sync cache save failed: %s",
                cache_err ? cache_err->message : "unknown");
      g_clear_error(&cache_err);
    }
  }

  sk_state_file_free(state);
}

/* ---- Public API --------------------------------------------------------- */

SkStateDebounce *
sk_state_debounce_new(const char *save_path, const char *host_fingerprint)
{
  g_return_val_if_fail(save_path != NULL, NULL);

  SkStateDebounce *db = g_new0(SkStateDebounce, 1);
  db->save_path = g_strdup(save_path);
  db->host_fingerprint = g_strdup(host_fingerprint); /* NULL-safe */
  db->pending = NULL;
  db->timer_id = 0;
  db->save_in_progress = false;
  return db;
}

/* FR-STATE-03: max 1 write every 2 seconds. */
void
sk_state_schedule_save(SkStateDebounce *debounce, const SkStateFile *state)
{
  g_return_if_fail(debounce != NULL);
  g_return_if_fail(state != NULL);

  /* Replace any previously pending state. */
  if (debounce->pending != NULL)
  {
    sk_state_file_free(debounce->pending);
  }
  debounce->pending = copy_state_via_json(state);

  /* If no timer is running and no save in progress, start one. */
  if (debounce->timer_id == 0 && !debounce->save_in_progress)
  {
    debounce->timer_id = g_timeout_add(SK_STATE_DEBOUNCE_INTERVAL_MS, debounce_timer_cb, debounce);
  }
  /* If a timer/save is already running, the pending state will be
   * picked up when it completes. At most one write per interval. */
}

void
sk_state_debounce_flush(SkStateDebounce *debounce)
{
  g_return_if_fail(debounce != NULL);

  /* Cancel pending timer. */
  if (debounce->timer_id != 0)
  {
    g_source_remove(debounce->timer_id);
    debounce->timer_id = 0;
  }

  /* Synchronous save of pending state (for shutdown). */
  if (debounce->pending != NULL)
  {
    SkStateFile *to_save = debounce->pending;
    debounce->pending = NULL;
    save_sync(debounce, to_save);
  }
}

void
sk_state_debounce_free(SkStateDebounce *debounce)
{
  if (debounce == NULL)
    return;

  /* Flush any pending save before freeing. */
  sk_state_debounce_flush(debounce);

  g_free(debounce->save_path);
  g_free(debounce->host_fingerprint);

  /* pending should be NULL after flush, but be safe. */
  if (debounce->pending != NULL)
  {
    sk_state_file_free(debounce->pending);
  }

  g_free(debounce);
}
